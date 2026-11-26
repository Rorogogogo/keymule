//! Tools that store a new secret: tokens, passwords and keys, SSH logins,
//! website logins and MCP server secrets.

use super::*;

/// Add a password, private key or AWS key pair: no URL, no hosts; it's only
/// handed to programs through `keymule exec`.
pub(super) async fn add_other_kind(
    name: String,
    preset: &'static presets::Preset,
    description: Option<String>,
) -> anyhow::Result<String> {
    let (message, multiline) = other_kind_prompt(preset.kind, &name, None);
    let secret = check_other_kind(preset.kind, &ask_secret_with(message, multiline).await?)?;
    let cred = Credential {
        service: preset.id.into(),
        url: String::new(),
        description: description.unwrap_or_default(),
        auth: Auth::Bearer,
        hosts: vec![],
        added_by: "mcp".into(),
        ssh: None,
    };
    let mut store = Store::load()?;
    if store.creds.contains_key(&name) {
        bail!("\"{name}\" was added meanwhile; nothing changed");
    }
    let vault = store::vault();
    Sealed::new(secret, &cred).save(vault.as_ref(), &name)?;
    store.creds.insert(name.clone(), cred);
    if let Err(e) = store.save() {
        let _ = vault.delete(&name);
        return Err(e);
    }
    Ok(format!(
        "Saved \"{name}\" ({}). {}",
        preset.title,
        preset.hint.replace("<name>", &name)
    ))
}

pub async fn add_with_dialog(a: AddArgs) -> anyhow::Result<String> {
    add(a).await
}

pub(super) async fn add(a: AddArgs) -> anyhow::Result<String> {
    let store = Store::load()?;
    let preset = presets::find(&a.service).ok_or_else(|| {
        let ids: Vec<_> = presets::PRESETS.iter().map(|p| p.id).collect();
        anyhow!(
            "unknown service \"{}\"; use one of: {}",
            a.service,
            ids.join(", ")
        )
    })?;
    let name = a.name.unwrap_or_else(|| store.free_name(preset.id));
    validate_name(&name)?;
    if store.creds.contains_key(&name) {
        bail!(
            "\"{name}\" already exists. Use rotate_credential to replace its secret, or choose another name."
        );
    }
    if preset.kind == presets::Kind::Ssh {
        bail!(
            "an SSH login needs its server: call add_ssh_credential with name, host, username \
             (and port, auth=\"key\" for a private key)"
        );
    }
    if preset.kind == presets::Kind::Login {
        bail!(
            "a website login needs its site and username: call add_website_login with name, url \
             (the sign-in page) and username"
        );
    }
    if preset.kind != presets::Kind::Http {
        return add_other_kind(name, preset, a.description).await;
    }
    let url = a
        .url
        .or(preset.default_url.map(String::from))
        .ok_or_else(|| {
            anyhow!(
                "{} needs a base URL (e.g. {}). Ask the user for it; URLs are not secret.",
                preset.title,
                preset.url_example
            )
        })?
        .trim_end_matches('/')
        .to_string();
    let hosts = presets::hosts_for(preset, &url)?;
    let auth = presets::auth_for(preset, a.header, a.username)
        .map_err(|e| anyhow!("{e}. Ask the user for their username and pass it as `username`."))?;

    let secret = ask_secret(format!(
        "Paste your {} token for \"{name}\"\n\nIt will only be sent to:\n\n        {}",
        preset.title,
        hosts.join(", ")
    ))
    .await?;
    let cred = Credential {
        service: preset.id.into(),
        url,
        description: a.description.unwrap_or_default(),
        auth,
        hosts,
        added_by: "mcp".into(),
        ssh: None,
    };
    let note = check(&name, &cred, &secret, a.verify.unwrap_or(true)).await?;

    // Reload: the dialog may have been open for a while.
    let mut store = Store::load()?;
    if store.creds.contains_key(&name) {
        bail!("\"{name}\" was created by something else meanwhile. Nothing was saved.");
    }
    Sealed::new(secret, &cred).save(store::vault().as_ref(), &name)?;
    store.creds.insert(name.clone(), cred);
    store.save()?;
    Ok(format!(
        "Saved \"{name}\" ({}){note}. Use it with http_request(credential=\"{name}\", ...).",
        preset.title
    ))
}

/// Store an SSH login. Also used by `keymule add ssh --dialog`.
pub async fn add_ssh(a: SshArgs) -> anyhow::Result<String> {
    validate_name(&a.name)?;
    let auth = match a.auth.as_deref().map(str::trim) {
        None | Some("") | Some("password") => SshAuth::Password,
        Some("key" | "private-key") => SshAuth::Key,
        Some(other) => bail!("auth is \"password\" or \"key\", not \"{other}\""),
    };
    let mut target = SshTarget::new(&a.host, a.port, &a.username, auth)?;
    if let Some(path) = a.key_file.as_deref().filter(|p| !p.trim().is_empty()) {
        target = target.with_key_file(path)?;
    }
    let taken = |s: &Store| s.creds.contains_key(&a.name) || s.servers.contains_key(&a.name);
    if taken(&Store::load()?) {
        bail!("\"{}\" already exists; pick another name", a.name);
    }
    // A key file needs no secret typed; a password can be added later
    // with rotate_credential.
    let secret = if target.auth == SshAuth::KeyFile {
        String::new()
    } else {
        let (message, multiline) = other_kind_prompt(presets::Kind::Ssh, &a.name, Some(&target));
        let typed = ask_secret_with(message, multiline).await?;
        check_other_kind(
            if target.auth == SshAuth::Key {
                presets::Kind::PrivateKey
            } else {
                presets::Kind::Password
            },
            &typed,
        )?
    };
    let cred = Credential {
        service: "ssh".into(),
        url: String::new(),
        description: a.description.unwrap_or_default(),
        auth: Auth::Bearer,
        hosts: vec![],
        added_by: "mcp".into(),
        ssh: Some(target.clone()),
    };
    let mut store = Store::load()?;
    if taken(&store) {
        bail!("\"{}\" was added meanwhile; nothing was saved", a.name);
    }
    let vault = store::vault();
    Sealed::new(secret, &cred).save(vault.as_ref(), &a.name)?;
    store.creds.insert(a.name.clone(), cred);
    if let Err(e) = store.save() {
        let _ = vault.delete(&a.name);
        return Err(e);
    }
    Ok(format!(
        "Saved \"{name}\" (SSH login for {}, {}). Run commands there with: keymule ssh {name} -- \
         <command> (e.g. keymule ssh {name} -- uptime). The user approves it once per folder; you \
         get the output, never the {}.",
        target.label(),
        target.auth_label(),
        target.auth_label(),
        name = a.name,
    ))
}

/// Store a website login. The password is typed in a dialog.
pub(super) async fn add_login(a: LoginArgs) -> anyhow::Result<String> {
    validate_name(&a.name)?;
    let cred = crate::browser::login_credential(
        a.url.trim(),
        &a.username,
        a.description.unwrap_or_default(),
    )?;
    let taken = |s: &Store| s.creds.contains_key(&a.name) || s.servers.contains_key(&a.name);
    if taken(&Store::load()?) {
        bail!("\"{}\" already exists; pick another name", a.name);
    }
    let Auth::Basic { username } = cred.auth.clone() else {
        unreachable!("a login signs in with a username")
    };
    let password = ask_secret(format!(
        "Type the password for {username} on {}\n\nkeymule only fills it into {} in the keymule \
         browser, and asks you each time. Your agent never sees it.",
        cred.hosts[0], cred.url
    ))
    .await?;
    let password = check_other_kind(presets::Kind::Password, &password)?;
    let mut store = Store::load()?;
    if taken(&store) {
        bail!("\"{}\" was added meanwhile; nothing was saved", a.name);
    }
    let vault = store::vault();
    Sealed::new(password, &cred).save(vault.as_ref(), &a.name)?;
    let origin = cred.url.clone();
    store.creds.insert(a.name.clone(), cred);
    if let Err(e) = store.save() {
        let _ = vault.delete(&a.name);
        return Err(e);
    }
    Ok(format!(
        "Saved \"{name}\" (website login for {username} on {origin}). To sign in, open the \
         site's sign-in page in the keymule browser, then call sign_in(credential=\"{name}\"). \
         The user approves each sign-in; you never see the password.",
        name = a.name
    ))
}

pub(super) async fn add_mcp_secret(a: McpSecretArgs) -> anyhow::Result<String> {
    validate_name(&a.name)?;
    enum Slot {
        Env(String),
        Header(String),
    }
    let slot = match (
        a.env.as_deref().map(str::trim),
        a.header.as_deref().map(str::trim),
    ) {
        (Some(env), None) => {
            let ok = !env.is_empty()
                && env.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && !env.starts_with(|c: char| c.is_ascii_digit());
            if !ok {
                bail!("env must be a variable name like DATABASE_URI");
            }
            Slot::Env(env.to_string())
        }
        (None, Some(h)) => {
            if reqwest::header::HeaderName::from_bytes(h.as_bytes()).is_err()
                || crate::remote::RESERVED.contains(&h.to_ascii_lowercase().as_str())
            {
                bail!("header must be a header name like x-api-key or Authorization");
            }
            Slot::Header(h.to_string())
        }
        _ => bail!("give either env (local server) or header (remote server)"),
    };
    let needs = match &slot {
        Slot::Env(v) => v.clone(),
        Slot::Header(h) => format!("the {h} header"),
    };
    let store = Store::load()?;
    if store.creds.contains_key(&a.name) || store.servers.contains_key(&a.name) {
        bail!("\"{}\" already exists; pick another name", a.name);
    }
    let what = a
        .description
        .as_deref()
        .map(|d| format!(" ({d})"))
        .unwrap_or_default();
    let value = ask_secret(format!(
        "Enter {}{what} for a new MCP server\n\n\
         Only servers you approve will get it.",
        needs
    ))
    .await?;

    let mut binding = crate::servers::Binding::default();
    let mut meta = crate::store::ServerMeta {
        description: a.description.clone().unwrap_or_default(),
        ..Default::default()
    };
    match &slot {
        Slot::Env(env) => {
            binding.env.insert(env.clone(), value);
            meta.env = vec![env.clone()];
        }
        Slot::Header(h) => {
            // People type the token; Authorization wants "Bearer <token>".
            let value = if h.eq_ignore_ascii_case("authorization") && !value.contains(' ') {
                format!("Bearer {value}")
            } else {
                value
            };
            binding.headers.insert(h.clone(), value);
            meta.headers = vec![h.clone()];
        }
    }
    let mut store = Store::load()?;
    if store.creds.contains_key(&a.name) || store.servers.contains_key(&a.name) {
        bail!(
            "\"{}\" was created by something else meanwhile. Nothing was saved.",
            a.name
        );
    }
    binding.save(store::vault().as_ref(), &a.name)?;
    store.servers.insert(a.name.clone(), meta);
    store.save()?;
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "keymule".into());
    if let Slot::Header(h) = &slot {
        return Ok(format!(
            "Saved \"{name}\". Configure the MCP server as a stdio server with command \"{exe}\" and args [\"remote\", \"{name}\", \"<the server's https URL>\"]. keymule connects to the URL and adds the {h} header. Do not put the value or that header in the config. The first time it connects, the user approves the URL in a dialog. Example: claude mcp add <server> -- \"{exe}\" remote {name} <url>",
            name = a.name,
        ));
    }
    let Slot::Env(env) = &slot else {
        unreachable!()
    };
    Ok(format!(
        "Saved \"{name}\". Configure the MCP server with command \"{exe}\" and args [\"exec\", \"{name}\", \"--\", <the server's command and args>]. keymule sets {env} for it. Do not put the value in the config. The first time the server starts, the user approves the command in a dialog. Example: claude mcp add <server> -- \"{exe}\" exec {name} -- <command> <args>",
        name = a.name,
    ))
}
