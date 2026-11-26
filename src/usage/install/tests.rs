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
        ssh: None,
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
        q.starts_with("Let git use \"github-acme\" for github.com?")
            && q.contains("Signs in as acme-bot."),
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
