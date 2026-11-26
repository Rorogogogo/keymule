//! stdio MCP server. Agents see credential names and metadata only; secret
//! values are read from the keychain inside `http_request` and never
//! returned.

use std::collections::BTreeMap;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::schemars::{self, JsonSchema};
use rmcp::{ErrorData as McpError, ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use serde::Deserialize;

use anyhow::{anyhow, bail};

use crate::dialog::{self, Answer};
use crate::http;
use crate::presets;
use crate::store::{
    self, Auth, Credential, Sealed, SshAuth, SshTarget, Store, host_of, validate_name,
};

const INSTRUCTIONS: &str = "keymule holds the user's API tokens and passwords. \
Whenever the user asks about a service that needs authentication (Bamboo, GitHub Actions, GitLab, \
Jenkins, internal APIs...), call list_credentials first and use http_request with the matching \
credential name. For a server the user logs in to with SSH (host or IP, username, password or \
key), call add_ssh_credential, then run commands there with `keymule ssh <name> -- <command>`; \
add_mcp_secret is only for secrets an MCP server reads, never for SSH logins or passwords for \
other tools (those are add_credential with service password). To sign in to a website in a \
browser, store the login with add_website_login and call sign_in once its sign-in page is open \
in the keymule browser (`keymule browser -- <browser MCP server>`). Never ask the user to paste a token, password or API key into the chat. If no \
matching credential exists, call add_credential: keymule shows a secure dialog on the user's \
screen where they type the secret, and you never see it. Ask the user for the service's base URL \
first if needed (URLs are not secret). If a token stops working (HTTP 401), offer \
rotate_credential. When you set up an MCP server that needs a password, API key or database URL, \
never write the secret into the MCP config or ask for it in chat: call add_mcp_secret (the user \
types it in a dialog), then configure the server to run `keymule exec <name> [--env VAR] -- \
<server command>`. For a remote (HTTP) MCP server that needs an API key header, call \
add_mcp_secret with `header` instead of `env`, and configure it as a stdio server running \
`keymule remote <name> <url>`. The user approves that command or URL once, in a dialog, when it \
first starts. When a command-line tool needs a stored credential (gh, glab...), run it as \
`keymule exec <name> --env GH_TOKEN -- gh ...`: the user approves each program and folder once, \
the secret is hidden from its output, and shells and interpreters are refused. For git over \
HTTPS, call install_credential (name, host, username, dir = the repo folder; one dialog also \
approves the service's CLI there) and point the repo's remote at \
https://<username>@<host>/<owner>/<repo>.git. To put a stored secret into a repo's GitHub \
Actions secrets (for CI), run `keymule gh-secret --repo <owner>/<repo> SECRET_NAME=<name> \
[SECRET2=<name2> ...]`: one dialog covers all of them, keymule encrypts each with the repo's \
key, and it uploads with the account the gh CLI is already signed in as (no GitHub token in \
keymule needed) or a stored GitHub credential, whichever has admin there; `--dry-run` shows \
which account first. Never move a secret through the clipboard, a file, a helper program or \
the chat. Never read a token back (git credential fill, \
gh auth token, security find-*-password -w). If the user asks for keymule to stop asking in this project, run `keymule allow` \
(one dialog; deleting still asks; `keymule allow --off` undoes it). Don't turn it on unless they ask.";

#[derive(Debug, Deserialize, JsonSchema)]
pub struct InstallArgs {
    /// Credential name from list_credentials.
    pub name: String,
    /// Git host, e.g. "github.com" (not the API host).
    pub host: String,
    /// The account the credential belongs to, as it will appear in the remote URL
    /// (https://<username>@<host>/...).
    pub username: String,
    /// The repo folder (absolute path). The service's CLI (gh for GitHub) is then
    /// approved there in the same dialog, so `keymule exec <name> -- gh ...` won't ask again.
    #[serde(default)]
    pub dir: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddArgs {
    /// Service preset: bamboo, github, gitlab, jenkins, stripe, openai or generic for an
    /// HTTP API token; password, private-key or aws for secrets that are only handed to
    /// programs through `keymule exec` (no URL needed). For an SSH login use
    /// add_ssh_credential instead.
    pub service: String,
    /// Base URL of the API, e.g. "https://bamboo.yourcompany.com". Not needed for github.com or gitlab.com.
    #[serde(default)]
    pub url: Option<String>,
    /// Name to store it under. Defaults to the service name.
    #[serde(default)]
    pub name: Option<String>,
    /// Username, for basic auth (e.g. Jenkins).
    #[serde(default)]
    pub username: Option<String>,
    /// Send the secret in this header instead of the Authorization header.
    #[serde(default)]
    pub header: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Check the token against the service before saving (default true).
    #[serde(default)]
    pub verify: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SshArgs {
    /// Name to store it under, e.g. "vultr-ssh". Letters, digits, '-' and '_'.
    pub name: String,
    /// The server's host name or IP address, e.g. "203.0.113.7".
    pub host: String,
    /// SSH port (default 22).
    #[serde(default)]
    pub port: Option<u16>,
    /// The account to log in as, e.g. "root".
    pub username: String,
    /// "password" (default) or "key" (the user pastes a private key).
    #[serde(default)]
    pub auth: Option<String>,
    /// A private key file already on this machine, e.g. "~/.ssh/id_ed25519": ssh uses it
    /// where it is (nothing is copied) and no dialog is needed.
    #[serde(default)]
    pub key_file: Option<String>,
    /// What it's for, in a line, e.g. "Vultr VPS in Sydney (production)".
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LoginArgs {
    /// Name to store it under, e.g. "github-login". Letters, digits, '-' and '_'.
    pub name: String,
    /// The site's sign-in page (or just the site), e.g. "https://github.com/login". The
    /// password is only ever filled into pages on exactly this origin. URLs are not secret.
    pub url: String,
    /// The username or email the user signs in with. Ask the user for it.
    pub username: String,
    /// What it's for, in a line, e.g. "GitHub, my personal account".
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SignInArgs {
    /// The website login's name, from list_credentials.
    pub credential: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ConvertArgs {
    /// The MCP server secret's name, from list_credentials (mcp_server_secrets).
    pub name: String,
    /// "ssh" for an SSH login (give host and username), or "password" for a password
    /// handed to programs with keymule exec.
    pub to: String,
    /// For ssh: the server's host name or IP address.
    #[serde(default)]
    pub host: Option<String>,
    /// For ssh: the port (default 22).
    #[serde(default)]
    pub port: Option<u16>,
    /// For ssh: the account to log in as.
    #[serde(default)]
    pub username: Option<String>,
    /// What it's for, in a line (keeps the old one if left out).
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RotateArgs {
    /// Credential name from list_credentials.
    pub name: String,
    /// Check the new token before saving (default true).
    #[serde(default)]
    pub verify: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScanArgs {
    /// Also list unsure matches (secrets without a known format).
    #[serde(default)]
    pub all: Option<bool>,
    /// Project directory to check for .env and .mcp.json files (default: the server's working directory).
    #[serde(default)]
    pub dir: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct McpSecretArgs {
    /// Name to store it under, e.g. "pg-local". Letters, digits, '-' and '_'.
    pub name: String,
    /// The env var a local MCP server reads it from, e.g. "DATABASE_URI" or "NOTION_TOKEN".
    #[serde(default)]
    pub env: Option<String>,
    /// For a remote (HTTP) MCP server instead: the header that carries it,
    /// e.g. "x-api-key" or "Authorization".
    #[serde(default)]
    pub header: Option<String>,
    /// What it is, shown in the dialog, e.g. "Postgres URL for the local dev database".
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SaveHeldArgs {
    /// The held secret's id, from keymule's note or list_credentials.
    pub id: String,
    /// The name the user picked (lowercase with hyphens, service first, e.g. github-homebrew-tap).
    pub name: String,
    /// What it's for, in a line, so a later agent picks the right one.
    pub description: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct HeldIdArgs {
    /// The held secret's id.
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateArgs {
    /// Credential name from list_credentials.
    pub name: String,
    /// New base URL. A new host needs the user's approval in a dialog.
    #[serde(default)]
    pub url: Option<String>,
    /// Username for basic auth, or the account an SSH login uses.
    #[serde(default)]
    pub username: Option<String>,
    /// For an SSH login: a new host or IP. Needs the user's approval in a dialog.
    #[serde(default)]
    pub host: Option<String>,
    /// For an SSH login: a new port.
    #[serde(default)]
    pub port: Option<u16>,
    /// For an SSH login: log in with this private key file on this machine (e.g.
    /// "~/.ssh/id_ed25519"), used where it is. The stored password, if any, is kept.
    /// "" switches back to the password.
    #[serde(default)]
    pub key_file: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

fn done(result: anyhow::Result<String>) -> CallToolResult {
    match result {
        Ok(s) => text(s),
        Err(e) => tool_error(format!("{e:#}")),
    }
}

async fn ask_secret(message: String) -> anyhow::Result<String> {
    ask_secret_with(message, false).await
}

async fn ask_secret_with(message: String, multiline: bool) -> anyhow::Result<String> {
    match tokio::task::spawn_blocking(move || dialog::secret_with(&message, multiline)).await?? {
        Answer::Text(s) => Ok(s),
        Answer::Cancelled => bail!("The user cancelled the dialog. Nothing was saved."),
        Answer::TimedOut => bail!("The dialog timed out after 3 minutes. Nothing was saved."),
    }
}

async fn ask_confirm(message: String) -> anyhow::Result<bool> {
    tokio::task::spawn_blocking(move || crate::trust::confirm(&message)).await?
}

/// Verify before saving. Returns a note for the result message.
async fn check(
    name: &str,
    cred: &Credential,
    secret: &str,
    verify: bool,
) -> anyhow::Result<String> {
    if !verify || cred.url.is_empty() {
        return Ok(" (not verified)".into());
    }
    match http::verify(name, cred, secret).await {
        Ok(Some(who)) => Ok(format!(", verified as {who}")),
        Ok(None) => Ok(" (this service has no verify endpoint)".into()),
        Err(e) => bail!(
            "The token did not work, so it was NOT saved: {}. Ask the user to check the token and \
             URL and try again (verify=false skips the check, e.g. when the service is only \
             reachable on VPN).",
            http::redact(&format!("{e:#}"), secret)
        ),
    }
}

/// Also used by `keymule add --dialog`.
/// What to ask for, and whether the field takes several lines, for a
/// secret that isn't an HTTP token.
fn other_kind_prompt(kind: presets::Kind, name: &str, ssh: Option<&SshTarget>) -> (String, bool) {
    if let Some(t) = ssh {
        return match t.auth {
            SshAuth::Password => (
                format!(
                    "Type the SSH password for {}\n\n\"{name}\": keymule only uses it to log in \
                     to that server. Your agent never sees it.",
                    t.label()
                ),
                false,
            ),
            SshAuth::Key => (
                format!(
                    "Paste the private key for {}\n\n\"{name}\": the whole block, from -----BEGIN \
                     to -----END. keymule only uses it to log in to that server.",
                    t.label()
                ),
                true,
            ),
            SshAuth::KeyFile => (
                format!(
                    "Type the password for {}\n\n\"{name}\" logs in with {}; keymule keeps \
                     the password with it (e.g. for the provider's console). Your agent never sees it.",
                    t.label(),
                    t.key_file_label().unwrap_or_default()
                ),
                false,
            ),
        };
    }
    match kind {
        presets::Kind::PrivateKey => (
            format!(
                "Paste the private key for \"{name}\"\n\nThe whole block, from -----BEGIN to -----END. \
                 It's only ever handed to programs you approve, as a file only you can read."
            ),
            true,
        ),
        presets::Kind::AwsKeys => (
            format!(
                "Paste the AWS keys for \"{name}\"\n\nThe access key ID on the first line, the \
                 secret access key on the second."
            ),
            true,
        ),
        presets::Kind::Login => (
            format!(
                "Type the new password for \"{name}\"\n\nkeymule only fills it into that site's \
                 sign-in page, and asks you each time."
            ),
            false,
        ),
        _ => (
            format!(
                "Type the password for \"{name}\"\n\nIt's only ever handed to programs you approve."
            ),
            false,
        ),
    }
}

/// Check a pasted private key or AWS key pair looks right; returns it tidied.
pub fn check_other_kind(kind: presets::Kind, secret: &str) -> anyhow::Result<String> {
    let secret = secret.trim().replace("\r\n", "\n");
    if secret.is_empty() {
        bail!("nothing was typed; nothing was saved");
    }
    match kind {
        presets::Kind::PrivateKey => {
            if !(secret.starts_with("-----BEGIN") && secret.contains("PRIVATE KEY-----")) {
                bail!(
                    "that doesn't look like a private key (it should start with -----BEGIN … PRIVATE KEY-----)"
                );
            }
            Ok(secret)
        }
        presets::Kind::AwsKeys => {
            let lines: Vec<&str> = secret
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect();
            match lines.as_slice() {
                [id, key]
                    if (id.starts_with("AKIA") || id.starts_with("ASIA")) && key.len() >= 30 =>
                {
                    Ok(format!("{id}\n{key}"))
                }
                _ => bail!(
                    "paste the access key ID (AKIA…) on the first line and the secret access key on the second"
                ),
            }
        }
        _ => Ok(secret),
    }
}

/// Add a password, private key or AWS key pair: no URL, no hosts; it's only
/// handed to programs through `keymule exec`.
async fn add_other_kind(
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

/// Also used by `keymule rotate --dialog`.
pub async fn rotate_with_dialog(a: RotateArgs) -> anyhow::Result<String> {
    rotate(a).await
}

async fn add(a: AddArgs) -> anyhow::Result<String> {
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
async fn add_login(a: LoginArgs) -> anyhow::Result<String> {
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

async fn sign_in(a: SignInArgs) -> anyhow::Result<String> {
    let name = a.credential.clone();
    let login = tokio::task::spawn_blocking(move || {
        crate::browser::load_login(store::vault().as_ref(), &name)
    })
    .await??;
    // Not the allow-all shortcut: every sign-in asks.
    let ask = |q: &str| {
        let q = q.to_string();
        std::thread::spawn(move || dialog::confirm(&q))
            .join()
            .map_err(|_| anyhow!("the dialog failed"))?
    };
    crate::browser::sign_in(&a.credential, login, ask).await
}

async fn convert(a: ConvertArgs) -> anyhow::Result<String> {
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
async fn load_sealed(name: &str) -> anyhow::Result<Sealed> {
    let name = name.to_string();
    tokio::task::spawn_blocking(move || {
        let store = Store::load()?;
        Sealed::load(store::vault().as_ref(), &name, store.get(&name)?)
    })
    .await?
}

async fn rotate(a: RotateArgs) -> anyhow::Result<String> {
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

async fn add_mcp_secret(a: McpSecretArgs) -> anyhow::Result<String> {
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

async fn update(a: UpdateArgs) -> anyhow::Result<String> {
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

const DEFAULT_MAX_BYTES: usize = 60_000;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct HttpRequestArgs {
    /// Credential name from list_credentials, e.g. "bamboo".
    pub credential: String,
    /// GET, POST, PUT, PATCH or DELETE. Defaults to GET.
    #[serde(default)]
    pub method: Option<String>,
    /// Path relative to the credential's base URL (e.g. "/rest/api/latest/plan"), or a full URL on one of its allowed hosts.
    pub path: String,
    /// Query string parameters.
    #[serde(default)]
    pub query: Option<BTreeMap<String, String>>,
    /// Extra request headers. Authentication headers are added automatically and cannot be set here.
    #[serde(default)]
    pub headers: Option<BTreeMap<String, String>>,
    /// Request body: a JSON value, or a string sent as-is.
    #[serde(default)]
    pub body: Option<serde_json::Value>,
    /// Maximum response bytes to return (default 60000).
    #[serde(default)]
    pub max_bytes: Option<usize>,
    /// Return the end of a long response instead of the start. Use for build logs.
    #[serde(default)]
    pub keep_tail: Option<bool>,
}

#[derive(Clone)]
pub struct Server {
    tool_router: ToolRouter<Self>,
}

fn text(s: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(s.into())])
}

fn tool_error(s: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(s.into())])
}

#[tool_router]
impl Server {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "List the credentials the user has stored: name, service, base URL, allowed hosts, and an API cheat-sheet for the service. Secret values are never included. Call this before http_request."
    )]
    async fn list_credentials(&self) -> Result<CallToolResult, McpError> {
        let store = match Store::load() {
            Ok(s) => s,
            Err(e) => return Ok(tool_error(format!("{e:#}"))),
        };
        let names: Vec<String> = store.creds.keys().cloned().collect();
        let sealed: BTreeMap<String, Sealed> = tokio::task::spawn_blocking(move || {
            let store = Store::load().ok();
            let vault = store::vault();
            names
                .into_iter()
                .filter_map(|n| {
                    let c = store.as_ref()?.creds.get(&n)?;
                    Some((n.clone(), Sealed::load(vault.as_ref(), &n, c).ok()?))
                })
                .collect()
        })
        .await
        .unwrap_or_default();
        let none_held = tokio::task::spawn_blocking(|| {
            crate::hook::list_held(store::vault().as_ref()).is_empty()
        })
        .await
        .unwrap_or(true);
        if let Some(why) = tokio::task::spawn_blocking(crate::vault::key_problem)
            .await
            .ok()
            .flatten()
        {
            return Ok(tool_error(format!(
                "keymule can't store or use secrets on this machine yet ({why}). Tell the user \
                 exactly this, then stop:\n\n{}",
                crate::vault::PASSPHRASE_HELP
            )));
        }
        if store.creds.is_empty() && store.servers.is_empty() && none_held {
            return Ok(text(
                "No credentials stored yet. Use add_credential (services: bamboo, github, gitlab, \
                 jenkins, generic, password), add_ssh_credential for an SSH login or \
                 add_website_login for a website sign-in; the user types the secret into a \
                 secure dialog.",
            ));
        }
        let list: Vec<_> = store
            .creds
            .iter()
            .map(|(name, c)| {
                let preset = presets::get_or_generic(&c.service);
                let target = sealed.get(name).and_then(|s| s.ssh.clone()).or(c.ssh.clone());
                if preset.kind == presets::Kind::Login {
                    let s = sealed.get(name).map(|s| s.apply(c)).unwrap_or_else(|| c.clone());
                    let username = match &s.auth {
                        Auth::Basic { username } => username.clone(),
                        _ => String::new(),
                    };
                    return serde_json::json!({
                        "name": name,
                        "kind": "Website login (not for http_request)",
                        "site": s.hosts.first().map(|h| crate::browser::origin_for(h, &c.url)),
                        "username": username,
                        "description": c.description,
                        "how_to_use": [format!(
                            "open the site's sign-in page in the keymule browser (your browser tool run through `keymule browser -- ...`), then sign_in(credential=\"{name}\"); the user approves each sign-in"
                        )],
                        "needs_setup": serde_json::Value::Null,
                    });
                }
                if preset.kind == presets::Kind::Ssh {
                    return serde_json::json!({
                        "name": name,
                        "kind": "SSH login (not for http_request)",
                        "server": target.as_ref().map(|t| t.label()),
                        "auth": target.as_ref().map(|t| t.auth_label()),
                        "description": c.description,
                        "how_to_use": [format!(
                            "keymule ssh {name} -- <command>  (runs it on the server; the user approves once per folder)"
                        )],
                        "needs_setup": if target.is_none() {
                            serde_json::json!([format!("ask the user for the server, then update_credential(name=\"{name}\", host=..., username=...)")])
                        } else { serde_json::Value::Null },
                    });
                }
                let mut setup = Vec::new();
                if c.url.is_empty() {
                    setup.push(format!(
                        "ask the user for the base URL, then update_credential(name=\"{name}\", url=...)"
                    ));
                }
                if matches!(&c.auth, Auth::Basic { username } if username.is_empty()) {
                    setup.push(format!(
                        "ask the user for their username, then update_credential(name=\"{name}\", username=...)"
                    ));
                }
                let mut how = Vec::new();
                if preset.kind == presets::Kind::Http {
                    how.push(format!("http_request(credential=\"{name}\", ...)"));
                    let (var, cli) = preset.cli_env.unwrap_or(("VAR", "<command>"));
                    how.push(format!("CLI tools: keymule exec {name} --env {var} -- {cli} ..."));
                } else {
                    how.push(preset.hint.replace("<name>", name));
                }
                if c.service != "github" && preset.kind != presets::Kind::Login {
                    how.push(format!(
                        "GitHub Actions secret: keymule gh-secret --repo <owner>/<repo> <SECRET_NAME>={name} (several in one go: add more SECRET=name)"
                    ));
                }
                if c.service == "github" || c.service == "gitlab" {
                    how.push(format!(
                        "git over HTTPS: install_credential(name=\"{name}\", host=..., username=<account>)"
                    ));
                }
                let installed: Vec<String> = sealed
                    .get(name)
                    .map(|s| s.installs.iter().map(|i| i.label()).collect())
                    .unwrap_or_default();
                serde_json::json!({
                    "name": name,
                    "service": preset.title,
                    "base_url": c.url,
                    "allowed_hosts": c.hosts,
                    "how_to_use": how,
                    "installed": installed,
                    "description": c.description,
                    "auth": c.auth.label(),
                    "api_hint": preset.hint,
                    "needs_setup": if setup.is_empty() { serde_json::Value::Null } else { setup.into() },
                })
            })
            .collect();
        let servers: Vec<_> = store
            .servers
            .iter()
            .map(|(name, s)| {
                serde_json::json!({
                    "name": name,
                    "kind": "MCP server secret (not for http_request)",
                    "description": s.description,
                    "env": s.env,
                    "headers": s.headers,
                    "used_by": s.used_by,
                    "how_to_use": if s.headers.is_empty() {
                        format!("in an MCP server config: keymule exec {name} [--env VAR] -- <server command>")
                    } else {
                        format!("in an MCP server config, as a stdio server: keymule remote {name} <url>")
                    },
                })
            })
            .collect();
        let held = tokio::task::spawn_blocking(|| crate::hook::list_held(store::vault().as_ref()))
            .await
            .unwrap_or_default();
        let mut out = serde_json::json!({ "credentials": list, "mcp_server_secrets": servers });
        if !held.is_empty() {
            // Secrets from the user's messages that still need a name: ask
            // the user, then save_held (or discard_held).
            out["held_waiting_for_a_name"] = serde_json::Value::Array(held);
        }
        Ok(text(serde_json::to_string_pretty(&out).unwrap_or_default()))
    }

    #[tool(
        description = "Call an HTTP API using one of the user's stored credentials. keymule attaches the secret itself, only for the credential's allowed hosts, and redacts it from the response. You never see the secret."
    )]
    async fn http_request(
        &self,
        Parameters(args): Parameters<HttpRequestArgs>,
    ) -> Result<CallToolResult, McpError> {
        let store = match Store::load() {
            Ok(s) => s,
            Err(e) => return Ok(tool_error(format!("{e:#}"))),
        };
        let cred = match store.get(&args.credential) {
            Ok(c) => c.clone(),
            Err(e) => return Ok(tool_error(format!("{e:#}"))),
        };
        let sealed = match load_sealed(&args.credential).await {
            Ok(s) => s,
            Err(e) => return Ok(tool_error(format!("{e:#}"))),
        };
        // Hosts and auth come from the keychain item, so an edited
        // credentials.toml can't send the secret anywhere new.
        let cred = sealed.apply(&cred);
        let secret = sealed.secret;
        let req = http::Request {
            method: args.method.unwrap_or_else(|| "GET".into()),
            path: args.path,
            query: args.query.unwrap_or_default(),
            headers: args.headers.unwrap_or_default(),
            body: args.body,
            max_bytes: args.max_bytes.unwrap_or(DEFAULT_MAX_BYTES),
            keep_tail: args.keep_tail.unwrap_or(false),
        };
        match http::send(&args.credential, &cred, &secret, &req).await {
            Ok(resp) => Ok(text(resp.render())),
            Err(e) => Ok(tool_error(http::redact(&format!("{e:#}"), &secret))),
        }
    }

    #[tool(
        description = "Store a new credential. keymule opens a secure dialog on the user's screen where they type it; you never see it. An HTTP API token is verified against the service and locked to the URL's host (ask the user for the base URL first if the service needs one). A password, private key (service private-key: the dialog takes several lines) or AWS key pair (service aws) needs no URL; it's only handed to programs with keymule exec. For logging in to a server over SSH, use add_ssh_credential instead; for signing in to a website in a browser, add_website_login."
    )]
    async fn add_credential(
        &self,
        Parameters(args): Parameters<AddArgs>,
    ) -> Result<CallToolResult, McpError> {
        Ok(done(add(args).await))
    }

    #[tool(
        description = "Store a username and password for one website, so a browser agent can sign in without seeing the password. Give the sign-in page's URL and the username (ask the user for both; neither is secret). keymule opens a secure dialog where the user types the password. It's only ever filled into pages on exactly that site's origin, with sign_in, after the user approves. Use this for website sign-ins, not add_credential."
    )]
    async fn add_website_login(
        &self,
        Parameters(args): Parameters<LoginArgs>,
    ) -> Result<CallToolResult, McpError> {
        Ok(done(add_login(args).await))
    }

    #[tool(
        description = "Sign in to a website with a stored website login, in the keymule browser (the Chrome your browser tool uses when it runs through `keymule browser -- ...`). Open the site's sign-in page with your browser tool first. keymule asks the user, checks the tab is on exactly the login's site, types the username and password into the page and submits; two-step forms (username, then password) are handled. You never see the password, and it's hidden from the browser tool's replies. Then take a snapshot to carry on."
    )]
    async fn sign_in(
        &self,
        Parameters(args): Parameters<SignInArgs>,
    ) -> Result<CallToolResult, McpError> {
        Ok(done(sign_in(args).await))
    }

    #[tool(
        description = "Store an SSH login for one server: host or IP, port, username, and a password (or a private key with auth=\"key\", or key_file: the path of a private key already on this machine, used where it is with no dialog). keymule opens a secure dialog where the user types the password; you never see it. It's listed under Credentials and only used to log in to that server: run commands there with `keymule ssh <name> -- <command>` (the user approves once per folder). Use this, not add_mcp_secret, for any SSH server (VPS, Vultr, DigitalOcean, a Raspberry Pi...)."
    )]
    async fn add_ssh_credential(
        &self,
        Parameters(args): Parameters<SshArgs>,
    ) -> Result<CallToolResult, McpError> {
        Ok(done(add_ssh(args).await))
    }

    #[tool(
        description = "Turn an MCP server secret that isn't used by an MCP server into a credential: to=\"ssh\" (an SSH login; give host, username, optional port) or to=\"password\" (a password for programs). The value moves inside keymule: nobody sees it or types it again. The user confirms in a dialog. Use it to fix an SSH password that was stored with add_mcp_secret."
    )]
    async fn convert_mcp_secret(
        &self,
        Parameters(args): Parameters<ConvertArgs>,
    ) -> Result<CallToolResult, McpError> {
        Ok(done(convert(args).await))
    }

    #[tool(
        description = "Replace a stored credential's secret, e.g. after the user rotated a token or it returns 401. The user types the new one into a secure dialog; you never see it."
    )]
    async fn rotate_credential(
        &self,
        Parameters(args): Parameters<RotateArgs>,
    ) -> Result<CallToolResult, McpError> {
        Ok(done(rotate(args).await))
    }

    #[tool(
        description = "Only for a secret an MCP server process reads (database URL, API key, password in its env or a remote server's header), without it entering the chat or a config file. Not for SSH logins (use add_ssh_credential), API tokens (add_credential) or passwords for other tools (add_credential with service password). Opens a secure dialog where the user types it. Then configure the server to run `keymule exec <name> -- <server command>`; keymule passes the secret in the env var you name, and the user approves the command once when it first starts. For a remote (HTTP) server, give `header` (e.g. x-api-key) instead of `env` and configure it as a stdio server running `keymule remote <name> <url>`."
    )]
    async fn add_mcp_secret(
        &self,
        Parameters(args): Parameters<McpSecretArgs>,
    ) -> Result<CallToolResult, McpError> {
        Ok(done(add_mcp_secret(args).await))
    }

    #[tool(
        description = "Save a secret keymule is holding from the user's message, under the name the user picked. Call it only after asking the user (offer two or three names that fit the conversation plus \"Don't save it\"; they can type their own). You never see the value."
    )]
    async fn save_held(
        &self,
        Parameters(a): Parameters<SaveHeldArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = tokio::task::spawn_blocking(move || {
            let mut store = Store::load()?;
            crate::hook::save_held(
                &a.id,
                &a.name,
                &a.description,
                &mut store,
                store::vault().as_ref(),
                crate::trust::now(),
            )
        })
        .await
        .map_err(|e| anyhow!(e.to_string()))
        .and_then(|r| r);
        Ok(done(result))
    }

    #[tool(
        description = "Throw away a secret keymule is holding from the user's message, when the user doesn't want it saved."
    )]
    async fn discard_held(
        &self,
        Parameters(a): Parameters<HeldIdArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = tokio::task::spawn_blocking(move || {
            crate::hook::discard_held(&a.id, store::vault().as_ref())
        })
        .await
        .map_err(|e| anyhow!(e.to_string()))
        .and_then(|r| r);
        Ok(done(result))
    }

    #[tool(
        description = "Change a credential's base URL, username or description, or an SSH login's host, port, username or key_file (a private key on this machine; the stored password is kept), e.g. to finish setting up one marked needs_setup. Moving it to a new host or server shows the user an approval dialog."
    )]
    async fn update_credential(
        &self,
        Parameters(args): Parameters<UpdateArgs>,
    ) -> Result<CallToolResult, McpError> {
        Ok(done(update(args).await))
    }

    #[tool(
        description = "Copy a stored credential into git's own credential store (macOS keychain, Git Credential Manager...) so git push/pull over HTTPS works as that account. The user approves in a dialog; you never see the secret. Pass `dir` (the repo folder) and the same dialog also lets the service's CLI (gh) use it there. Afterwards point the repo's remote at https://<username>@<host>/<owner>/<repo>.git. keymule keeps the copy updated on rotate and removes it on delete."
    )]
    async fn install_credential(
        &self,
        Parameters(args): Parameters<InstallArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = tokio::task::spawn_blocking(move || {
            let store = Store::load()?;
            let cli = args.dir.as_deref().and_then(|d| {
                crate::cliexec::cli_grant(
                    &store.get(&args.name).ok()?.service,
                    std::path::Path::new(d),
                )
            });
            crate::install::install(
                store::vault().as_ref(),
                &store,
                &args.name,
                &args.host,
                &args.username,
                cli,
                &|m| crate::trust::confirm(m),
                &crate::install::RealGit,
            )
        })
        .await
        .map_err(|e| anyhow!(e.to_string()))
        .and_then(|r| r);
        Ok(done(result))
    }

    #[tool(
        description = "Scan this machine for secrets that leaked to AI models (agent transcripts), sit in plain text in agent configs, or are readable in .env files and shell history. Results are masked; you never see a full secret. Takes a few seconds."
    )]
    async fn scan_for_leaks(
        &self,
        Parameters(args): Parameters<ScanArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = tokio::task::spawn_blocking(move || -> anyhow::Result<String> {
            let home = std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .ok_or_else(|| anyhow!("HOME is not set"))?;
            let dir = match args.dir {
                Some(d) => std::path::PathBuf::from(d),
                None => std::env::current_dir()?,
            };
            let report = crate::doctor::run(&crate::doctor::Options {
                home,
                dir,
                check_stored: true,
            });
            Ok(crate::doctor::render(&report, args.all.unwrap_or(false)))
        })
        .await
        .map_err(|e| anyhow!(e.to_string()))
        .and_then(|r| r);
        Ok(done(result))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("keymule", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }
}

pub async fn serve() -> anyhow::Result<()> {
    let service = Server::new().serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasted_keys_are_checked() {
        use crate::presets::Kind;
        let pem = "-----BEGIN PRIVATE KEY-----\r\nMIIE\r\n-----END PRIVATE KEY-----\n";
        assert_eq!(
            check_other_kind(Kind::PrivateKey, pem).unwrap(),
            "-----BEGIN PRIVATE KEY-----\nMIIE\n-----END PRIVATE KEY-----"
        );
        assert!(check_other_kind(Kind::PrivateKey, "ssh-ed25519 AAAA… public").is_err());
        let (id, key) = (
            crate::detect::fake::aws_id(),
            crate::detect::fake::aws_secret(),
        );
        let pair = format!(" {id} \n\n {key} ");
        assert_eq!(
            check_other_kind(Kind::AwsKeys, &pair).unwrap(),
            format!("{id}\n{key}")
        );
        assert!(check_other_kind(Kind::AwsKeys, "just-one-line").is_err());
    }
}
