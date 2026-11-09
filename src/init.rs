//! `agentkey init`: register the MCP server, install the paste hook, and
//! tell the agent (via CLAUDE.md) to use agentkey instead of asking for tokens.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};

const MD_START: &str = "<!-- agentkey:start -->";
const MD_END: &str = "<!-- agentkey:end -->";
const MD_BLOCK: &str = "## Credentials (agentkey)
- API tokens and passwords are stored in agentkey. Use the `agentkey` MCP tools (`list_credentials`, then `http_request`) for any service that needs auth, e.g. Bamboo, GitHub Actions, GitLab, Jenkins.
- Never ask me to paste a token, password or API key into the chat. If a credential is missing, call `add_credential`: it opens a secure dialog where I type the secret. Ask me for the base URL first if the service needs one.
- Never try to read secrets from the keychain, `security`, or agentkey's files.";

fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("HOME is not set"))
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

pub fn run(dry_run: bool) -> Result<()> {
    let exe = std::env::current_exe()?.canonicalize()?;
    let exe_s = exe.to_string_lossy().to_string();
    let home = home()?;
    let say = |done: &str, plan: &str| {
        println!(
            "{} {}",
            if dry_run { "would" } else { "✓" },
            if dry_run { plan } else { done }
        )
    };

    // 1. MCP server
    let has_claude = Command::new("claude").arg("--version").output().is_ok();
    if has_claude {
        let exists = Command::new("claude")
            .args(["mcp", "get", "agentkey"])
            .output()
            .is_ok_and(|o| o.status.success());
        if exists {
            println!("· MCP server `agentkey` already registered in Claude Code");
        } else {
            if !dry_run {
                let status = Command::new("claude")
                    .args([
                        "mcp", "add", "--scope", "user", "agentkey", "--", &exe_s, "mcp",
                    ])
                    .status()
                    .context("running `claude mcp add`")?;
                if !status.success() {
                    return Err(anyhow!("`claude mcp add` failed"));
                }
            }
            say(
                "registered MCP server `agentkey` (user scope)",
                &format!("run: claude mcp add --scope user agentkey -- {exe_s} mcp"),
            );
        }
    } else {
        println!(
            "· `claude` not found; add the MCP server manually: command {exe_s}, args [\"mcp\"]"
        );
    }

    // 2. Paste hook
    let settings = home.join(".claude").join("settings.json");
    let command = format!("{} hook prompt", shell_quote(&exe_s));
    if install_hook(&settings, &command, dry_run)? {
        say(
            &format!("installed paste-to-save hook in {}", settings.display()),
            &format!(
                "add a UserPromptSubmit hook to {}: {command}",
                settings.display()
            ),
        );
    } else {
        println!("· paste-to-save hook already installed");
    }

    // 3. Agent instructions
    let md = home.join(".claude").join("CLAUDE.md");
    if install_md(&md, dry_run)? {
        say(
            &format!("added agentkey instructions to {}", md.display()),
            &format!("append an agentkey section to {}", md.display()),
        );
    } else {
        println!("· {} already mentions agentkey", md.display());
    }

    println!(
        "\nOther agents (manual for now):\n  Codex  ~/.codex/config.toml\n    [mcp_servers.agentkey]\n    command = \"{exe_s}\"\n    args = [\"mcp\"]\n  Cursor ~/.cursor/mcp.json\n    \"agentkey\": {{ \"command\": \"{exe_s}\", \"args\": [\"mcp\"] }}"
    );
    if !dry_run {
        println!("\nRestart Claude Code, then try: agentkey add bamboo");
    }
    Ok(())
}

/// Adds the hook unless an agentkey hook is already there. Returns whether it changed anything.
pub fn install_hook(path: &Path, command: &str, dry_run: bool) -> Result<bool> {
    let mut root: Value = match std::fs::read_to_string(path) {
        Ok(text) if !text.trim().is_empty() => {
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?
        }
        Ok(_) => json!({}),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(e.into()),
    };
    let obj = root
        .as_object_mut()
        .ok_or_else(|| anyhow!("{} is not a JSON object", path.display()))?;
    let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
    let list = hooks
        .as_object_mut()
        .ok_or_else(|| anyhow!("`hooks` in {} is not an object", path.display()))?
        .entry("UserPromptSubmit")
        .or_insert_with(|| json!([]));
    let arr = list
        .as_array_mut()
        .ok_or_else(|| anyhow!("`hooks.UserPromptSubmit` is not an array"))?;
    let already = arr.iter().any(|group| {
        group["hooks"].as_array().is_some_and(|hs| {
            hs.iter().any(|h| {
                h["command"]
                    .as_str()
                    .is_some_and(|c| c.contains("agentkey") && c.contains("hook prompt"))
            })
        })
    });
    if already {
        return Ok(false);
    }
    arr.push(json!({ "hooks": [{ "type": "command", "command": command, "timeout": 10 }] }));
    if !dry_run {
        if path.exists() {
            std::fs::copy(path, path.with_extension("json.agentkey-bak"))?;
        } else if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(&root)? + "\n")?;
    }
    Ok(true)
}

pub fn install_md(path: &Path, dry_run: bool) -> Result<bool> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    if let (Some(start), Some(end)) = (existing.find(MD_START), existing.find(MD_END)) {
        // Already there: refresh it if this version's text differs.
        let wanted = format!("{MD_START}\n{MD_BLOCK}\n{MD_END}");
        let end = end + MD_END.len();
        if start > end || existing[start..end] == wanted {
            return Ok(false);
        }
        if !dry_run {
            let updated = format!("{}{wanted}{}", &existing[..start], &existing[end..]);
            std::fs::write(path, updated)?;
        }
        return Ok(true);
    }
    if !dry_run {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let sep = if existing.is_empty() || existing.ends_with("\n\n") {
            ""
        } else if existing.ends_with('\n') {
            "\n"
        } else {
            "\n\n"
        };
        std::fs::write(
            path,
            format!("{existing}{sep}{MD_START}\n{MD_BLOCK}\n{MD_END}\n"),
        )?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_install_preserves_other_settings_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(
            &p,
            r#"{"model":"opus","hooks":{"UserPromptSubmit":[{"hooks":[{"type":"command","command":"other"}]}]}}"#,
        )
        .unwrap();
        assert!(install_hook(&p, "'/bin/agentkey' hook prompt", false).unwrap());
        assert!(!install_hook(&p, "'/bin/agentkey' hook prompt", false).unwrap());
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["model"], "opus");
        assert_eq!(v["hooks"]["UserPromptSubmit"].as_array().unwrap().len(), 2);
        assert!(p.with_extension("json.agentkey-bak").exists());
    }

    #[test]
    fn md_block_added_once() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("CLAUDE.md");
        std::fs::write(&p, "# mine\n").unwrap();
        assert!(install_md(&p, false).unwrap());
        assert!(!install_md(&p, false).unwrap());
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.starts_with("# mine\n\n<!-- agentkey:start -->"));
    }

    #[test]
    fn outdated_md_block_is_replaced_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("CLAUDE.md");
        std::fs::write(
            &p,
            format!("# mine\n\n{MD_START}\nold text\n{MD_END}\n\n# after\n"),
        )
        .unwrap();
        assert!(install_md(&p, false).unwrap());
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(!text.contains("old text") && text.contains("add_credential"));
        assert!(text.starts_with("# mine\n\n") && text.ends_with("\n\n# after\n"));
        assert!(!install_md(&p, false).unwrap());
    }

    #[test]
    fn quotes_paths_with_spaces() {
        assert_eq!(shell_quote("/a b/agentkey"), "'/a b/agentkey'");
    }
}
