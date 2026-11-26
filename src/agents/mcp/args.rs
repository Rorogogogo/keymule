//! What each MCP tool takes. The doc comments are the descriptions agents see.

use std::collections::BTreeMap;

use rmcp::schemars::{self, JsonSchema};
use serde::Deserialize;

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

pub(super) const DEFAULT_MAX_BYTES: usize = 60_000;

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
