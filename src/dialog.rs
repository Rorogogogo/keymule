//! Native dialogs shown by keymule itself, so the user can type a secret
//! or approve a change without it passing through the agent. The text goes
//! to AppleScript as an argument, never spliced into the script, so names
//! and URLs chosen by an agent cannot inject AppleScript.

use anyhow::Result;

#[derive(Debug, PartialEq, Eq)]
pub enum Answer {
    Text(String),
    Cancelled,
    /// Only the macOS dialog can time out.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    TimedOut,
}

#[cfg(target_os = "macos")]
const TIMEOUT_SECS: u32 = 180;

#[cfg(target_os = "macos")]
fn osascript(lines: &[String], message: &str) -> Result<Answer> {
    use anyhow::Context;
    let mut cmd = std::process::Command::new("/usr/bin/osascript");
    for line in lines {
        cmd.arg("-e").arg(line);
    }
    let out = cmd
        .arg(message)
        .stdin(std::process::Stdio::null())
        .output()
        .context("running osascript")?;
    if out.status.success() {
        let text = String::from_utf8_lossy(&out.stdout);
        return Ok(Answer::Text(
            text.trim_end_matches(['\n', '\r']).to_string(),
        ));
    }
    let err = String::from_utf8_lossy(&out.stderr);
    if err.contains("(-128)") {
        Ok(Answer::Cancelled)
    } else if err.contains("keymule-timeout") {
        Ok(Answer::TimedOut)
    } else {
        anyhow::bail!("dialog failed: {}", err.trim())
    }
}

/// Debug builds only: `KEYMULE_TEST_DIALOG` answers dialogs ("cancel",
/// "allow", "deny", or the secret to "type"), and `KEYMULE_TEST_DIALOG_LOG`
/// records what each dialog said.
#[cfg(debug_assertions)]
fn test_answer(message: &str) -> Option<String> {
    let answer = std::env::var("KEYMULE_TEST_DIALOG").ok()?;
    if let Some(log) = std::env::var_os("KEYMULE_TEST_DIALOG_LOG") {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
        {
            let _ = writeln!(f, "{message}\n---");
        }
    }
    Some(answer)
}

#[cfg(not(debug_assertions))]
fn test_answer(_message: &str) -> Option<String> {
    None
}

/// Ask for a secret in a hidden-input dialog.
pub fn secret(message: &str) -> Result<Answer> {
    if let Some(a) = test_answer(message) {
        return Ok(if a == "cancel" {
            Answer::Cancelled
        } else {
            Answer::Text(a)
        });
    }
    platform_secret(message)
}

/// Ask the user to allow something. Defaults to "Don't Allow".
pub fn confirm(message: &str) -> Result<bool> {
    if let Some(a) = test_answer(message) {
        return Ok(a == "allow");
    }
    platform_confirm(message)
}

#[cfg(target_os = "macos")]
fn platform_secret(message: &str) -> Result<Answer> {
    let script = [
        "on run argv".to_string(),
        format!(
            "set r to display dialog (item 1 of argv) default answer \"\" with hidden answer \
             with title \"keymule\" buttons {{\"Cancel\", \"Save\"}} default button \"Save\" \
             cancel button \"Cancel\" with icon caution giving up after {TIMEOUT_SECS}"
        ),
        "if gave up of r then error \"keymule-timeout\" number 2".to_string(),
        "return text returned of r".to_string(),
        "end run".to_string(),
    ];
    Ok(match osascript(&script, message)? {
        Answer::Text(t) if t.trim().is_empty() => Answer::Cancelled,
        Answer::Text(t) => Answer::Text(t.trim().to_string()),
        other => other,
    })
}

#[cfg(target_os = "macos")]
fn platform_confirm(message: &str) -> Result<bool> {
    let script = [
        "on run argv".to_string(),
        format!(
            "set r to display dialog (item 1 of argv) with title \"keymule\" \
             buttons {{\"Don't Allow\", \"Allow\"}} default button \"Don't Allow\" \
             cancel button \"Don't Allow\" with icon caution giving up after {TIMEOUT_SECS}"
        ),
        "if gave up of r then error \"keymule-timeout\" number 2".to_string(),
        "return button returned of r".to_string(),
        "end run".to_string(),
    ];
    Ok(osascript(&script, message)? == Answer::Text("Allow".into()))
}

#[cfg(not(target_os = "macos"))]
fn platform_secret(_message: &str) -> Result<Answer> {
    anyhow::bail!("the secure dialog is macOS-only for now; run `keymule add` in a terminal")
}

#[cfg(not(target_os = "macos"))]
fn platform_confirm(_message: &str) -> Result<bool> {
    anyhow::bail!("the confirmation dialog is macOS-only for now; run `keymule edit` in a terminal")
}
