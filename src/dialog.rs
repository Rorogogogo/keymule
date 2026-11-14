//! Native dialogs shown by keymule itself, so the user can type a secret
//! or approve a change without it passing through the agent.
//!
//! On macOS each one is a small window showing src/dialog.html, opened by
//! src/dialog.js under `osascript -l JavaScript`. Its text goes in as a JSON
//! literal on stdin, never as script source or a command-line argument, and
//! the page sets it with textContent, so names and URLs chosen by an agent
//! can't inject code, and other processes can't read it.

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
const SCRIPT: &str = include_str!("dialog.js");

#[cfg(target_os = "macos")]
const PAGE: &str = include_str!("dialog.html");

/// What a dialog shows. `message` is split into a bold headline (its first
/// paragraph) and the text below; lines indented by 8 spaces show as code.
#[cfg(target_os = "macos")]
#[derive(serde::Serialize)]
struct Spec<'a> {
    title: &'a str,
    body: &'a str,
    /// "key", "unlock", "trash", "eye" or "shield".
    icon: &'a str,
    /// "" (the accent colour), "warn" or "bad".
    tone: &'a str,
    /// Left to right.
    buttons: &'a [&'a str],
    /// The filled button.
    primary: &'a str,
    /// Return presses it.
    default: &'a str,
    /// Escape presses it.
    cancel: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    destructive: Option<&'a str>,
    secure: bool,
    timeout: u32,
}

#[cfg(target_os = "macos")]
impl<'a> Spec<'a> {
    /// Buttons left to right: the first cancels and is the default, the
    /// last is filled.
    fn new(message: &'a str, icon: &'a str, tone: &'a str, buttons: &'a [&'a str]) -> Self {
        let (title, body) = message.split_once("\n\n").unwrap_or((message, ""));
        Spec {
            title,
            body,
            icon,
            tone,
            buttons,
            primary: buttons[buttons.len() - 1],
            default: buttons[0],
            cancel: buttons[0],
            destructive: None,
            secure: false,
            timeout: TIMEOUT_SECS,
        }
    }
}

#[cfg(target_os = "macos")]
#[derive(Debug, serde::Deserialize)]
struct Picked {
    button: String,
    #[serde(default)]
    text: Option<String>,
}

/// Show the alert and wait. None if nobody answered in time.
#[cfg(target_os = "macos")]
fn alert(spec: &Spec) -> Result<Option<Picked>> {
    use anyhow::Context;
    use std::io::Write;
    use std::process::{Command, Stdio};
    let script = format!(
        "{SCRIPT}\nfunction run() {{ return show({}, {}); }}\n",
        serde_json::to_string(spec)?,
        serde_json::to_string(PAGE)?
    );
    let mut child = Command::new("/usr/bin/osascript")
        .args(["-l", "JavaScript", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("running osascript")?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(script.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    if out.status.success() {
        let picked = serde_json::from_slice(&out.stdout).context("reading the dialog's answer")?;
        return Ok(Some(picked));
    }
    let err = String::from_utf8_lossy(&out.stderr);
    if err.contains("keymule-timeout") {
        Ok(None)
    } else {
        anyhow::bail!("dialog failed: {}", err.trim())
    }
}

/// The button the user clicked, "" if the dialog timed out.
#[cfg(target_os = "macos")]
fn pick(spec: &Spec) -> Result<String> {
    Ok(alert(spec)?.map(|p| p.button).unwrap_or_default())
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

/// Ask before deleting something. Defaults to "Cancel".
pub fn confirm_delete(message: &str) -> Result<bool> {
    if let Some(a) = test_answer(message) {
        return Ok(a == "allow");
    }
    platform_confirm_delete(message)
}

#[cfg(target_os = "macos")]
fn platform_confirm_delete(message: &str) -> Result<bool> {
    let spec = Spec {
        destructive: Some("Delete"),
        ..Spec::new(message, "trash", "bad", &["Cancel", "Delete"])
    };
    Ok(pick(&spec)? == "Delete")
}

#[cfg(not(target_os = "macos"))]
fn platform_confirm_delete(_message: &str) -> Result<bool> {
    anyhow::bail!("the confirmation dialog is macOS-only for now; run `keymule rm` in a terminal")
}

/// Ask the user to allow something. Return and Escape both cancel.
pub fn confirm(message: &str) -> Result<bool> {
    if let Some(a) = test_answer(message) {
        return Ok(a == "allow");
    }
    platform_confirm(message)
}

/// The allow-all dialog: Cancel / Today / Always (Return picks Always).
/// Test answers: "today", "allow" (until off), anything else is no.
pub fn allow_all(message: &str) -> Result<Option<crate::trust::Span>> {
    use crate::trust::Span;
    let pick = match test_answer(message) {
        Some(a) => a,
        None => platform_allow_all(message)?,
    };
    Ok(match pick.as_str() {
        "today" | "Today" => Some(Span::Today),
        "allow" | "Always" => Some(Span::UntilOff),
        _ => None,
    })
}

#[cfg(target_os = "macos")]
fn platform_allow_all(message: &str) -> Result<String> {
    // Return picks Always: the user asked for this switch.
    let spec = Spec {
        default: "Always",
        ..Spec::new(message, "unlock", "warn", &["Cancel", "Today", "Always"])
    };
    pick(&spec)
}

#[cfg(not(target_os = "macos"))]
fn platform_allow_all(_message: &str) -> Result<String> {
    anyhow::bail!("the allow-all dialog is macOS-only for now")
}

/// What to do with a message that has a secret in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteChoice {
    Save,
    SendAsIs,
    DontSend,
}

/// Ask what to do with a pasted secret: Don't Send / Send Anyway / Save
/// (the default). None if no dialog could be shown or it timed
/// out. Test answers: "save" or "allow", "send", anything else is don't.
pub fn paste_choice(message: &str) -> Option<PasteChoice> {
    let pick = match test_answer(message) {
        Some(a) => a,
        None => platform_paste_choice(message).ok()?,
    };
    match pick.as_str() {
        "save" | "allow" | "Save" => Some(PasteChoice::Save),
        "send" | "Send Anyway" => Some(PasteChoice::SendAsIs),
        "" => None,
        _ => Some(PasteChoice::DontSend),
    }
}

#[cfg(target_os = "macos")]
fn platform_paste_choice(message: &str) -> Result<String> {
    let spec = Spec {
        default: "Save",
        ..Spec::new(
            message,
            "shield",
            "warn",
            &["Don't Send", "Send Anyway", "Save"],
        )
    };
    pick(&spec)
}

#[cfg(not(target_os = "macos"))]
fn platform_paste_choice(_message: &str) -> Result<String> {
    anyhow::bail!("no dialog here")
}

/// Show stored values on screen, after the user agrees to it. Returns false
/// if they didn't. `copy` is the value the Copy button puts on the clipboard
/// (cleared again after 30 seconds); without it there is no Copy button.
/// With `ask_first` false (allow all), it shows them straight away.
pub fn reveal(
    name: &str,
    items: &[(String, String)],
    copy: Option<&str>,
    ask_first: bool,
) -> Result<bool> {
    let shown = items
        .iter()
        .map(|(k, v)| format!("{k}\n{v}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    let ask = format!("Show \"{name}\" on screen?\n\nIt closes after 30 seconds.");
    let question = ask;
    let ask = ask_first;
    if let Some(a) = test_answer(&question) {
        if ask && a != "allow" {
            return Ok(false);
        }
        let _ = test_answer(&shown);
        return Ok(true);
    }
    if ask && !platform_ask_reveal(&question)? {
        return Ok(false);
    }
    platform_show(name, items, copy)?;
    Ok(true)
}

#[cfg(target_os = "macos")]
fn platform_ask_reveal(message: &str) -> Result<bool> {
    Ok(pick(&Spec::new(message, "eye", "warn", &["Cancel", "Show"]))? == "Show")
}

/// An AppleScript string literal.
#[cfg(target_os = "macos")]
fn applescript_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\" & linefeed & \""),
            '\r' => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Run an AppleScript given on stdin. Secrets go this way, never as
/// arguments: other processes, agents included, can read a command line.
#[cfg(target_os = "macos")]
fn osascript_stdin(script: &str, wait: bool) -> Result<()> {
    use anyhow::Context;
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("/usr/bin/osascript")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("running osascript")?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(script.as_bytes())?;
    }
    if wait {
        child.wait()?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn platform_show(name: &str, items: &[(String, String)], copy: Option<&str>) -> Result<()> {
    // Each value as code under its label. The values travel on stdin only.
    let mut message = format!("keymule · {name}");
    for (k, v) in items {
        message.push_str(&format!("\n\n{k}\n\n"));
        let code: Vec<String> = v.lines().map(|l| format!("        {l}")).collect();
        message.push_str(&code.join("\n"));
    }
    let buttons: &[&str] = if copy.is_some() {
        &["Copy", "Done"]
    } else {
        &["Done"]
    };
    let spec = Spec {
        default: "Done",
        cancel: "Done",
        timeout: 30,
        ..Spec::new(&message, "eye", "", buttons)
    };
    if pick(&spec)? != "Copy" {
        return Ok(());
    }
    let Some(v) = copy else { return Ok(()) };
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut pb = Command::new("/usr/bin/pbcopy")
        .stdin(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = pb.stdin.take() {
        stdin.write_all(v.as_bytes())?;
    }
    pb.wait()?;
    // Clear the clipboard in 30 seconds if it still holds the value. This
    // runs on after keymule exits; the value goes in on stdin.
    osascript_stdin(
        &format!(
            "delay 30\ntry\nif (the clipboard as text) is {} then set the clipboard to \"\"\nend try\n",
            applescript_str(v)
        ),
        false,
    )?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn platform_ask_reveal(_message: &str) -> Result<bool> {
    anyhow::bail!("revealing a secret is macOS-only for now")
}

#[cfg(not(target_os = "macos"))]
fn platform_show(_name: &str, _items: &[(String, String)], _copy: Option<&str>) -> Result<()> {
    anyhow::bail!("revealing a secret is macOS-only for now")
}

#[cfg(target_os = "macos")]
fn platform_secret(message: &str) -> Result<Answer> {
    let spec = Spec {
        default: "Save",
        cancel: "Cancel",
        secure: true,
        ..Spec::new(message, "key", "", &["Cancel", "Save"])
    };
    Ok(match alert(&spec)? {
        None => Answer::TimedOut,
        Some(p) if p.button != "Save" => Answer::Cancelled,
        Some(p) => match p.text.unwrap_or_default().trim() {
            "" => Answer::Cancelled,
            t => Answer::Text(t.to_string()),
        },
    })
}

#[cfg(target_os = "macos")]
fn platform_confirm(message: &str) -> Result<bool> {
    Ok(pick(&Spec::new(message, "key", "", &["Cancel", "Allow"]))? == "Allow")
}

#[cfg(not(target_os = "macos"))]
fn platform_secret(_message: &str) -> Result<Answer> {
    anyhow::bail!("the secure dialog is macOS-only for now; run `keymule add` in a terminal")
}

#[cfg(not(target_os = "macos"))]
fn platform_confirm(_message: &str) -> Result<bool> {
    anyhow::bail!("the confirmation dialog is macOS-only for now; run `keymule edit` in a terminal")
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::applescript_str;

    #[test]
    fn applescript_strings_are_escaped() {
        assert_eq!(applescript_str("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(applescript_str("x\ny"), "\"x\" & linefeed & \"y\"");
    }
}
