
use super::*;
use crate::store::testing::MemVault;
use crate::store::{Auth, Credential};
use std::cell::Cell;

const TOKEN: &str = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";

fn setup() -> (tempfile::TempDir, Store, MemVault) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::load_from(dir.path().join("c.toml")).unwrap();
    let cred = Credential {
        service: "github".into(),
        url: "https://api.github.com".into(),
        description: String::new(),
        auth: Auth::Bearer,
        hosts: vec!["api.github.com".into()],
        added_by: "cli".into(),
        ssh: None,
    };
    let vault = MemVault::default();
    Sealed::new(TOKEN, &cred)
        .save(&vault, "github-acme")
        .unwrap();
    store.creds.insert("github-acme".into(), cred);
    (dir, store, vault)
}

fn argv(s: &str) -> Vec<String> {
    s.split_whitespace().map(String::from).collect()
}

#[test]
fn refuses_shells_interpreters_and_env_printers() {
    for p in [
        "sh",
        "bash",
        "zsh",
        "python3",
        "python3.12",
        "node",
        "perl",
        "env",
        "printenv",
        "echo",
        "cat",
    ] {
        assert!(refused(p), "{p}");
    }
    for p in ["gh", "glab", "git", "curl", "terraform"] {
        assert!(!refused(p), "{p}");
    }
    let (dir, store, vault) = setup();
    let never = |_: &str| -> Result<bool> { panic!("no dialog for a refused program") };
    for cmd in [
        "sh -c true",
        "env",
        "printenv GH_TOKEN",
        "/bin/cat /dev/null",
    ] {
        let err = prepare(
            &vault,
            &store,
            "github-acme",
            &[],
            &argv(cmd),
            dir.path(),
            &never,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("won't hand a credential"), "{cmd}: {err}");
    }
}

#[test]
fn a_symlink_to_a_shell_is_still_a_shell() {
    let (dir, store, vault) = setup();
    let link = dir.path().join("innocent-tool");
    std::os::unix::fs::symlink("/bin/sh", &link).unwrap();
    let never = |_: &str| -> Result<bool> { panic!("no dialog") };
    let err = prepare(
        &vault,
        &store,
        "github-acme",
        &[],
        &[link.to_string_lossy().into()],
        dir.path(),
        &never,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("won't hand a credential"), "{err}"); // sh, or dash on Linux
}

#[test]
fn scripts_run_by_a_shell_or_interpreter_are_refused() {
    let (dir, store, vault) = setup();
    let never = |_: &str| -> Result<bool> { panic!("no dialog") };
    for (i, shebang) in [
        "#!/bin/sh",
        "#!/usr/bin/env python3",
        "#!/usr/bin/env -S node --flag",
        "#!/bin/bash -e",
    ]
    .iter()
    .enumerate()
    {
        let p = dir.path().join(format!("tool{i}"));
        std::fs::write(&p, format!("{shebang}\nprintenv GH_TOKEN > /tmp/x\n")).unwrap();
        std::fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let err = prepare(
            &vault,
            &store,
            "github-acme",
            &[],
            &[p.to_string_lossy().into()],
            dir.path(),
            &never,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("it's a script run by"), "{shebang}: {err}");
    }
}

#[test]
fn asks_once_per_program_and_folder_and_sets_the_env_var() {
    let (dir, store, vault) = setup();
    let asked = Cell::new(0);
    let yes = |msg: &str| -> Result<bool> {
        asked.set(asked.get() + 1);
        assert!(msg.contains("github-acme") && msg.contains("GH_TOKEN") && !msg.contains(TOKEN));
        Ok(true)
    };
    let run =
        |args: &str, d: &Path| prepare(&vault, &store, "github-acme", &[], &argv(args), d, &yes);
    let (cmd, secret) = run("true", dir.path()).unwrap();
    assert_eq!(secret, TOKEN);
    assert!(
        cmd.get_envs()
            .any(|(k, v)| k == "GH_TOKEN" && v.is_some_and(|v| v == TOKEN))
    );
    assert!(
        cmd.get_envs()
            .any(|(k, v)| k == "NODE_OPTIONS" && v.is_none())
    );
    run("true --other args", dir.path()).unwrap();
    assert_eq!(asked.get(), 1, "same program, same folder");
    let sub = dir.path().join("web/app");
    std::fs::create_dir_all(&sub).unwrap();
    run("true", &sub).unwrap();
    assert_eq!(asked.get(), 1, "a folder inside it is covered");
    let other = tempfile::tempdir().unwrap();
    run("true", other.path()).unwrap();
    assert_eq!(asked.get(), 2, "a new folder asks again");

    let no = |_: &str| -> Result<bool> { Ok(false) };
    let err = prepare(
        &vault,
        &store,
        "github-acme",
        &[],
        &argv("false"),
        dir.path(),
        &no,
    )
    .unwrap_err();
    assert!(err.to_string().contains("didn't allow"));
}

#[test]
fn allow_all_runs_a_new_program_without_asking_or_remembering_it() {
    let (dir, store, vault) = setup();
    let never = |_: &str| -> Result<bool> { panic!("allow all: no dialog") };
    prepare_in(
        &vault,
        &store,
        "github-acme",
        &[],
        &argv("true"),
        dir.path(),
        &never,
        true,
    )
    .unwrap();
    let sealed = Sealed::load(&vault, "github-acme", store.get("github-acme").unwrap()).unwrap();
    assert!(sealed.exec_grants.is_empty(), "nothing remembered");
    prepare_in(
        &vault,
        &store,
        "github-acme",
        &[],
        &argv("sh -c true"),
        dir.path(),
        &never,
        true,
    )
    .expect("allow all lets shells through too");
    let err = prepare(
        &vault,
        &store,
        "github-acme",
        &[],
        &argv("sh -c true"),
        dir.path(),
        &never,
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("won't hand a credential"),
        "without it, still refused"
    );
}

#[test]
fn needs_a_valid_env_var_when_the_service_has_none() {
    let (dir, mut store, vault) = setup();
    store.creds.get_mut("github-acme").unwrap().service = "generic".into();
    let yes = |_: &str| -> Result<bool> { Ok(true) };
    let err = prepare(
        &vault,
        &store,
        "github-acme",
        &[],
        &argv("true"),
        dir.path(),
        &yes,
    )
    .unwrap_err();
    assert!(err.to_string().contains("--env VAR"), "{err}");
    let err = prepare(
        &vault,
        &store,
        "github-acme",
        &["gh token".into()],
        &argv("true"),
        dir.path(),
        &yes,
    )
    .unwrap_err();
    assert!(err.to_string().contains("isn't an env var"), "{err}");
    assert!(
        prepare(
            &vault,
            &store,
            "github-acme",
            &["ACME_TOKEN".into()],
            &argv("true"),
            dir.path(),
            &yes
        )
        .is_ok()
    );
}

fn kind_store(service: &str, secret: &str) -> (tempfile::TempDir, Store, MemVault) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::load_from(dir.path().join("credentials.toml")).unwrap();
    let vault = MemVault::default();
    let cred = Credential {
        service: service.into(),
        url: String::new(),
        description: String::new(),
        auth: Auth::Bearer,
        hosts: vec![],
        added_by: "test".into(),
        ssh: None,
    };
    Sealed::new(secret, &cred).save(&vault, "it").unwrap();
    store.creds.insert("it".into(), cred);
    (dir, store, vault)
}

fn env_of(cmd: &Command, var: &str) -> Option<String> {
    cmd.get_envs()
        .find(|(k, _)| *k == var)
        .and_then(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
}

#[test]
fn a_private_key_goes_to_the_command_as_a_private_file() {
    let pem = "-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n-----END OPENSSH PRIVATE KEY-----";
    let (dir, store, vault) = kind_store("private-key", pem);
    let yes = |_: &str| -> Result<bool> { Ok(true) };
    let err = prepare_with(
        &vault,
        &store,
        "it",
        &[],
        &[],
        &argv("true"),
        dir.path(),
        &yes,
        false,
    )
    .err()
    .unwrap();
    assert!(err.to_string().contains("--file KEY"), "{err}");

    let p = prepare_with(
        &vault,
        &store,
        "it",
        &[],
        &["KEY".into()],
        &argv("true -i {KEY} host"),
        dir.path(),
        &yes,
        false,
    )
    .unwrap();
    let file = env_of(&p.cmd, "KEY").unwrap();
    let args: Vec<String> = p
        .cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args, ["-i", file.as_str(), "host"]);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), format!("{pem}\n"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    drop(p);
    assert!(!std::path::Path::new(&file).exists(), "removed afterwards");
}

#[test]
fn aws_keys_go_in_their_two_variables() {
    let (id, key) = (
        crate::detect::fake::aws_id(),
        crate::detect::fake::aws_secret(),
    );
    let (dir, store, vault) = kind_store("aws", &format!("{id}\n{key}"));
    let yes = |_: &str| -> Result<bool> { Ok(true) };
    let p = prepare_with(
        &vault,
        &store,
        "it",
        &[],
        &[],
        &argv("true"),
        dir.path(),
        &yes,
        false,
    )
    .unwrap();
    assert_eq!(
        env_of(&p.cmd, "AWS_ACCESS_KEY_ID").as_deref(),
        Some(id.as_str())
    );
    assert_eq!(
        env_of(&p.cmd, "AWS_SECRET_ACCESS_KEY").as_deref(),
        Some(key.as_str())
    );
    assert_eq!(p.scrub, key);
    assert!(
        prepare_with(
            &vault,
            &store,
            "it",
            &["X".into()],
            &[],
            &argv("true"),
            dir.path(),
            &yes,
            false
        )
        .is_err()
    );
}

#[test]
fn a_password_needs_its_env_var_named() {
    let (dir, store, vault) = kind_store("password", "Tr0ub4dor&3x");
    let yes = |_: &str| -> Result<bool> { Ok(true) };
    let err = prepare_with(
        &vault,
        &store,
        "it",
        &[],
        &[],
        &argv("true"),
        dir.path(),
        &yes,
        false,
    )
    .err()
    .unwrap();
    assert!(err.to_string().contains("--env VAR"), "{err}");
    let p = prepare_with(
        &vault,
        &store,
        "it",
        &["PGPASSWORD".into()],
        &[],
        &argv("true"),
        dir.path(),
        &yes,
        false,
    )
    .unwrap();
    assert_eq!(
        env_of(&p.cmd, "PGPASSWORD").as_deref(),
        Some("Tr0ub4dor&3x")
    );
}
