//! Questions that must be answered by a human at the keyboard, not by an
//! agent driving a shell. Everything goes through /dev/tty, so piping
//! "y" into stdin does not count as an answer.

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};

use anyhow::{Context, Result, bail};

fn open_tty() -> Result<std::fs::File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .context("this step needs you at a terminal (no TTY available, e.g. when an agent runs it)")
}

pub fn ask(question: &str) -> Result<String> {
    let mut tty = open_tty()?;
    write!(tty, "{question}")?;
    tty.flush()?;
    let mut line = String::new();
    BufReader::new(tty).read_line(&mut line)?;
    Ok(line.trim().to_string())
}

pub fn ask_default(question: &str, default: &str) -> Result<String> {
    let answer = ask(&format!("{question} [{default}]: "))?;
    Ok(if answer.is_empty() {
        default.to_string()
    } else {
        answer
    })
}

pub fn confirm(question: &str) -> Result<()> {
    let answer = ask(&format!("{question} [y/N]: "))?;
    if !matches!(answer.to_ascii_lowercase().as_str(), "y" | "yes") {
        bail!("cancelled");
    }
    Ok(())
}

pub fn secret(prompt: &str) -> Result<String> {
    let value = rpassword::prompt_password(prompt).context("reading secret from the terminal")?;
    let value = value.trim().to_string();
    if value.is_empty() {
        bail!("no secret entered");
    }
    Ok(value)
}

/// A hidden answer, or None if the user just pressed Enter.
pub fn secret_or_keep(prompt: &str) -> Result<Option<String>> {
    let value = rpassword::prompt_password(prompt).context("reading secret from the terminal")?;
    let value = value.trim().to_string();
    Ok((!value.is_empty()).then_some(value))
}
