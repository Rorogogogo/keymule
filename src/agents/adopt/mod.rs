//! `keymule adopt`: move secrets that MCP server entries keep in plain text
//! (env vars, inline `VAR=value` commands, arguments) into the keychain, and
//! rewrite each entry to `keymule exec <name> -- <its original command>`.
//! Remote (HTTP) servers whose headers hold a key become
//! `keymule remote <name> <url>`, a local bridge that adds the headers.

mod apply;
mod find;
mod plan;
mod render;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};

use crate::detect;
use crate::doctor::{is_secret_name, mask};
use crate::servers::{Binding, placeholder};
use crate::store::{Store, Vault, validate_name};

pub use apply::*;
use find::*;
pub use plan::*;

pub struct Options {
    pub home: PathBuf,
    /// Project directory, to report (not rewrite) its .mcp.json.
    pub dir: PathBuf,
    /// Path of the keymule binary to put into configs.
    pub exe: String,
    /// Servers to adopt even when their secrets look like defaults (a local
    /// database's `postgres` password, say).
    pub force: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Json,
    Toml,
}

struct Config {
    path: PathBuf,
    display: String,
    format: Format,
    /// A project's own file (likely committed), reported but not rewritten.
    project: bool,
    text: String,
    /// Parsed contents (TOML converted to JSON for reading).
    value: Value,
}

/// One config entry to rewrite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    config: usize,
    /// Keys leading to the server's object, e.g. ["projects", "/x", "mcpServers", "gh"].
    keys: Vec<String>,
    pub place: String,
    /// Program and arguments, with `{keymule:N}` placeholders for secrets.
    argv: Vec<String>,
    secret_env: BTreeMap<String, String>,
    arg_secrets: BTreeMap<String, String>,
    /// The env object to leave in the config (non-secret vars).
    keep_env: Map<String, Value>,
    had_env: bool,
    /// For a remote server: its URL, the headers keymule takes over, and the
    /// rest ("Name: value"), which stay in the config as `--header` args.
    url: Option<String>,
    secret_headers: BTreeMap<String, String>,
    keep_headers: Vec<String>,
    pub binding: String,
}

#[derive(Debug)]
pub struct Skipped {
    pub place: String,
    pub reason: &'static str,
}

pub struct Plan {
    configs: Vec<Config>,
    pub targets: Vec<Target>,
    pub skipped: Vec<Skipped>,
    /// Bindings to save: existing ones with new commands, and new ones.
    bindings: BTreeMap<String, Binding>,
    exe: String,
}
