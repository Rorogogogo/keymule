//! stdio MCP server. Agents see credential names and metadata only; secret
//! values are read from the keychain inside `http_request` and never
//! returned.

mod add;
mod args;
mod capture;
mod edit;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ErrorData as McpError, ServerHandler, ServiceExt, tool, tool_handler, tool_router};

use anyhow::{anyhow, bail};

use crate::dialog::{self, Answer};
use crate::http;
use crate::presets;
use crate::store::{
    self, Auth, Credential, Sealed, SshAuth, SshTarget, Store, host_of, validate_name,
};

use add::*;
use capture::*;
use edit::*;

pub use add::{add_ssh, add_with_dialog};
pub use args::*;
pub use edit::rotate_with_dialog;

const INSTRUCTIONS: &str = "keymule holds the user's API tokens and passwords. \
Whenever the user asks about a service that needs authentication (Bamboo, GitHub Actions, GitLab, \
Jenkins, internal APIs...), call list_credentials first and use http_request with the matching \
credential name. For a server the user logs in to with SSH (host or IP, username, password or \
key), call add_ssh_credential, then run commands there with `keymule ssh <name> -- <command>`; \
add_mcp_secret is only for secrets an MCP server reads, never for SSH logins or passwords for \
other tools (those are add_credential with service password). To sign in to a website in a \
browser, store the login with add_website_login and call sign_in once its sign-in page is open \
in the keymule browser (`keymule browser -- <browser MCP server>`, or the window `keymule browser --open` opens for Claude in Chrome). Never ask the user to paste a token, password or API key into the chat. If no \
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

fn done(result: anyhow::Result<String>) -> CallToolResult {
    match result {
        Ok(s) => text(s),
        Err(e) => tool_error(format!("{e:#}")),
    }
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
                            "open the site's sign-in page in the keymule browser (your browser tool run through `keymule browser -- ...`, or with Claude in Chrome the Chrome `keymule browser --open` opened), then sign_in(credential=\"{name}\"); the user approves each sign-in"
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
        description = "Sign in to a website with a stored website login, in the keymule browser (the Chrome your browser tool uses when it runs through `keymule browser -- ...`; with Claude in Chrome, the Chrome `keymule browser --open` opened, not the user's everyday one). Open the site's sign-in page in it first. keymule asks the user, checks the tab is on exactly the login's site, types the username and password into the page and submits; two-step forms (username, then password) are handled. You never see the password: it's hidden from a browser MCP server's replies, and cleared from the page after submitting. Then take a snapshot to carry on."
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
