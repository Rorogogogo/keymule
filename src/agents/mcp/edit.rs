//! Tools that change a stored credential: rotate, update, convert.

use super::*;

/// Also used by `keymule rotate --dialog`.
pub async fn rotate_with_dialog(a: RotateArgs) -> anyhow::Result<String> {
    rotate(a).await
}

pub(super) async fn convert(a: ConvertArgs) -> anyhow::Result<String> {
    tokio::task::spawn_blocking(move || {
        let to = crate::ssh::ConvertTo::parse(
            a.to.trim(),
            a.host.as_deref(),
            a.port,
            a.username.as_deref(),
        )?;
        let mut store = Store::load()?;
        crate::ssh::convert(
            store::vault().as_ref(),
            &mut store,
            &a.name,
            to,
            a.description,
            &|m| crate::trust::confirm(m),
        )
    })
    .await?
}

/// The sealed credential. The keychain may show a macOS approval dialog, so
/// this runs off the async runtime.
pub(super) async fn load_sealed(name: &str) -> anyhow::Result<Sealed> {
    let name = name.to_string();
    tokio::task::spawn_blocking(move || {
        let store = Store::load()?;
        Sealed::load(store::vault().as_ref(), &name, store.get(&name)?)
    })
    .await?
}

pub(super) async fn rotate(a: RotateArgs) -> anyhow::Result<String> {
    let mut sealed = load_sealed(&a.name).await?;
    let cred = sealed.apply(Store::load()?.get(&a.name)?);
    let preset = presets::get_or_generic(&cred.service);
    let title = preset.title;
    let hosts = if cred.hosts.is_empty() {
        "(no host set yet)".to_string()
    } else {
        cred.hosts.join(", ")
    };
    let (secret, note) = if preset.kind == presets::Kind::Http {
        let secret = ask_secret(format!(
            "Paste the new {title} token for \"{}\"\n\nIt will only be sent to:\n\n        {hosts}",
            a.name
        ))
        .await?;
        let note = check(&a.name, &cred, &secret, a.verify.unwrap_or(true)).await?;
        (secret, note)
    } else {
        let (message, multiline) = other_kind_prompt(preset.kind, &a.name, sealed.ssh.as_ref());
        let kind = match sealed.ssh.as_ref().map(|t| t.auth) {
            Some(SshAuth::Key) => presets::Kind::PrivateKey,
            Some(SshAuth::Password | SshAuth::KeyFile) => presets::Kind::Password,
            None => preset.kind,
        };
        let secret = check_other_kind(kind, &ask_secret_with(message, multiline).await?)?;
        (secret, String::new())
    };
    sealed.secret = secret;
    sealed.save(store::vault().as_ref(), &a.name)?;
    let synced = tokio::task::spawn_blocking(move || {
        let notes = crate::install::resync(&sealed);
        (sealed.installs.len(), notes)
    })
    .await?;
    let copies = match synced {
        (0, _) => String::new(),
        (_, notes) if notes.is_empty() => {
            " Its copy in git's credential store was updated too.".into()
        }
        (_, notes) => format!(" {}", notes.join("; ")),
    };
    let revoke = if preset.kind == presets::Kind::Http {
        " Remind the user to revoke the old token at the service."
    } else {
        ""
    };
    Ok(format!("Rotated \"{}\"{note}.{copies}{revoke}", a.name))
}

pub(super) async fn update(a: UpdateArgs) -> anyhow::Result<String> {
    let mut sealed = load_sealed(&a.name).await?;
    // Check new hosts against the sealed rules, not the editable file.
    let mut cred = sealed.apply(Store::load()?.get(&a.name)?);
    let mut changed = Vec::new();
    if let Some(t) = cred.ssh.clone() {
        if a.url.is_some() {
            bail!(
                "\"{}\" is an SSH login: change its host, port or username instead of url",
                a.name
            );
        }
        let mut new = SshTarget::new(
            a.host.as_deref().unwrap_or(&t.host),
            Some(a.port.unwrap_or(t.port)),
            a.username.as_deref().unwrap_or(&t.username),
            t.auth,
        )?;
        new.key_file = t.key_file.clone();
        match a.key_file.as_deref().map(str::trim) {
            Some("") if sealed.secret.is_empty() => {
                bail!(
                    "\"{}\" has no password stored; add one with rotate_credential first",
                    a.name
                )
            }
            Some("") => {
                new.auth = SshAuth::Password;
                new.key_file = None;
            }
            Some(path) => new = new.with_key_file(path)?,
            None => {}
        }
        if new.host != t.host || new.port != t.port {
            let allowed = ask_confirm(format!(
                "Use \"{}\" to log in to another server?\n\n        {}\n\nNow it's only used for {}. \
                 Only allow it if you asked for it.",
                a.name,
                new.label(),
                t.label()
            ))
            .await?;
            if !allowed {
                bail!(
                    "The user did not allow moving \"{}\" to {}. Nothing changed.",
                    a.name,
                    new.label()
                );
            }
        }
        if new.auth != t.auth || new.key_file != t.key_file {
            changed.push("how it logs in");
        }
        if new.host != t.host || new.port != t.port || new.username != t.username {
            changed.push("server");
            sealed.exec_grants.clear(); // approvals named the old server
        }
        cred.ssh = Some(new);
    } else if a.host.is_some() || a.port.is_some() || a.key_file.is_some() {
        bail!(
            "host, port and key_file are for SSH logins; \"{}\" isn't one",
            a.name
        );
    }
    let ssh_user = cred.ssh.is_some();
    if let Some(url) = a.url {
        let url = url.trim_end_matches('/').to_string();
        let host = host_of(&url)?;
        if !cred.hosts.contains(&host) {
            let current = if cred.hosts.is_empty() {
                String::new()
            } else {
                format!("Now it only goes to {}. ", cred.hosts.join(", "))
            };
            let allowed = ask_confirm(format!(
                "Send \"{}\" to a new site?\n\n        {host}\n\n\
                 {current}Only allow it if you asked for it.",
                a.name
            ))
            .await?;
            if !allowed {
                bail!(
                    "The user did not allow sending \"{}\" to {host}. Nothing changed.",
                    a.name
                );
            }
        }
        cred.hosts = presets::hosts_for(presets::get_or_generic(&cred.service), &url)?;
        cred.url = url;
        changed.push("url");
    }
    if let Some(u) = a.username.filter(|_| !ssh_user) {
        cred.auth = Auth::Basic { username: u };
        changed.push("username");
    }
    if let Some(d) = a.description {
        cred.description = d;
        changed.push("description");
    }
    if changed.is_empty() {
        bail!("nothing to change: pass url, username, host, port or description");
    }
    let mut store = Store::load()?;
    store.get(&a.name)?;
    sealed.hosts = cred.hosts.clone();
    sealed.auth = cred.auth.clone();
    sealed.ssh = cred.ssh.clone();
    sealed.save(store::vault().as_ref(), &a.name)?;
    store.creds.insert(a.name.clone(), cred);
    store.save()?;
    Ok(format!("Updated \"{}\": {}.", a.name, changed.join(", ")))
}
