//! Claude Code `UserPromptSubmit` hook. It runs locally before the prompt
//! is sent. When a secret is found, it is saved to the keychain and the
//! prompt is blocked, so the secret never reaches the model.

use anyhow::Result;
use serde::Deserialize;

use crate::detect;
use crate::presets::{self, AuthKind};
use crate::store::{Auth, Credential, Store, Vault, host_of};

/// Put this anywhere in a prompt to send it as-is.
pub const IGNORE_MARKER: &str = "#agentkey-ignore";

#[derive(Deserialize)]
struct HookInput {
    #[serde(default)]
    prompt: String,
}

/// Returns the JSON to print on stdout, or None to let the prompt through.
pub fn run(stdin: &str, store: &mut Store, vault: &dyn Vault) -> Option<String> {
    let input: HookInput = serde_json::from_str(stdin).ok()?;
    let reason = check(&input.prompt, store, vault)?;
    Some(serde_json::json!({ "decision": "block", "reason": reason }).to_string())
}

/// Returns the message shown to the user when the prompt must be blocked.
pub fn check(prompt: &str, store: &mut Store, vault: &dyn Vault) -> Option<String> {
    if prompt.contains(IGNORE_MARKER) {
        return None;
    }
    let found = detect::scan(prompt);
    if found.is_empty() {
        return None;
    }

    let mut lines = vec![format!(
        "agentkey caught {} secret{} in your message and did NOT send it to the model.",
        found.len(),
        if found.len() == 1 { "" } else { "s" }
    )];
    let mut first_name = None;
    for f in &found {
        match save(f, store, vault) {
            Ok((name, todo)) => {
                let title = presets::get_or_generic(&store.creds[&name].service).title;
                lines.push(format!("  ✓ saved as `{name}` ({title}){todo}"));
                first_name.get_or_insert(name);
            }
            Err(e) => lines.push(format!(
                "  ✗ could not save it ({e:#}); run `agentkey add` instead"
            )),
        }
    }
    if let Some(name) = first_name {
        lines.push(format!(
            "Re-send your message without the secret. The agent can use `{name}` by name."
        ));
    }
    lines.push(format!(
        "(False alarm? Add {IGNORE_MARKER} to the message to send it unchanged.)"
    ));
    Some(lines.join("\n"))
}

fn save(found: &detect::Found, store: &mut Store, vault: &dyn Vault) -> Result<(String, String)> {
    let preset = presets::get_or_generic(found.service.unwrap_or("generic"));
    let name = store.free_name(preset.id);
    let url = preset.default_url.unwrap_or("").to_string();
    let hosts = if url.is_empty() {
        vec![]
    } else {
        vec![host_of(&url)?]
    };
    let auth = match preset.auth {
        AuthKind::Bearer => Auth::Bearer,
        AuthKind::Basic => Auth::Basic {
            username: String::new(),
        },
        AuthKind::Header(h) => Auth::Header { name: h.into() },
    };

    let mut todo = Vec::new();
    if url.is_empty() {
        todo.push(format!(
            "--url {}",
            preset
                .url_example
                .split_whitespace()
                .next()
                .unwrap_or("<url>")
        ));
    }
    if matches!(auth, Auth::Basic { .. }) {
        todo.push("--username <you>".to_string());
    }
    if preset.id == "generic" {
        todo.push("--service <bamboo|github|...>".to_string());
    }

    vault.set(&name, &found.value)?;
    store.creds.insert(
        name.clone(),
        Credential {
            service: preset.id.into(),
            url,
            description: String::new(),
            auth,
            hosts,
            added_by: "paste-hook".into(),
        },
    );
    if let Err(e) = store.save() {
        let _ = vault.delete(&name);
        store.creds.remove(&name);
        return Err(e);
    }

    let todo = if todo.is_empty() {
        String::new()
    } else {
        format!(
            "\n      finish setup: ask your agent to finish setting up `{name}`, or run: agentkey edit {name} {}",
            todo.join(" ")
        )
    };
    Ok((name, todo))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::testing::MemVault;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::load_from(dir.path().join("credentials.toml")).unwrap();
        (dir, s)
    }

    #[test]
    fn blocks_and_saves_bamboo_token() {
        let (_d, mut s) = store();
        let v = MemVault::default();
        let input = r#"{"prompt":"my bamboo token is NjQ3MjE4MzQ1Njc4OkxQmPq9Zr2Wd+Hs8 check the build","session_id":"x"}"#;
        let out = run(input, &mut s, &v).expect("should block");
        let json: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(json["decision"], "block");
        let reason = json["reason"].as_str().unwrap();
        assert!(reason.contains("saved as `bamboo`"));
        assert!(reason.contains("agentkey edit bamboo --url"));
        assert!(
            !reason.contains("NjQ3MjE4"),
            "the secret must not be echoed"
        );
        assert_eq!(
            v.get("bamboo").unwrap(),
            "NjQ3MjE4MzQ1Njc4OkxQmPq9Zr2Wd+Hs8"
        );
        assert_eq!(s.creds["bamboo"].added_by, "paste-hook");
    }

    #[test]
    fn github_token_is_ready_to_use() {
        let (_d, mut s) = store();
        let v = MemVault::default();
        let reason = check("ghp_abcdefghijklmnopqrstuvwxyz0123456789", &mut s, &v).unwrap();
        assert!(!reason.contains("finish setup"));
        assert_eq!(s.creds["github"].hosts, vec!["api.github.com"]);
    }

    #[test]
    fn second_token_gets_new_name() {
        let (_d, mut s) = store();
        let v = MemVault::default();
        check("ghp_abcdefghijklmnopqrstuvwxyz0123456789", &mut s, &v);
        check("ghp_zyxwvutsrqponmlkjihgfedcba9876543210", &mut s, &v);
        assert!(s.creds.contains_key("github-2"));
    }

    #[test]
    fn lets_clean_prompts_and_ignore_marker_through() {
        let (_d, mut s) = store();
        let v = MemVault::default();
        assert!(run(r#"{"prompt":"check my bamboo pipeline"}"#, &mut s, &v).is_none());
        let p = format!("ghp_abcdefghijklmnopqrstuvwxyz0123456789 {IGNORE_MARKER}");
        assert!(check(&p, &mut s, &v).is_none());
        assert!(s.creds.is_empty());
    }
}
