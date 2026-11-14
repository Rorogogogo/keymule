//! `keymule install <name> --host H --username U`: copy a credential into
//! git's own credential store (osxkeychain, Git Credential Manager…), so
//! `git push` works as that account with nothing to paste. keymule runs
//! `git credential approve` itself and writes the secret on its stdin, never
//! in an argument or an output.
//!
//! keymule remembers each copy (sealed with the secret) and keeps it in
//! step: `rotate` writes the new value, `rm` and `uninstall` erase it.
//!
//! Once git's store holds it, any program running as the user can ask git
//! for it (`git credential fill`). keymule's agent hook blocks the usual
//! ways to do that; the approval dialog says it's best effort.

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::store::{Install, Sealed, Store, Vault};

fn valid(i: &Install) -> Result<()> {
    let host_ok = !i.host.is_empty()
        && i.host
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || ".-:".contains(c));
    if !host_ok {
        bail!("`{}` isn't a host name (like github.com)", i.host);
    }
    if i.username.is_empty() || i.username.chars().any(char::is_control) {
        bail!(
            "give the account's username (as in https://<username>@{})",
            i.host
        );
    }
    Ok(())
}

/// What `git credential` reads on stdin.
pub fn git_input(i: &Install, secret: Option<&str>) -> String {
    let mut s = format!("protocol=https\nhost={}\nusername={}\n", i.host, i.username);
    if let Some(secret) = secret {
        s.push_str(&format!("password={secret}\n"));
    }
    s.push('\n');
    s
}

fn git() -> Result<std::path::PathBuf> {
    crate::servers::which("git").context("git isn't installed (or not on PATH)")
}

/// The credential helpers git is set up with.
pub fn helpers() -> Result<Vec<String>> {
    let out = Command::new(git()?)
        .args(["config", "--get-all", "credential.helper"])
        .output()
        .context("running git config")?;
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect())
}

/// `git credential approve|reject` with `input` on stdin.
fn git_credential(action: &str, input: &str, secret: Option<&str>) -> Result<()> {
    let mut child = Command::new(git()?)
        .args(["credential", action])
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("running git credential")?;
    child
        .stdin
        .take()
        .expect("piped")
        .write_all(input.as_bytes())?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = secret.map_or(err.to_string(), |s| crate::redact::scrub(&err, s));
        bail!("git credential {action} failed: {}", err.trim());
    }
    Ok(())
}

fn approve(i: &Install, secret: &str) -> Result<()> {
    git_credential("approve", &git_input(i, Some(secret)), Some(secret))
}

fn reject(i: &Install) -> Result<()> {
    git_credential("reject", &git_input(i, None), None)
}

/// Copy `name` into git's store for https://<host> as <username>, after the
/// user agrees in `confirm`. Returns what to tell the agent.
pub fn install(
    vault: &dyn Vault,
    store: &Store,
    name: &str,
    host: &str,
    username: &str,
    confirm: &dyn Fn(&str) -> Result<bool>,
) -> Result<String> {
    let target = Install {
        host: host.trim().to_ascii_lowercase(),
        username: username.trim().to_string(),
    };
    valid(&target)?;
    let mut sealed = Sealed::load(vault, name, store.get(name)?)?;
    let helpers = helpers()?;
    if helpers.is_empty() {
        bail!(
            "git has no credential helper, so there's nowhere to keep it. Have the user set one \
             (on macOS: git config --global credential.helper osxkeychain), then try again."
        );
    }
    let off_host = if sealed.hosts.contains(&target.host) {
        String::new()
    } else {
        format!(
            "Note: \"{name}\" is set up for {}, not {}.\n\n",
            if sealed.hosts.is_empty() {
                "no host yet".into()
            } else {
                sealed.hosts.join(", ")
            },
            target.host
        )
    };
    let allowed = confirm(&format!(
        "An agent wants to copy your keymule credential \"{name}\" into git's credential store \
         ({}), for:\n\n        https://{}  as  {}\n\n{off_host}\
         Git will then use it for that account without asking. keymule blocks agents from \
         reading it back, but that's best effort once it's in git's store: any program you run \
         could ask git for it.\n\nkeymule keeps the copy in step: rotating updates it, \
         deleting removes it.",
        helpers.join(", "),
        target.host,
        target.username,
    ))
    .context("keymule needs to ask you first, but can't show a dialog here")?;
    if !allowed {
        bail!("the user didn't allow copying \"{name}\" into git's store; nothing changed");
    }
    approve(&target, &sealed.secret)?;
    if !sealed.installs.contains(&target) {
        sealed.installs.push(target.clone());
        sealed.save(vault, name)?;
    }
    crate::http::audit(
        name,
        "INSTALL",
        &format!("git https://{}@{}", target.username, target.host),
        0,
    );
    let Install { host, username } = &target;
    Ok(format!(
        "Copied \"{name}\" into git's credential store for https://{host} as {username}. \
         Git uses it for remotes whose URL names that user, so point the repo at it:\n\n\
         \x20   git remote set-url origin https://{username}@{host}/<owner>/<repo>.git\n\n\
         So repos without a user in their URL don't pick this account by chance, the user may \
         want to name their default once (their call; keymule doesn't change git config):\n\n\
         \x20   git config --global credential.https://{host}.username <their usual account>\n\n\
         For gh, run it through keymule: keymule exec {name} --env GH_TOKEN -- gh …\n\
         Don't read the token back (git credential fill, gh auth token): keymule blocks that."
    ))
}

/// Erase `name`'s copies from git's store (those matching `host` and
/// `username`, or all). Returns what was removed.
pub fn uninstall(
    vault: &dyn Vault,
    store: &Store,
    name: &str,
    host: Option<&str>,
    username: Option<&str>,
) -> Result<Vec<Install>> {
    let mut sealed = Sealed::load(vault, name, store.get(name)?)?;
    let (gone, kept): (Vec<Install>, Vec<Install>) = sealed.installs.drain(..).partition(|i| {
        host.is_none_or(|h| h.eq_ignore_ascii_case(&i.host))
            && username.is_none_or(|u| u == i.username)
    });
    for i in &gone {
        reject(i)?;
        crate::http::audit(
            name,
            "UNINSTALL",
            &format!("git https://{}@{}", i.username, i.host),
            0,
        );
    }
    sealed.installs = kept;
    sealed.save(vault, name)?;
    Ok(gone)
}

/// After a rotation: write the new value into each copy. Returns a note
/// per copy that couldn't be updated.
pub fn resync(sealed: &Sealed) -> Vec<String> {
    sealed
        .installs
        .iter()
        .filter_map(|i| {
            let r = reject(i).and_then(|()| approve(i, &sealed.secret));
            r.err()
                .map(|e| format!("couldn't update {} in git's store: {e:#}", i.label()))
        })
        .collect()
}

/// Before deleting: erase every copy. Returns a note per copy that stayed.
pub fn erase_all(sealed: &Sealed) -> Vec<String> {
    sealed
        .installs
        .iter()
        .filter_map(|i| {
            reject(i)
                .err()
                .map(|e| format!("couldn't remove {} from git's store: {e:#}", i.label()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_input_carries_the_secret_only_when_storing() {
        let i = Install {
            host: "github.com".into(),
            username: "acme-bot".into(),
        };
        assert_eq!(
            git_input(&i, Some("tok_123")),
            "protocol=https\nhost=github.com\nusername=acme-bot\npassword=tok_123\n\n"
        );
        assert!(!git_input(&i, None).contains("password"));
    }

    #[test]
    fn refuses_odd_hosts_and_usernames() {
        let bad = |host: &str, username: &str| {
            valid(&Install {
                host: host.into(),
                username: username.into(),
            })
            .is_err()
        };
        assert!(bad("github.com\nhost=evil.com", "me"));
        assert!(bad("https://github.com", "me"));
        assert!(bad("github.com", "me\npassword=x"));
        assert!(bad("github.com", ""));
        assert!(!bad("github.com", "acme-bot"));
        assert!(!bad("git.corp.com:8443", "acme.bot"));
    }
}
