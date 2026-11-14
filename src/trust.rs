//! Allow all: one yes for a project folder, after which keymule stops
//! asking there. Every approval dialog goes through [`confirm`]; deleting
//! never does (it has its own dialog), so deletes still ask.
//!
//! What "allow all" covers, in that folder and its subfolders:
//! any program `keymule exec` would ask about or refuse (agent-built ones,
//! shells and interpreters too), the read-back guard,
//! `gh-secret`, git installs, MCP server commands and URLs, sending a key
//! to a new host, revealing a secret on screen, and the paste hook saving a
//! secret without holding the message. Typing a new secret into keymule's
//! own dialog is not an approval, so that dialog still appears.
//!
//! The record is sealed in the vault, like exec approvals, so an agent can't
//! write one by editing a file. Turning it on takes the dialog in [`ask`];
//! turning it off never asks.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::store::Vault;

/// The vault item. Credential names can't contain ':', so it can't clash.
const ITEM: &str = "allow:all";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Allowed {
    /// Absolute folder, symlinks resolved.
    pub dir: String,
    pub since: u64,
    /// Unix time it ends; None until turned off.
    pub until: Option<u64>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Record {
    #[serde(default)]
    allowed: Vec<Allowed>,
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn load(vault: &dyn Vault) -> Record {
    vault
        .get(ITEM)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(vault: &dyn Vault, r: &Record) -> Result<()> {
    if r.allowed.is_empty() {
        let _ = vault.delete(ITEM);
        return Ok(());
    }
    vault.set(ITEM, &serde_json::to_string(r)?)
}

/// `dir` with symlinks resolved, also when its last parts don't exist yet.
fn canon(dir: &Path) -> PathBuf {
    if let Ok(c) = dir.canonicalize() {
        return c;
    }
    match (dir.parent(), dir.file_name()) {
        (Some(parent), Some(name)) => canon(parent).join(name),
        _ => dir.to_path_buf(),
    }
}

/// The project a folder belongs to: its git repo's top folder, else itself.
pub fn project_of(dir: &Path) -> PathBuf {
    let dir = canon(dir);
    dir.ancestors()
        .find(|d| d.join(".git").exists())
        .map(Path::to_path_buf)
        .unwrap_or(dir)
}

/// The allow-all that covers `dir` now, if any.
pub fn covering(vault: &dyn Vault, dir: &Path, at: u64) -> Option<Allowed> {
    let dir = canon(dir);
    load(vault)
        .allowed
        .into_iter()
        .find(|a| dir.starts_with(&a.dir) && a.until.is_none_or(|u| at < u))
}

/// Everything turned on and not yet over.
pub fn list(vault: &dyn Vault, at: u64) -> Vec<Allowed> {
    load(vault)
        .allowed
        .into_iter()
        .filter(|a| a.until.is_none_or(|u| at < u))
        .collect()
}

/// Turn it on for `dir` (replacing an older entry for the same folder).
pub fn grant(vault: &dyn Vault, dir: &Path, until: Option<u64>, at: u64) -> Result<Allowed> {
    let dir = canon(dir).to_string_lossy().into_owned();
    let mut r = load(vault);
    r.allowed
        .retain(|a| a.dir != dir && a.until.is_none_or(|u| at < u));
    let a = Allowed {
        dir,
        since: at,
        until,
    };
    r.allowed.push(a.clone());
    save(vault, &r)?;
    Ok(a)
}

/// Turn it off for `dir`, or everywhere. Returns what was on.
pub fn revoke(vault: &dyn Vault, dir: Option<&Path>) -> Result<Vec<Allowed>> {
    let mut r = load(vault);
    let (gone, kept) = match dir {
        Some(d) => {
            let d = canon(d);
            r.allowed
                .into_iter()
                .partition(|a| d.starts_with(&a.dir) || Path::new(&a.dir).starts_with(&d))
        }
        None => (std::mem::take(&mut r.allowed), vec![]),
    };
    r.allowed = kept;
    save(vault, &r)?;
    Ok(gone)
}

/// Whether allow-all covers `dir` (logged when it does).
pub fn skips(vault: &dyn Vault, dir: &Path, what: &str) -> bool {
    match covering(vault, dir, now()) {
        Some(a) => {
            let first = what.lines().next().unwrap_or(what);
            crate::http::audit(
                "-",
                "AUTO-ALLOW",
                &format!("{first} (allow all: {})", a.dir),
                0,
            );
            true
        }
        None => false,
    }
}

/// Ask the user to allow something, unless allow-all covers the folder
/// keymule runs in. Use this for every approval except deletes.
pub fn confirm(message: &str) -> Result<bool> {
    let dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    confirm_in(&dir, message)
}

/// [`confirm`] for a given folder (the paste hook's, an MCP call's).
pub fn confirm_in(dir: &Path, message: &str) -> Result<bool> {
    if skips(crate::store::vault().as_ref(), dir, message) {
        return Ok(true);
    }
    crate::dialog::confirm(message)
}

/// How long the user allowed it for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Span {
    Today,
    UntilOff,
}

/// The dialog that turns it on.
pub fn question(project: &Path) -> String {
    format!(
        "Allow all for {}?\n\n\
         Agents working in this folder can then use every keymule credential without a \
         dialog: with any program (shells and ones they build too), copied into GitHub Actions, \
         sent to new sites, and shown on screen. Deleting still asks.\n\n\
         keymule still keeps secrets out of its own output and logs every use, but an agent \
         that sets out to read a secret will be able to. Turn it off any time: keymule allow --off",
        crate::cliexec::tilde(project)
    )
}

/// Ask, and record the answer. None if the user said no.
pub fn ask(vault: &dyn Vault, project: &Path) -> Result<Option<Allowed>> {
    let span = crate::dialog::allow_all(&question(project))
        .context("keymule needs to ask you, but can't show a dialog here")?;
    let at = now();
    Ok(match span {
        None => None,
        Some(Span::Today) => Some(grant(vault, project, Some(end_of_today(at)), at)?),
        Some(Span::UntilOff) => Some(grant(vault, project, None, at)?),
    })
}

/// Local midnight after `at`, or 24 hours on if the time zone is unknown.
fn end_of_today(at: u64) -> u64 {
    let out = std::process::Command::new("date")
        .args(["+%H %M %S"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    let secs_today = out.and_then(|s| {
        let v: Vec<u64> = s
            .split_whitespace()
            .filter_map(|x| x.parse().ok())
            .collect();
        (v.len() == 3).then(|| v[0] * 3600 + v[1] * 60 + v[2])
    });
    match secs_today {
        Some(s) => at + 86_400 - s.min(86_399),
        None => at + 86_400,
    }
}

/// "until you turn it off" or "until 23:59 today"-style text.
pub fn until_text(a: &Allowed) -> String {
    match a.until {
        None => "until you turn it off".into(),
        Some(u) => {
            let left = u.saturating_sub(now());
            format!(
                "for {}h{:02}m more (ends at midnight)",
                left / 3600,
                left % 3600 / 60
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::testing::MemVault;

    #[test]
    fn covers_the_folder_and_below_until_it_ends() {
        let v = MemVault::default();
        let proj = tempfile::tempdir().unwrap();
        let sub = proj.path().join("web/app");
        std::fs::create_dir_all(&sub).unwrap();
        let other = tempfile::tempdir().unwrap();

        assert!(covering(&v, proj.path(), 100).is_none());
        grant(&v, proj.path(), Some(200), 100).unwrap();
        assert!(covering(&v, proj.path(), 150).is_some());
        assert!(covering(&v, &sub, 150).is_some(), "subfolders count");
        assert!(
            covering(&v, other.path(), 150).is_none(),
            "other folders don't"
        );
        assert!(covering(&v, proj.path(), 200).is_none(), "it ends");

        grant(&v, proj.path(), None, 300).unwrap();
        assert!(covering(&v, proj.path(), u64::MAX - 1).is_some());
        assert_eq!(list(&v, 300).len(), 1, "a new grant replaces the old one");
    }

    #[test]
    fn off_removes_it_for_a_folder_or_everywhere() {
        let v = MemVault::default();
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        grant(&v, a.path(), None, 1).unwrap();
        grant(&v, b.path(), None, 1).unwrap();
        let gone = revoke(&v, Some(&a.path().join("sub"))).unwrap();
        assert_eq!(
            gone.len(),
            1,
            "off from inside the project turns that project off"
        );
        assert!(covering(&v, a.path(), 2).is_none());
        assert!(covering(&v, b.path(), 2).is_some());
        revoke(&v, None).unwrap();
        assert!(list(&v, 2).is_empty());
        assert!(v.get(ITEM).is_err(), "nothing left behind");
    }

    #[test]
    fn the_project_is_the_git_repo() {
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir(repo.path().join(".git")).unwrap();
        let deep = repo.path().join("src/bin");
        std::fs::create_dir_all(&deep).unwrap();
        assert_eq!(project_of(&deep), repo.path().canonicalize().unwrap());
        let plain = tempfile::tempdir().unwrap();
        assert_eq!(
            project_of(plain.path()),
            plain.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn the_question_says_what_it_gives_away() {
        let q = question(Path::new("/work/shop"));
        assert!(q.contains("Allow all for /work/shop?"));
        assert!(q.contains("Deleting still asks"));
        assert!(q.contains("will be able to"));
    }
}
