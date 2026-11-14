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

/// Ask before deleting something. Defaults to "Cancel".
pub fn confirm_delete(message: &str) -> Result<bool> {
    if let Some(a) = test_answer(message) {
        return Ok(a == "allow");
    }
    platform_confirm_delete(message)
}

#[cfg(target_os = "macos")]
fn platform_confirm_delete(message: &str) -> Result<bool> {
    let script = [
        "on run argv".to_string(),
        format!(
            "set r to display dialog (item 1 of argv) with title \"keymule\" \
             buttons {{\"Cancel\", \"Delete\"}} default button \"Cancel\" \
             cancel button \"Cancel\" with icon caution giving up after {TIMEOUT_SECS}"
        ),
        "if gave up of r then error \"keymule-timeout\" number 2".to_string(),
        "return button returned of r".to_string(),
        "end run".to_string(),
    ];
    Ok(osascript(&script, message)? == Answer::Text("Delete".into()))
}

#[cfg(not(target_os = "macos"))]
fn platform_confirm_delete(_message: &str) -> Result<bool> {
    anyhow::bail!("the confirmation dialog is macOS-only for now; run `keymule rm` in a terminal")
}

/// Ask the user to allow something. Defaults to "Don't Allow".
pub fn confirm(message: &str) -> Result<bool> {
    if let Some(a) = test_answer(message) {
        return Ok(a == "allow");
    }
    platform_confirm(message)
}

/// The allow-all dialog: Don't Allow / Allow for Today / Allow Until I Turn
/// It Off. Test answers: "today", "allow" (until off), anything else is no.
pub fn allow_all(message: &str) -> Result<Option<crate::trust::Span>> {
    use crate::trust::Span;
    let pick = match test_answer(message) {
        Some(a) => a,
        None => platform_allow_all(message)?,
    };
    Ok(match pick.as_str() {
        "today" | "Allow for Today" => Some(Span::Today),
        "allow" | "Allow Until I Turn It Off" => Some(Span::UntilOff),
        _ => None,
    })
}

#[cfg(target_os = "macos")]
fn platform_allow_all(message: &str) -> Result<String> {
    let script = [
        "on run argv".to_string(),
        format!(
            "set r to display dialog (item 1 of argv) with title \"keymule\" \
             buttons {{\"Don't Allow\", \"Allow for Today\", \"Allow Until I Turn It Off\"}} \
             default button \"Don't Allow\" cancel button \"Don't Allow\" with icon caution \
             giving up after {TIMEOUT_SECS}"
        ),
        "if gave up of r then error \"keymule-timeout\" number 2".to_string(),
        "return button returned of r".to_string(),
        "end run".to_string(),
    ];
    Ok(match osascript(&script, message)? {
        Answer::Text(t) => t,
        _ => String::new(),
    })
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

/// Ask what to do with a pasted secret: Don't Send / Send As Is / Save in
/// keymule (the default). None if no dialog could be shown or it timed
/// out. Test answers: "save" or "allow", "send", anything else is don't.
pub fn paste_choice(message: &str) -> Option<PasteChoice> {
    let pick = match test_answer(message) {
        Some(a) => a,
        None => platform_paste_choice(message).ok()?,
    };
    match pick.as_str() {
        "save" | "allow" | "Save in keymule" => Some(PasteChoice::Save),
        "send" | "Send As Is" => Some(PasteChoice::SendAsIs),
        "" => None,
        _ => Some(PasteChoice::DontSend),
    }
}

#[cfg(target_os = "macos")]
fn platform_paste_choice(message: &str) -> Result<String> {
    let script = [
        "on run argv".to_string(),
        format!(
            "set r to display dialog (item 1 of argv) with title \"keymule\" \
             buttons {{\"Don't Send\", \"Send As Is\", \"Save in keymule\"}} \
             default button \"Save in keymule\" cancel button \"Don't Send\" with icon caution \
             giving up after {TIMEOUT_SECS}"
        ),
        "if gave up of r then error \"keymule-timeout\" number 2".to_string(),
        "return button returned of r".to_string(),
        "end run".to_string(),
    ];
    Ok(match osascript(&script, message)? {
        Answer::Text(t) => t,
        Answer::Cancelled => "Don't Send".into(),
        Answer::TimedOut => String::new(),
    })
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
    let ask = format!(
        "Show the secret for \"{name}\" on screen?\n\nAnyone who can see your screen, or a \
         screen recording, will see it. The window closes after 30 seconds."
    );
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
    platform_show(name, &shown, copy)?;
    Ok(true)
}

#[cfg(target_os = "macos")]
fn platform_ask_reveal(message: &str) -> Result<bool> {
    let script = [
        "on run argv".to_string(),
        format!(
            "set r to display dialog (item 1 of argv) with title \"keymule\" \
             buttons {{\"Cancel\", \"Show\"}} default button \"Cancel\" \
             cancel button \"Cancel\" with icon caution giving up after {TIMEOUT_SECS}"
        ),
        "if gave up of r then error \"keymule-timeout\" number 2".to_string(),
        "return button returned of r".to_string(),
        "end run".to_string(),
    ];
    Ok(osascript(&script, message)? == Answer::Text("Show".into()))
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
fn platform_show(name: &str, shown: &str, copy: Option<&str>) -> Result<()> {
    let title = applescript_str(&format!("keymule · {name}"));
    let mut script = format!(
        "set r to display dialog {} with title {title} ",
        applescript_str(shown)
    );
    match copy {
        Some(v) => script.push_str(&format!(
            "buttons {{\"Copy\", \"Done\"}} default button \"Done\" giving up after 30\n\
             if button returned of r is \"Copy\" then set the clipboard to {}\n",
            applescript_str(v)
        )),
        None => script.push_str("buttons {\"Done\"} default button \"Done\" giving up after 30\n"),
    }
    osascript_stdin(&script, true)?;
    if let Some(v) = copy {
        // Clear the clipboard in 30 seconds if it still holds the value.
        // This runs on after keymule exits.
        let v = applescript_str(v);
        osascript_stdin(
            &format!(
                "delay 30\ntry\nif (the clipboard as text) is {v} then set the clipboard to \"\"\nend try\n"
            ),
            false,
        )?;
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn platform_ask_reveal(_message: &str) -> Result<bool> {
    anyhow::bail!("revealing a secret is macOS-only for now")
}

#[cfg(not(target_os = "macos"))]
fn platform_show(_name: &str, _shown: &str, _copy: Option<&str>) -> Result<()> {
    anyhow::bail!("revealing a secret is macOS-only for now")
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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::applescript_str;

    #[test]
    fn applescript_strings_are_escaped() {
        assert_eq!(applescript_str("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(applescript_str("x\ny"), "\"x\" & linefeed & \"y\"");
    }
}
