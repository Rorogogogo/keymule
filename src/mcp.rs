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

use crate::http;
use crate::presets;
use crate::store::Store;

const INSTRUCTIONS: &str = "agentkey holds the user's API tokens and passwords. \
Whenever the user asks about a service that needs authentication (Bamboo, GitHub Actions, GitLab, \
Jenkins, internal APIs...), call list_credentials first and use http_request with the matching \
credential name. Never ask the user to paste a token, password or API key into the chat. If no \
matching credential exists, tell the user to run `agentkey add <service>` in their own terminal.";

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
        if store.creds.is_empty() {
            return Ok(text(
                "No credentials stored. Ask the user to run `agentkey add <service>` in their terminal \
                 (services: bamboo, github, gitlab, jenkins, generic).",
            ));
        }
        let list: Vec<_> = store
            .creds
            .iter()
            .map(|(name, c)| {
                let preset = presets::get_or_generic(&c.service);
                let mut setup = Vec::new();
                if c.url.is_empty() {
                    setup.push(format!("agentkey edit {name} --url <base url>"));
                }
                if matches!(&c.auth, crate::store::Auth::Basic { username } if username.is_empty()) {
                    setup.push(format!("agentkey edit {name} --username <user>"));
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
        Ok(text(
            serde_json::to_string_pretty(&list).unwrap_or_default(),
        ))
    }

    #[tool(
        description = "Call an HTTP API using one of the user's stored credentials. agentkey attaches the secret itself, only for the credential's allowed hosts, and redacts it from the response. You never see the secret."
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
        let name = args.credential.clone();
        // The keychain may show a macOS approval dialog; keep it off the async runtime.
        let secret =
            match tokio::task::spawn_blocking(move || crate::store::vault().get(&name)).await {
                Ok(Ok(s)) => s,
                Ok(Err(e)) => return Ok(tool_error(format!("{e:#}"))),
                Err(e) => return Ok(tool_error(e.to_string())),
            };
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
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("agentkey", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }
}

pub async fn serve() -> anyhow::Result<()> {
    let service = Server::new().serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
