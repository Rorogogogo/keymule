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
use crate::store::{self, Auth, Credential, Sealed, Store, host_of, validate_name};

const INSTRUCTIONS: &str = "keymule holds the user's API tokens and passwords. \
Whenever the user asks about a service that needs authentication (Bamboo, GitHub Actions, GitLab, \
Jenkins, internal APIs...), call list_credentials first and use http_request with the matching \
credential name. Never ask the user to paste a token, password or API key into the chat. If no \
matching credential exists, call add_credential: keymule shows a secure dialog on the user's \
screen where they type the secret, and you never see it. Ask the user for the service's base URL \
first if needed (URLs are not secret). If a token stops working (HTTP 401), offer \
rotate_credential. When you set up an MCP server that needs a password, API key or database URL, \
never write the secret into the MCP config or ask for it in chat: call add_mcp_secret (the user \
types it in a dialog), then configure the server to run `keymule exec <name> [--env VAR] -- \
<server command>`. The user approves that command once, in a dialog, when it first starts.";

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddArgs {
    /// Service preset: bamboo, github, gitlab, jenkins or generic.
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
    /// The env var the MCP server reads it from, e.g. "DATABASE_URI" or "NOTION_TOKEN".
    pub env: String,
    /// What it is, shown in the dialog, e.g. "Postgres URL for the local dev database".
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateArgs {
    /// Credential name from list_credentials.
    pub name: String,
    /// New base URL. A new host needs the user's approval in a dialog.
    #[serde(default)]
    pub url: Option<String>,
    /// Username for basic auth.
    #[serde(default)]
    pub username: Option<String>,
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
    match tokio::task::spawn_blocking(move || dialog::secret(&message)).await?? {
        Answer::Text(s) => Ok(s),
        Answer::Cancelled => bail!("The user cancelled the dialog. Nothing was saved."),
        Answer::TimedOut => bail!("The dialog timed out after 3 minutes. Nothing was saved."),
    }
}

async fn ask_confirm(message: String) -> anyhow::Result<bool> {
    tokio::task::spawn_blocking(move || dialog::confirm(&message)).await?
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
    let host = host_of(&url)?;
    let auth = presets::auth_for(preset, a.header, a.username)
        .map_err(|e| anyhow!("{e}. Ask the user for their username and pass it as `username`."))?;

    let secret = ask_secret(format!(
        "Paste your {} token for \"{name}\".\n\nkeymule will only ever send it to:\n\n        {host}\n\n\
         If you don't recognise that host, click Cancel.",
        preset.title
    ))
    .await?;
    let cred = Credential {
        service: preset.id.into(),
        url,
        description: a.description.unwrap_or_default(),
        auth,
        hosts: vec![host],
        added_by: "mcp".into(),
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
    let title = presets::get_or_generic(&cred.service).title;
    let hosts = if cred.hosts.is_empty() {
        "(no host set yet)".to_string()
    } else {
        cred.hosts.join(", ")
    };
    let secret = ask_secret(format!(
        "Paste the NEW {title} token for \"{}\".\n\nIt will only ever be sent to:\n\n        {hosts}",
        a.name
    ))
    .await?;
    let note = check(&a.name, &cred, &secret, a.verify.unwrap_or(true)).await?;
    sealed.secret = secret;
    sealed.save(store::vault().as_ref(), &a.name)?;
    Ok(format!(
        "Rotated \"{}\"{note}. Remind the user to revoke the old token at the service.",
        a.name
    ))
}

async fn add_mcp_secret(a: McpSecretArgs) -> anyhow::Result<String> {
    validate_name(&a.name)?;
    let env_ok = !a.env.is_empty()
        && a.env.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !a.env.starts_with(|c: char| c.is_ascii_digit());
    if !env_ok {
        bail!("env must be a variable name like DATABASE_URI");
    }
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
        "Your agent is setting up an MCP server that needs {}{what}.\n\n\
         Type the value here. keymule keeps it in the keychain, and only a server command you \
         approve will get it.",
        a.env
    ))
    .await?;

    let mut binding = crate::servers::Binding::default();
    binding.env.insert(a.env.clone(), value);
    let mut store = Store::load()?;
    if store.creds.contains_key(&a.name) || store.servers.contains_key(&a.name) {
        bail!(
            "\"{}\" was created by something else meanwhile. Nothing was saved.",
            a.name
        );
    }
    binding.save(store::vault().as_ref(), &a.name)?;
    store.servers.insert(
        a.name.clone(),
        crate::store::ServerMeta {
            env: vec![a.env.clone()],
            arg_secrets: 0,
            used_by: Vec::new(),
        },
    );
    store.save()?;
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "keymule".into());
    Ok(format!(
        "Saved \"{name}\". Configure the MCP server with command \"{exe}\" and args [\"exec\", \"{name}\", \"--\", <the server's command and args>]. keymule sets {env} for it. Do not put the value in the config. The first time the server starts, the user approves the command in a dialog. Example: claude mcp add <server> -- \"{exe}\" exec {name} -- <command> <args>",
        name = a.name,
        env = a.env,
    ))
}

async fn update(a: UpdateArgs) -> anyhow::Result<String> {
    let mut sealed = load_sealed(&a.name).await?;
    // Check new hosts against the sealed rules, not the editable file.
    let mut cred = sealed.apply(Store::load()?.get(&a.name)?);
    let mut changed = Vec::new();
    if let Some(url) = a.url {
        let url = url.trim_end_matches('/').to_string();
        let host = host_of(&url)?;
        if !cred.hosts.contains(&host) {
            let current = if cred.hosts.is_empty() {
                String::new()
            } else {
                format!("Currently it may only go to: {}\n\n", cred.hosts.join(", "))
            };
            let allowed = ask_confirm(format!(
                "Your agent wants to let the credential \"{}\" be sent to:\n\n        {host}\n\n\
                 {current}Only allow this if you asked for it.",
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
        cred.hosts = vec![host];
        cred.url = url;
        changed.push("url");
    }
    if let Some(u) = a.username {
        cred.auth = Auth::Basic { username: u };
        changed.push("username");
    }
    if let Some(d) = a.description {
        cred.description = d;
        changed.push("description");
    }
    if changed.is_empty() {
        bail!("nothing to change: pass url, username or description");
    }
    let mut store = Store::load()?;
    store.get(&a.name)?;
    sealed.hosts = cred.hosts.clone();
    sealed.auth = cred.auth.clone();
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
        if store.creds.is_empty() && store.servers.is_empty() {
            return Ok(text(
                "No credentials stored yet. Use add_credential (services: bamboo, github, gitlab, \
                 jenkins, generic); the user types the secret into a secure dialog.",
            ));
        }
        let list: Vec<_> = store
            .creds
            .iter()
            .map(|(name, c)| {
                let preset = presets::get_or_generic(&c.service);
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
                serde_json::json!({
                    "name": name,
                    "service": preset.title,
                    "base_url": c.url,
                    "allowed_hosts": c.hosts,
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
                    "env": s.env,
                    "used_by": s.used_by,
                    "how_to_use": format!(
                        "in an MCP server config: keymule exec {name} [--env VAR] -- <server command>"
                    ),
                })
            })
            .collect();
        let out = serde_json::json!({ "credentials": list, "mcp_server_secrets": servers });
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
        description = "Store a new credential. keymule opens a secure dialog on the user's screen where they type the token; you never see it. It is verified against the service and then locked to the URL's host. Ask the user for the base URL first if the service needs one."
    )]
    async fn add_credential(
        &self,
        Parameters(args): Parameters<AddArgs>,
    ) -> Result<CallToolResult, McpError> {
        Ok(done(add(args).await))
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
        description = "Store a secret an MCP server needs (database URL, API key, password) without it entering the chat or a config file. Opens a secure dialog where the user types it. Then configure the server to run `keymule exec <name> -- <server command>`; keymule passes the secret in the env var you name, and the user approves the command once when it first starts."
    )]
    async fn add_mcp_secret(
        &self,
        Parameters(args): Parameters<McpSecretArgs>,
    ) -> Result<CallToolResult, McpError> {
        Ok(done(add_mcp_secret(args).await))
    }

    #[tool(
        description = "Change a credential's base URL, username or description, e.g. to finish setting up one marked needs_setup. Moving it to a new host shows the user an approval dialog."
    )]
    async fn update_credential(
        &self,
        Parameters(args): Parameters<UpdateArgs>,
    ) -> Result<CallToolResult, McpError> {
        Ok(done(update(args).await))
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
