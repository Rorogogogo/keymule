//! Website logins, filled into a browser the agent drives without the agent
//! ever holding the password.
//!
//! `keymule browser -- <browser MCP server>` starts a Chrome with keymule's
//! own profile (or reuses the one already running), points the browser tool
//! (Playwright MCP, Chrome DevTools MCP) at it, and sits between the agent
//! and the tool, hiding stored website passwords from every reply.
//!
//! The `sign_in` MCP tool then connects to the same Chrome, finds the tab
//! that is on the login's site, asks the user, and types the username and
//! password into the page itself. The site is sealed with the secret, and the
//! tab's origin is checked again right before each field is filled, so a
//! lookalike page (or one the sign-in redirected to) never gets it.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result, anyhow, bail};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio_tungstenite::tungstenite::Message;

use crate::presets;
use crate::store::{self, Auth, Credential, Sealed, Store, Vault};

mod cdp;
mod chrome;
mod proxy;
mod sign_in;
#[cfg(test)]
mod tests;

use cdp::*;
use chrome::*;
pub use proxy::*;
pub use sign_in::*;

/// What replaces a stored website password in the browser tool's replies.
pub const HIDDEN: &str = "[website password hidden by keymule]";

/// Where the running keymule Chrome is: `<config>/browser/state.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct State {
    pid: u32,
    port: u16,
    ws: String,
}

fn browser_dir() -> Result<PathBuf> {
    Ok(store::config_dir()?.join("browser"))
}

fn state_path() -> Result<PathBuf> {
    Ok(browser_dir()?.join("state.json"))
}

// ---- the site a login is for ----------------------------------------------

fn is_local(host: &str) -> bool {
    let name = host.rsplit_once(':').map_or(host, |(h, _)| h);
    matches!(name, "localhost" | "127.0.0.1" | "[::1]") || name.ends_with(".localhost")
}

/// The origin (`https://github.com`) a login with this sealed host fills
/// into. Logins are https, except to this machine, where `url`'s scheme
/// (from credentials.toml) decides.
pub fn origin_for(host: &str, url: &str) -> String {
    let scheme = if is_local(host) && url.starts_with("http://") {
        "http"
    } else {
        "https"
    };
    format!("{scheme}://{host}")
}

/// The origin of a page's URL, as `location.origin` gives it.
fn origin_of(url: &str) -> Option<String> {
    let o = reqwest::Url::parse(url).ok()?.origin();
    o.is_tuple().then(|| o.ascii_serialization())
}

/// Check a URL the user gave for a website login and return its sealed host.
pub fn login_host(url: &str) -> Result<String> {
    let host = store::host_of(url)?;
    if !url.starts_with("https://") && !is_local(&host) {
        bail!("a website login is only filled into https pages; use the https:// address");
    }
    Ok(host)
}
