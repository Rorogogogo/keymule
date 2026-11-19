//! Setting keymule up: the menu bar app, and small helpers for the hook
//! commands.

use std::io::Read;

use anyhow::Result;
#[cfg(not(target_os = "macos"))]
use anyhow::bail;

#[cfg(target_os = "macos")]
use crate::*;

pub(super) fn home() -> Result<std::path::PathBuf> {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("HOME is not set"))
}

#[cfg(target_os = "macos")]
pub(super) fn menubar_cmd(background: bool, login: Option<String>, restart: bool) -> Result<()> {
    if restart {
        menubar::restart()?;
        println!(
            "Restarted keymule in the menu bar ({}).",
            crate::app::update::CURRENT
        );
        return Ok(());
    }
    match login.as_deref() {
        Some(v) => {
            menubar::set_login(v == "on")?;
            println!(
                "{}",
                if v == "on" {
                    "keymule will start in the menu bar at login."
                } else {
                    "keymule won't start at login any more."
                }
            );
            Ok(())
        }
        None => menubar::run(background),
    }
}

#[cfg(not(target_os = "macos"))]
pub(super) fn menubar_cmd(_background: bool, _login: Option<String>, _restart: bool) -> Result<()> {
    bail!("the menu bar app is macOS-only; use `keymule ui`")
}

pub(super) fn read_stdin() -> Result<String> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    Ok(input)
}
