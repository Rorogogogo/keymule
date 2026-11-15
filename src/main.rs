//! keymule: let AI agents use your tokens and passwords without ever seeing
//! them.
//!
//! - `cli` and `commands`: the command line and what each command does
//! - `storage`: the vault and what's in it
//! - `consent`: asking the user (popups, page approvals, allow-all)
//! - `agents`: the MCP server, paste hook and agent setup
//! - `usage`: using a secret without showing it
//! - `scan`: finding secrets, before sending and after a leak
//! - `app`: the local page and the menu bar app

mod agents;
mod app;
mod cli;
mod commands;
mod consent;
mod scan;
mod storage;
mod usage;

// Each module keeps its short path (`crate::store`, `crate::hook`…).
use agents::{adopt, guard, hook, init, mcp};
#[cfg(target_os = "macos")]
use app::menubar;
use app::ui;
use consent::{approvals, dialog, trust, tty};
use scan::{detect, doctor};
use storage::{presets, redact, store, vault};
use usage::{cliexec, ghsecret, http, install, remote, servers, ssh};

use clap::Parser;

#[tokio::main]
async fn main() {
    // ssh runs keymule as its password helper (see `keymule ssh`).
    if let Some(code) = ssh::askpass_main() {
        std::process::exit(code);
    }
    if let Err(e) = commands::run(cli::Cli::parse()).await {
        eprintln!("keymule: {e:#}");
        std::process::exit(1);
    }
}
