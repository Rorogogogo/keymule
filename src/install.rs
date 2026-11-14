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

use std::path::Path;

use crate::cliexec::tilde;
use crate::store::{ExecGrant, Install, Sealed, Store, Vault};

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

/// Where `install` stores the copy: git's configured helpers in real use,
/// a stand-in in tests.
pub trait GitStore {
    fn helpers(&self) -> Result<Vec<String>>;
    fn approve(&self, i: &Install, secret: &str) -> Result<()>;
}

pub struct RealGit;

impl GitStore for RealGit {
    fn helpers(&self) -> Result<Vec<String>> {
        helpers()
    }
    fn approve(&self, i: &Install, secret: &str) -> Result<()> {
        approve(i, secret)
    }
}

/// Copy `name` into git's store for https://<host> as <username>. With
/// `cli` (the service's CLI in the repo folder, e.g. gh), the same dialog
/// also lets that CLI use it there, so setting up a repo is one Allow. Asks
/// nothing when both are already in place. Returns what to tell the agent.
#[allow(clippy::too_many_arguments)]
pub fn install(
    vault: &dyn Vault,
    store: &Store,
    name: &str,
    host: &str,
    username: &str,
    cli: Option<(String, ExecGrant)>,
    confirm: &dyn Fn(&str) -> Result<bool>,
    git: &dyn GitStore,
) -> Result<String> {
    let target = Install {
        host: host.trim().to_ascii_lowercase(),
        username: username.trim().to_string(),
    };
    valid(&target)?;
    let mut sealed = Sealed::load(vault, name, store.get(name)?)?;
    let helpers = git.helpers()?;
    if helpers.is_empty() {
        bail!(
            "git has no credential helper, so there's nowhere to keep it. Have the user set one \
             (on macOS: git config --global credential.helper osxkeychain), then try again."
        );
    }
    let new_copy = !sealed.installs.contains(&target);
    let new_cli = cli.filter(|(_, g)| {
        !sealed
            .exec_grants
            .iter()
            .any(|x| x.covers(&g.program, Path::new(&g.dir)))
    });
    let Install { host, username } = &target;
    if new_copy || new_cli.is_some() {
        let cli_line = new_cli
            .as_ref()
            .map(|(tool, g)| format!("{tool} may use it in {} too.", tilde(Path::new(&g.dir))));
        let question = if new_copy {
            let off_host = if sealed.hosts.contains(host) {
                String::new()
            } else {
                format!(
                    "\n\nNote: \"{name}\" was saved for {}.",
                    sealed.hosts.join(", ")
                )
            };
            format!(
                "Let git use \"{name}\" for {host} as {username}?\n\n\
                 keymule copies it into git's credential store ({}) and keeps the copy in step: \
                 rotating updates it, deleting removes it.{}{off_host}\n\n\
                 Other programs you run could then ask git for it. keymule stops agents from \
                 doing that, as far as it can.",
                helpers.join(", "),
                cli_line.map(|l| format!(" {l}")).unwrap_or_default(),
            )
        } else {
            let (tool, g) = new_cli.as_ref().expect("checked");
            format!(
                "Let {tool} use \"{name}\" in {}?\n\nGit already has it for {host} as \
                 {username}. keymule hides the token in {tool}'s output.",
                tilde(Path::new(&g.dir))
            )
        };
        let allowed = confirm(&question)
            .context("keymule needs to ask you first, but can't show a dialog here")?;
        if !allowed {
            bail!("the user didn't allow it; nothing changed");
        }
    }
    // Write it even when it's there already: the store may have lost it.
    git.approve(&target, &sealed.secret)?;
    if new_copy {
        sealed.installs.push(target.clone());
    }
    let cli_note = match &new_cli {
        Some((tool, g)) => {
            let note = format!(
                " {tool} may use it in {} too: keymule exec {name} -- {tool} ...",
                tilde(Path::new(&g.dir))
            );
            sealed.exec_grants.push(g.clone());
            note
        }
        None => format!(" For gh: keymule exec {name} -- gh ..."),
    };
    sealed.save(vault, name)?;
    crate::http::audit(
        name,
        "INSTALL",
        &format!("git https://{username}@{host}"),
        0,
    );
    Ok(format!(
        "Git can use \"{name}\" for https://{host} as {username}.{cli_note}\n\n\
         Point the repo's remote at that account:\n\n\
         \x20   git remote set-url origin https://{username}@{host}/<owner>/<repo>.git\n\n\
         Repos without a user in their URL could pick either account, so the user may want to \
         name their usual one once (their call; keymule doesn't change git config):\n\n\
         \x20   git config --global credential.https://{host}.username <their usual account>\n\n\
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

    use crate::store::testing::MemVault;
    use crate::store::{Auth, Credential};
    use std::cell::RefCell;

    #[derive(Default)]
    struct FakeGit {
        stored: RefCell<Vec<(Install, String)>>,
    }

    impl GitStore for FakeGit {
        fn helpers(&self) -> Result<Vec<String>> {
            Ok(vec!["osxkeychain".into()])
        }
        fn approve(&self, i: &Install, secret: &str) -> Result<()> {
            self.stored.borrow_mut().push((i.clone(), secret.into()));
            Ok(())
        }
    }

    fn setup() -> (tempfile::TempDir, Store, MemVault) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(dir.path().join("c.toml")).unwrap();
        let cred = Credential {
            service: "github".into(),
            url: "https://api.github.com".into(),
            description: String::new(),
            auth: Auth::Bearer,
            hosts: vec!["api.github.com".into(), "github.com".into()],
            added_by: "cli".into(),
        };
        let vault = MemVault::default();
        Sealed::new("tok_acme_123456", &cred)
            .save(&vault, "github-acme")
            .unwrap();
        store.creds.insert("github-acme".into(), cred);
        (dir, store, vault)
    }

    #[test]
    fn one_dialog_covers_git_and_gh_and_a_repeat_asks_nothing() {
        let (dir, store, vault) = setup();
        let git = FakeGit::default();
        let asked = RefCell::new(Vec::<String>::new());
        let yes = |m: &str| -> Result<bool> {
            asked.borrow_mut().push(m.to_string());
            Ok(true)
        };
        let gh = || {
            Some((
                "gh".to_string(),
                ExecGrant {
                    program: "/opt/homebrew/bin/gh".into(),
                    dir: dir.path().to_string_lossy().into(),
                },
            ))
        };
        let msg = install(
            &vault,
            &store,
            "github-acme",
            "github.com",
            "acme-bot",
            gh(),
            &yes,
            &git,
        )
        .unwrap();
        assert_eq!(asked.borrow().len(), 1);
        let q = asked.borrow()[0].clone();
        assert!(
            q.starts_with("Let git use \"github-acme\" for github.com as acme-bot?"),
            "{q}"
        );
        assert!(
            q.contains("gh may use it in") && !q.contains("tok_acme"),
            "{q}"
        );
        assert!(
            msg.contains("https://acme-bot@github.com/") && !msg.contains("tok_acme"),
            "{msg}"
        );
        let sealed = Sealed::load(&vault, "github-acme", &store.creds["github-acme"]).unwrap();
        assert_eq!(sealed.installs.len(), 1);
        assert!(sealed.exec_grants[0].covers("/opt/homebrew/bin/gh", &dir.path().join("web")));

        // Again: nothing new, so no dialog, but git gets the value again.
        install(
            &vault,
            &store,
            "github-acme",
            "github.com",
            "acme-bot",
            gh(),
            &yes,
            &git,
        )
        .unwrap();
        assert_eq!(asked.borrow().len(), 1);
        assert_eq!(git.stored.borrow().len(), 2);
        assert_eq!(git.stored.borrow()[1].1, "tok_acme_123456");
    }

    #[test]
    fn a_no_changes_nothing() {
        let (_dir, store, vault) = setup();
        let git = FakeGit::default();
        let no = |_: &str| -> Result<bool> { Ok(false) };
        assert!(
            install(
                &vault,
                &store,
                "github-acme",
                "github.com",
                "acme-bot",
                None,
                &no,
                &git
            )
            .is_err()
        );
        assert!(git.stored.borrow().is_empty());
        let sealed = Sealed::load(&vault, "github-acme", &store.creds["github-acme"]).unwrap();
        assert!(sealed.installs.is_empty());
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
