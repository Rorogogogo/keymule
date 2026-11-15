//! Saving a pasted secret as a credential or an MCP server secret, and
//! finding one that's already saved.

use super::*;

/// The name a secret already has in keymule, if it's stored.
pub(super) fn known_as(found: &detect::Found, store: &Store, vault: &dyn Vault) -> Option<String> {
    if found.service == Some(detect::DATABASE) {
        return store
            .servers
            .keys()
            .find(|n| {
                crate::servers::Binding::load(vault, n)
                    .is_ok_and(|b| b.env.values().any(|v| *v == found.value))
            })
            .cloned();
    }
    store
        .creds
        .keys()
        .find(|n| Sealed::peek_secret(vault, n).as_deref() == Some(found.value.as_str()))
        .cloned()
}

/// A database URL becomes an MCP server secret (env `DATABASE_URL`, which
/// `keymule exec --env` can rename). No command may use it until the user
/// approves one.
pub(super) fn save_database(
    found: &detect::Found,
    name: &str,
    store: &mut Store,
    vault: &dyn Vault,
) -> Result<String> {
    let name = name.to_string();
    let mut binding = crate::servers::Binding::default();
    binding
        .env
        .insert("DATABASE_URL".into(), found.value.clone());
    binding.save(vault, &name)?;
    store.servers.insert(
        name.clone(),
        crate::store::ServerMeta {
            env: vec!["DATABASE_URL".into()],
            ..Default::default()
        },
    );
    if let Err(e) = store.save() {
        let _ = crate::servers::Binding::delete(vault, &name);
        store.servers.remove(&name);
        return Err(e);
    }
    Ok(name)
}

pub(super) fn save(
    found: &detect::Found,
    name: &str,
    description: &str,
    store: &mut Store,
    vault: &dyn Vault,
) -> Result<(String, String)> {
    let preset = presets::get_or_generic(found.service.unwrap_or("generic"));
    let name = name.to_string();
    let url = preset.default_url.unwrap_or("").to_string();
    let hosts = presets::hosts_for(preset, &url)?;
    let auth = match preset.auth {
        AuthKind::Bearer => Auth::Bearer,
        AuthKind::Basic => Auth::Basic {
            username: String::new(),
        },
        AuthKind::Header(h) => Auth::Header { name: h.into() },
    };

    let mut todo = Vec::new();
    let http = preset.kind == presets::Kind::Http;
    if http && url.is_empty() {
        todo.push(format!(
            "--url {}",
            preset
                .url_example
                .split_whitespace()
                .next()
                .unwrap_or("<url>")
        ));
    }
    if http && matches!(auth, Auth::Basic { .. }) {
        todo.push("--username <you>".to_string());
    }
    if preset.id == "generic" {
        todo.push("--service <bamboo|github|...>".to_string());
    }

    let cred = Credential {
        service: preset.id.into(),
        url,
        description: description.to_string(),
        auth,
        hosts,
        added_by: "paste-hook".into(),
    };
    Sealed::new(found.value.clone(), &cred).save(vault, &name)?;
    store.creds.insert(name.clone(), cred);
    if let Err(e) = store.save() {
        let _ = vault.delete(&name);
        store.creds.remove(&name);
        return Err(e);
    }

    let todo = if todo.is_empty() {
        String::new()
    } else {
        format!(
            "; finish setup: ask your agent, or run `keymule edit {name} {}`",
            todo.join(" ")
        )
    };
    Ok((name, todo))
}
