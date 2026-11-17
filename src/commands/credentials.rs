//! Credential commands: add, list, edit, rotate, reveal, test, delete.

use anyhow::{Result, bail};

use super::setup::home;
use crate::presets::{self, AuthKind, PRESETS};
use crate::store::{
    self, Auth, Credential, Sealed, SshAuth, SshTarget, Store, host_of, validate_name,
};
use crate::*;

pub(super) async fn add(
    service: Option<String>,
    name: Option<String>,
    url: Option<String>,
    username: Option<String>,
    header: Option<String>,
    description: Option<String>,
    no_verify: bool,
) -> Result<()> {
    let mut store = Store::load()?;
    let service = match service {
        Some(s) => s,
        None => {
            let ids: Vec<_> = PRESETS.iter().map(|p| p.id).collect();
            tty::ask_default(&format!("Service ({})", ids.join(", ")), "generic")?
        }
    };
    let Some(preset) = presets::find(&service) else {
        bail!("unknown service \"{service}\" (see `keymule presets`, or use generic)");
    };

    let name = match name {
        Some(n) => n,
        None => tty::ask_default("Name", &store.free_name(preset.id))?,
    };
    validate_name(&name)?;
    if store.creds.contains_key(&name) {
        bail!("\"{name}\" already exists; use `keymule rotate {name}` to replace its secret");
    }

    let url = match url.or(preset.default_url.map(String::from)) {
        Some(u) => u,
        None => tty::ask(&format!("Base URL (e.g. {}): ", preset.url_example))?,
    };
    let url = url.trim_end_matches('/').to_string();
    let host = host_of(&url)?;

    let username = match (username, preset.auth, &header) {
        (None, AuthKind::Basic, None) => Some(tty::ask("Username: ")?),
        (u, _, _) => u,
    };
    let auth = presets::auth_for(preset, header, username)?;

    let secret = tty::secret(&format!("{} token/password (hidden): ", preset.title))?;
    let hosts = presets::hosts_for(preset, &url)?;
    let cred = Credential {
        service: preset.id.into(),
        url,
        description: description.unwrap_or_default(),
        auth,
        hosts,
        added_by: "cli".into(),
        ssh: None,
    };

    if !no_verify {
        match http::verify(&name, &cred, &secret).await {
            Ok(Some(who)) => println!("✓ verified with {host} ({who})"),
            Ok(None) => println!(
                "· no verify endpoint for {}; saved without checking",
                preset.title
            ),
            Err(e) => {
                eprintln!("✗ {}", http::redact(&format!("{e:#}"), &secret));
                tty::confirm("Save it anyway?")?;
            }
        }
    }

    Sealed::new(secret, &cred).save(store::vault().as_ref(), &name)?;
    store.creds.insert(name.clone(), cred);
    store.save()?;
    println!("Saved \"{name}\". Your agent can now use it by name; it will never see the secret.");
    Ok(())
}

/// `keymule add ssh`: an SSH login. A password can be typed in the
/// terminal; a key, or `--dialog`, goes through keymule's window.
#[allow(clippy::too_many_arguments)]
pub(super) async fn add_ssh(
    name: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    username: Option<String>,
    key: bool,
    key_file: Option<String>,
    description: Option<String>,
    dialog: bool,
) -> Result<()> {
    let store = Store::load()?;
    let name = match name {
        Some(n) => n,
        None if dialog => store.free_name("ssh"),
        None => tty::ask_default("Name", &store.free_name("ssh"))?,
    };
    let host = match host {
        Some(h) => h,
        None if dialog => bail!("give the server with --host (a host name or IP)"),
        None => tty::ask("Host or IP: ")?,
    };
    let username = match username {
        Some(u) => u,
        None if dialog => bail!("give the account with --username"),
        None => tty::ask_default("Username", "root")?,
    };
    if dialog || key || key_file.is_some() {
        let msg = mcp::add_ssh(mcp::SshArgs {
            key_file,
            name,
            host,
            port,
            username,
            auth: Some(if key { "key" } else { "password" }.into()),
            description,
        })
        .await?;
        println!("{msg}");
        return Ok(());
    }
    validate_name(&name)?;
    if store.creds.contains_key(&name) || store.servers.contains_key(&name) {
        bail!("\"{name}\" already exists; pick another name");
    }
    let target = SshTarget::new(&host, port, &username, SshAuth::Password)?;
    let secret = tty::secret(&format!("SSH password for {} (hidden): ", target.label()))?;
    if secret.is_empty() {
        bail!("nothing was typed; nothing was saved");
    }
    let cred = Credential {
        service: "ssh".into(),
        url: String::new(),
        description: description.unwrap_or_default(),
        auth: Auth::Bearer,
        hosts: vec![],
        added_by: "cli".into(),
        ssh: Some(target.clone()),
    };
    let mut store = Store::load()?;
    Sealed::new(secret, &cred).save(store::vault().as_ref(), &name)?;
    store.creds.insert(name.clone(), cred);
    store.save()?;
    println!(
        "Saved \"{name}\" (SSH login for {}). Run commands there with: keymule ssh {name} -- <command>",
        target.label()
    );
    Ok(())
}

pub(super) fn list() -> Result<()> {
    if let Some(why) = crate::vault::key_problem() {
        eprintln!(
            "⚠ keymule can't store secrets here yet ({why}).\n{}\n",
            crate::vault::PASSPHRASE_HELP
        );
    }
    let store = Store::load()?;
    if store.creds.is_empty() && store.servers.is_empty() {
        println!("No credentials yet. Try: keymule add bamboo");
        return Ok(());
    }
    for (name, s) in &store.servers {
        let mut what = s
            .env
            .iter()
            .cloned()
            .chain(s.headers.iter().map(|h| format!("{h} header")))
            .collect::<Vec<_>>()
            .join(", ");
        if s.arg_secrets > 0 {
            what = format!("{what} (+{} in arguments)", s.arg_secrets);
        }
        println!(
            "{name:<16} {:<8} {:<40} used by {}",
            "mcp",
            what.trim(),
            s.used_by.len()
        );
        if !s.description.is_empty() {
            println!("  {}", s.description);
        }
    }
    let vault = store::vault();
    for (name, c) in &store.creds {
        // Passwords, keys and AWS pairs aren't for HTTP: say how they're used.
        let ssh_label;
        let (url, auth) = match presets::get_or_generic(&c.service).kind {
            presets::Kind::Ssh => {
                ssh_label = c
                    .ssh
                    .as_ref()
                    .map(|t| format!("{} · keymule ssh {name}", t.label()))
                    .unwrap_or_else(|| "(no server: keymule edit --host --username)".into());
                (
                    ssh_label.as_str(),
                    c.ssh
                        .as_ref()
                        .map(|t| t.auth_label().to_string())
                        .unwrap_or_default(),
                )
            }
            presets::Kind::Http if c.url.is_empty() => {
                ("(no URL: keymule edit --url)", c.auth.label())
            }
            presets::Kind::Http => (c.url.as_str(), c.auth.label()),
            presets::Kind::Password => ("for programs: keymule exec --env VAR", String::new()),
            presets::Kind::PrivateKey => ("for programs: keymule exec --file KEY", String::new()),
            presets::Kind::AwsKeys => ("for programs: keymule exec (AWS_* vars)", String::new()),
        };
        println!("{name:<16} {:<11} {:<40} {auth}", c.service, url);
        if !c.description.is_empty() {
            println!("  {}", c.description);
        }
        if let Ok(sealed) = Sealed::load(vault.as_ref(), name, c)
            && sealed.differs_from(c)
        {
            println!(
                "  ⚠ credentials.toml was edited outside keymule; still only sent to {} ({})",
                sealed.hosts.join(", "),
                sealed.auth.label()
            );
        }
    }
    Ok(())
}

/// `keymule list --json`, for the Claude Code panel and scripts. No secrets.
pub(super) fn list_json() -> Result<()> {
    let store = Store::load()?;
    let vault = store::vault();
    let creds: Vec<_> = store
        .creds
        .iter()
        .map(|(name, c)| {
            let sealed = Sealed::load(vault.as_ref(), name, c).ok();
            let used = sealed
                .as_ref()
                .map(|s| s.apply(c))
                .unwrap_or_else(|| c.clone());
            serde_json::json!({
                "name": name,
                "service": presets::get_or_generic(&c.service).title,
                "kind": match presets::get_or_generic(&c.service).kind {
                    presets::Kind::Http => "http",
                    presets::Kind::Password => "password",
                    presets::Kind::PrivateKey => "private-key",
                    presets::Kind::AwsKeys => "aws",
                    presets::Kind::Ssh => "ssh",
                },
                "ssh": used.ssh.as_ref().map(|t| serde_json::json!({
                    "host": t.host,
                    "port": t.port,
                    "username": t.username,
                    "auth": t.auth_label(),
                    "label": t.label(),
                    "key_file": t.key_file_label(),
                })),
                "url": c.url,
                "hosts": used.hosts,
                "auth": used.auth.label(),
                "description": c.description,
                "added_by": c.added_by,
                "file_edited": sealed.as_ref().is_some_and(|s| s.differs_from(c)),
                "installed": sealed.as_ref().map(|s| s.installs.iter().map(|i| i.label()).collect::<Vec<_>>()).unwrap_or_default(),
                "exec_allowed": sealed.as_ref().map(|s| s.exec_grants.iter().map(|g| format!("{} in {}", g.program, g.dir)).collect::<Vec<_>>()).unwrap_or_default(),
                "missing_secret": sealed.is_none(),
            })
        })
        .collect();
    let servers: Vec<_> = store
        .servers
        .iter()
        .map(|(name, s)| {
            serde_json::json!({
                "name": name,
                "env": s.env,
                "headers": s.headers,
                "arg_secrets": s.arg_secrets,
                "used_by": s.used_by,
                "description": s.description,
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "credentials": creds,
            "servers": servers,
            "vault_problem": crate::vault::key_problem()
                .map(|why| format!("{why}\n\n{}", crate::vault::PASSPHRASE_HELP)),
            "audit_log": store::config_dir()?.join("audit.log"),
        }))?
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn edit(
    name: &str,
    url: Option<String>,
    username: Option<String>,
    service: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    key_file: Option<String>,
    description: Option<String>,
) -> Result<()> {
    let mut store = Store::load()?;
    if !store.creds.contains_key(name)
        && let Some(meta) = store.servers.get_mut(name)
    {
        // An MCP server secret: only its description is yours to edit here.
        if url.is_some()
            || username.is_some()
            || service.is_some()
            || host.is_some()
            || port.is_some()
            || key_file.is_some()
        {
            bail!(
                "\"{name}\" is an MCP server secret: only --description can change (to make it an \
                 SSH login or a password, use keymule convert {name})"
            );
        }
        let Some(d) = description else {
            bail!("nothing to change: pass --description");
        };
        meta.description = d;
        store.save()?;
        println!("Updated \"{name}\".");
        return Ok(());
    }
    let vault = store::vault();
    let mut sealed = Sealed::load(vault.as_ref(), name, store.get(name)?)?;
    // Start from the sealed rules, not whatever the file says now.
    let mut cred = sealed.apply(store.get(name)?);
    let is_ssh = presets::get_or_generic(&cred.service).kind == presets::Kind::Ssh;
    if !is_ssh && (host.is_some() || port.is_some() || key_file.is_some()) {
        bail!("--host, --port and --key-file are for SSH logins; \"{name}\" isn't one");
    }
    if is_ssh {
        if url.is_some() || service.is_some() {
            bail!("\"{name}\" is an SSH login: change --host, --port, --username or --description");
        }
        let old = cred.ssh.clone();
        let mut new = match (&old, &host) {
            (Some(t), _) => {
                let mut n = SshTarget::new(
                    host.as_deref().unwrap_or(&t.host),
                    Some(port.unwrap_or(t.port)),
                    username.as_deref().unwrap_or(&t.username),
                    t.auth,
                )?;
                n.key_file = t.key_file.clone();
                n
            }
            (None, Some(h)) => {
                let auth = if sealed.secret.trim_start().starts_with("-----BEGIN") {
                    SshAuth::Key
                } else {
                    SshAuth::Password
                };
                SshTarget::new(
                    h,
                    port,
                    username
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("give --username too"))?,
                    auth,
                )?
            }
            (None, None) => bail!("give the server with --host and --username"),
        };
        match key_file.as_deref().map(str::trim) {
            Some("") if sealed.secret.is_empty() => {
                bail!("\"{name}\" has no password stored; add one with keymule rotate {name} first")
            }
            Some("") => {
                new.auth = SshAuth::Password;
                new.key_file = None;
            }
            Some(path) => new = new.with_key_file(path)?,
            None => {}
        }
        if old
            .as_ref()
            .is_some_and(|t| t.host != new.host || t.port != new.port)
        {
            let question = format!("Use \"{name}\" to log in to {} instead?", new.label());
            let here = std::env::current_dir().unwrap_or_default();
            if !trust::skips(vault.as_ref(), &here, &question) {
                tty::confirm(&question)?;
            }
        }
        if old.as_ref() != Some(&new) {
            sealed.exec_grants.clear(); // approvals named the old server
        }
        cred.ssh = Some(new);
        if let Some(d) = description {
            cred.description = d;
        }
        sealed.ssh = cred.ssh.clone();
        sealed.save(vault.as_ref(), name)?;
        store.creds.insert(name.to_string(), cred);
        store.save()?;
        println!("Updated \"{name}\".");
        return Ok(());
    }

    if let Some(s) = service {
        let Some(p) = presets::find(&s) else {
            bail!("unknown service \"{s}\"");
        };
        cred.service = p.id.into();
        if let AuthKind::Header(h) = p.auth {
            cred.auth = Auth::Header { name: h.into() };
        }
    }
    if let Some(u) = url {
        let u = u.trim_end_matches('/').to_string();
        let host = host_of(&u)?;
        if !cred.hosts.contains(&host) {
            // Pointing a secret at a new host is exactly what a prompt-injected
            // agent would try, so a human has to approve it on the terminal.
            let question = format!("Allow \"{name}\" to be sent to {host}?");
            let here = std::env::current_dir().unwrap_or_default();
            if !trust::skips(store::vault().as_ref(), &here, &question) {
                tty::confirm(&question)?;
            }
        }
        cred.hosts = presets::hosts_for(presets::get_or_generic(&cred.service), &u)?;
        cred.url = u;
    }
    if let Some(u) = username {
        cred.auth = Auth::Basic { username: u };
    }
    if let Some(d) = description {
        cred.description = d;
    }
    sealed.hosts = cred.hosts.clone();
    sealed.auth = cred.auth.clone();
    sealed.save(vault.as_ref(), name)?;
    store.creds.insert(name.to_string(), cred);
    store.save()?;
    println!("Updated \"{name}\".");
    Ok(())
}

pub(super) async fn rotate(name: &str, no_verify: bool, dialog: bool) -> Result<()> {
    let store = Store::load()?;
    if store.servers.contains_key(name) && !store.creds.contains_key(name) {
        return rotate_server(name, dialog);
    }
    if dialog {
        let msg = mcp::rotate_with_dialog(mcp::RotateArgs {
            name: name.into(),
            verify: Some(!no_verify),
        })
        .await?;
        println!("{msg}");
        return Ok(());
    }
    let vault = store::vault();
    let mut sealed = Sealed::load(vault.as_ref(), name, store.get(name)?)?;
    let cred = sealed.apply(store.get(name)?);
    let secret = tty::secret(&format!("New secret for {name} (hidden): "))?;
    if !no_verify && !cred.url.is_empty() {
        match http::verify(name, &cred, &secret).await {
            Ok(Some(who)) => println!("✓ verified ({who})"),
            Ok(None) => {}
            Err(e) => {
                eprintln!("✗ {}", http::redact(&format!("{e:#}"), &secret));
                tty::confirm("Save it anyway?")?;
            }
        }
    }
    sealed.secret = secret;
    sealed.save(vault.as_ref(), name)?;
    for note in install::resync(&sealed) {
        eprintln!("! {note}");
    }
    if !sealed.installs.is_empty() {
        println!("Updated its copy in git's credential store too.");
    }
    println!("Rotated \"{name}\". Revoke the old token at the service if you haven't.");
    Ok(())
}

/// Show a credential's or an MCP server's secret in a keymule window. The
/// value never goes to stdout, where an agent running this would see it.
pub(super) fn reveal(name: &str) -> Result<()> {
    let store = Store::load()?;
    let vault = store::vault();
    let (items, copy): (Vec<(String, String)>, Option<String>) =
        if let Some(c) = store.creds.get(name) {
            let sealed = Sealed::load(vault.as_ref(), name, c)?;
            let mut items = Vec::new();
            if let Some(t) = &sealed.ssh {
                items.push(("Server".to_string(), t.label()));
            }
            if let Auth::Basic { username } = &sealed.auth {
                items.push(("Username".to_string(), username.clone()));
            }
            if let Some(f) = sealed.ssh.as_ref().and_then(|t| t.key_file_label()) {
                items.push(("Key file".to_string(), f));
            }
            let what = match sealed.ssh.as_ref().map(|t| t.auth) {
                Some(SshAuth::Password | SshAuth::KeyFile) => "Password",
                Some(SshAuth::Key) => "Private key",
                None => "Secret",
            };
            if !sealed.secret.is_empty() {
                items.push((what.to_string(), sealed.secret.clone()));
            }
            let copy = (!sealed.secret.is_empty()).then(|| sealed.secret.clone());
            (items, copy)
        } else if store.servers.contains_key(name) {
            let b = servers::Binding::load(vault.as_ref(), name)?;
            let items: Vec<(String, String)> = b
                .env
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .chain(
                    b.headers
                        .iter()
                        .map(|(k, v)| (format!("{k} header"), v.clone())),
                )
                .chain(
                    b.args
                        .values()
                        .enumerate()
                        .map(|(i, v)| (format!("Argument secret #{}", i + 1), v.clone())),
                )
                .collect();
            let copy = (items.len() == 1).then(|| items[0].1.clone());
            (items, copy)
        } else {
            bail!("keymule has no credential or MCP server named \"{name}\"");
        };
    if items.is_empty() {
        bail!("\"{name}\" holds no secret");
    }
    let here = std::env::current_dir().unwrap_or_default();
    let ask = !trust::skips(vault.as_ref(), &here, &format!("reveal \"{name}\""));
    if dialog::reveal(name, &items, copy.as_deref(), ask)? {
        http::audit(name, "REVEAL", "", 0);
        println!("Showed \"{name}\" in a keymule window. The value isn't printed here.");
    } else {
        println!("Not shown.");
    }
    Ok(())
}

pub(super) async fn test(name: &str) -> Result<()> {
    let store = Store::load()?;
    let sealed = Sealed::load(store::vault().as_ref(), name, store.get(name)?)?;
    let cred = sealed.apply(store.get(name)?);
    let secret = sealed.secret;
    match http::verify(name, &cred, &secret).await {
        Ok(Some(who)) => println!("✓ {name} works ({who})"),
        Ok(None) => println!("· {name}: no verify endpoint for this service"),
        Err(e) => bail!("{}", http::redact(&format!("{e:#}"), &secret)),
    }
    Ok(())
}

/// A new value for one of an adopted server's secrets, or None to keep it.
pub(super) fn ask_new(dialog: bool, server: &str, what: &str) -> Result<Option<String>> {
    if !dialog {
        return tty::secret_or_keep(&format!(
            "New {what} for {server} (hidden, Enter keeps it): "
        ));
    }
    match dialog::secret(&format!(
        "Type the new {what} for \"{server}\"\n\nCancel keeps the current one."
    ))? {
        dialog::Answer::Text(s) if !s.trim().is_empty() => Ok(Some(s.trim().to_string())),
        dialog::Answer::Text(_) | dialog::Answer::Cancelled => Ok(None),
        dialog::Answer::TimedOut => bail!("the dialog timed out; nothing changed"),
    }
}

pub(super) fn rotate_server(name: &str, dialog: bool) -> Result<()> {
    let vault = store::vault();
    let mut binding = servers::Binding::load(vault.as_ref(), name)?;
    let mut changed = 0;
    for (var, value) in binding.env.iter_mut() {
        if let Some(new) = ask_new(dialog, name, var)? {
            *value = new;
            changed += 1;
        }
    }
    for (header, value) in binding.headers.iter_mut() {
        if let Some(new) = ask_new(dialog, name, &format!("the {header} header"))? {
            // Keep "Bearer " if the old value had it and the new one doesn't.
            *value = match value.split_once(' ') {
                Some((scheme, _))
                    if !new.contains(' ') && scheme.eq_ignore_ascii_case("bearer") =>
                {
                    format!("{scheme} {new}")
                }
                _ => new,
            };
            changed += 1;
        }
    }
    for (i, value) in binding.args.values_mut().enumerate() {
        if let Some(new) = ask_new(dialog, name, &format!("argument secret #{}", i + 1))? {
            *value = new;
            changed += 1;
        }
    }
    if changed == 0 {
        println!("Nothing changed.");
        return Ok(());
    }
    binding.save(vault.as_ref(), name)?;
    println!(
        "Rotated {changed} secret(s) for \"{name}\". Restart your agent so the server picks them up."
    );
    Ok(())
}

pub(super) fn migrate() -> Result<()> {
    let store = Store::load()?;
    let mut names: Vec<String> = store.creds.keys().cloned().collect();
    names.extend(store.servers.keys().map(|n| servers::item(n)));
    let dir = store::config_dir()?;
    let v = vault::EncVault::open_default(dir.clone());
    let moved = v.migrate(&names)?;
    println!(
        "Moved {moved} secret(s) into {}. {} secret(s) in all; only the vault's key stays in the keychain.",
        dir.join("vault.enc").display(),
        names.len()
    );
    Ok(())
}

pub(super) fn adopt(dry_run: bool, yes: bool, force: Vec<String>) -> Result<()> {
    let opts = adopt::Options {
        force,
        home: home()?,
        dir: std::env::current_dir()?,
        exe: std::env::current_exe()?.to_string_lossy().into_owned(),
    };
    let vault = store::vault();
    let mut store = Store::load()?;
    let plan = adopt::plan(&opts, &store, vault.as_ref())?;
    print!("{}", plan.render());
    if plan.targets.is_empty() || dry_run {
        if dry_run && !plan.targets.is_empty() {
            println!("\nDry run: nothing changed. Run `keymule adopt` to do it.");
        }
        return Ok(());
    }
    println!();
    if !yes {
        tty::confirm(&format!(
            "Move these secrets into the keychain and rewrite {}?",
            plan.files().join(", ")
        ))?;
    }
    // Configs may have changed while you were reading: plan again and make
    // sure it is still the plan you approved.
    let fresh = adopt::plan(&opts, &store, vault.as_ref())?;
    if fresh.fingerprint() != plan.fingerprint() {
        bail!("the configs changed while waiting for you; run `keymule adopt` again");
    }
    let done = adopt::apply(&fresh, &mut store, vault.as_ref(), &opts.home)?;
    println!(
        "✓ Moved {} secret(s) into the keychain as {}.",
        done.secrets,
        done.bindings.join(", ")
    );
    println!("✓ Rewrote {}.", fresh.files().join(", "));
    println!(
        "  Backups: {}. They still hold the old secrets: delete them once the servers work.",
        done.backups.join(", ")
    );
    println!("\nRestart Claude Code, Codex, Cursor or Claude Desktop so they pick up the change.");
    println!(
        "These secrets sat in plain text, so rotate them when you can: `keymule rotate <name>`."
    );
    Ok(())
}

/// A human has to say yes: on the terminal, or in keymule's own dialog.
pub(super) fn confirm_rm(question: &str, dialog: bool) -> Result<()> {
    if !dialog {
        return tty::confirm(question);
    }
    if dialog::confirm_delete(question)? {
        Ok(())
    } else {
        bail!("cancelled; nothing was deleted")
    }
}

pub(super) fn rm(name: &str, dialog: bool) -> Result<()> {
    let mut store = Store::load()?;
    if store.servers.contains_key(name) && !store.creds.contains_key(name) {
        confirm_rm(
            &format!(
                "Delete MCP server \"{name}\"?\n\nIts secrets go too, and configs that run `keymule exec {name}` stop working."
            ),
            dialog,
        )?;
        servers::Binding::delete(store::vault().as_ref(), name)?;
        store.servers.remove(name);
        store.save()?;
        println!("Deleted \"{name}\".");
        return Ok(());
    }
    let cred = store.get(name)?.clone();
    let vault = store::vault();
    let installs = Sealed::load(vault.as_ref(), name, &cred)
        .map(|s| s.installs.clone())
        .unwrap_or_default();
    let also = if installs.is_empty() {
        String::new()
    } else {
        let list: Vec<String> = installs.iter().map(|i| i.label()).collect();
        format!(" Git's copy ({}) goes too.", list.join(", "))
    };
    confirm_rm(
        &format!("Delete \"{name}\"?\n\nAgents using it will stop working.{also}"),
        dialog,
    )?;
    if let Ok(sealed) = Sealed::load(vault.as_ref(), name, &cred) {
        for note in install::erase_all(&sealed) {
            eprintln!("! {note}");
        }
    }
    vault.delete(name)?;
    store.creds.remove(name);
    store.save()?;
    println!("Deleted \"{name}\".");
    Ok(())
}
