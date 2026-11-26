
use super::*;
use crate::store::ServerMeta;
use crate::store::testing::MemVault;

fn ssh_store(auth: SshAuth, secret: &str) -> (tempfile::TempDir, Store, MemVault) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::load_from(dir.path().join("credentials.toml")).unwrap();
    let vault = MemVault::default();
    let cred = Credential {
        service: "ssh".into(),
        url: String::new(),
        description: String::new(),
        auth: Auth::Bearer,
        hosts: vec![],
        added_by: "test".into(),
        ssh: Some(SshTarget::new("203.0.113.7", Some(2222), "root", auth).unwrap()),
    };
    Sealed::new(secret, &cred).save(&vault, "vps").unwrap();
    store.creds.insert("vps".into(), cred);
    (dir, store, vault)
}

fn args_of(cmd: &std::process::Command) -> Vec<String> {
    cmd.get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

fn env_of(cmd: &std::process::Command, var: &str) -> Option<String> {
    cmd.get_envs()
        .find(|(k, _)| *k == var)
        .and_then(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
}

#[test]
fn targets_are_checked() {
    assert!(SshTarget::new("203.0.113.7", None, "root", SshAuth::Password).is_ok());
    assert!(SshTarget::new("[2001:db8::1]", None, "root", SshAuth::Password).is_ok());
    assert!(SshTarget::new("-oProxyCommand=x", None, "root", SshAuth::Password).is_err());
    assert!(SshTarget::new("host x", None, "root", SshAuth::Password).is_err());
    assert!(SshTarget::new("h", None, "-l", SshAuth::Password).is_err());
    assert!(SshTarget::new("h", Some(0), "root", SshAuth::Password).is_err());
    let t = SshTarget::new("VPS.example.com", Some(2222), "deploy", SshAuth::Key).unwrap();
    assert_eq!(t.label(), "deploy@vps.example.com:2222");
}

#[test]
fn a_password_login_runs_ssh_with_keymule_as_askpass() {
    let (dir, store, vault) = ssh_store(SshAuth::Password, "hunter2 with spaces");
    let asked = std::cell::Cell::new(0);
    let yes = |m: &str| -> Result<bool> {
        asked.set(asked.get() + 1);
        assert!(
            m.contains("root@203.0.113.7:2222") && m.contains("uptime"),
            "{m}"
        );
        assert!(!m.contains("hunter2"));
        Ok(true)
    };
    let cmd = ["uptime".to_string()];
    let exe = Path::new("/opt/keymule");
    let p = prepare(&vault, &store, "vps", &cmd, dir.path(), &yes, false, exe).unwrap();
    let args = args_of(&p.cmd);
    let tail = &args[args.len() - 3..];
    assert_eq!(tail, ["--", "203.0.113.7", "uptime"]);
    assert!(args.windows(2).any(|w| w == ["-p", "2222"]));
    assert!(args.windows(2).any(|w| w == ["-l", "root"]));
    assert_eq!(
        env_of(&p.cmd, "SSH_ASKPASS").as_deref(),
        Some("/opt/keymule")
    );
    assert_eq!(
        env_of(&p.cmd, "SSH_ASKPASS_REQUIRE").as_deref(),
        Some("force")
    );
    let file = env_of(&p.cmd, ASKPASS_ENV).unwrap();
    assert!(
        !args.iter().any(|a| a.contains("hunter2")),
        "never on the command line"
    );
    // The helper answers a password prompt once, and nothing else.
    assert_eq!(
        askpass_answer(Path::new(&file), "Are you sure (yes/no)?"),
        None
    );
    assert_eq!(
        askpass_answer(Path::new(&file), "root@203.0.113.7's password: ").as_deref(),
        Some("hunter2 with spaces")
    );
    assert_eq!(
        askpass_answer(Path::new(&file), "Password:"),
        None,
        "only once"
    );
    drop(p);
    // Same folder: no second question.
    prepare(&vault, &store, "vps", &cmd, dir.path(), &yes, false, exe).unwrap();
    assert_eq!(asked.get(), 1);
}

#[test]
fn askpass_only_reads_its_own_files() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("askpass");
    std::fs::write(&f, "x").unwrap();
    assert_eq!(askpass_answer(&f, "Password:"), None);
    assert!(f.exists());
}

#[test]
fn a_key_login_hands_ssh_a_private_file() {
    let pem = "-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n-----END OPENSSH PRIVATE KEY-----";
    let (dir, store, vault) = ssh_store(SshAuth::Key, pem);
    let yes = |_: &str| -> Result<bool> { Ok(true) };
    let p = prepare(
        &vault,
        &store,
        "vps",
        &[],
        dir.path(),
        &yes,
        false,
        Path::new("/x"),
    )
    .unwrap();
    let args = args_of(&p.cmd);
    let i = args.iter().position(|a| a == "-i").unwrap();
    let key = args[i + 1].clone();
    assert_eq!(std::fs::read_to_string(&key).unwrap(), format!("{pem}\n"));
    assert_eq!(env_of(&p.cmd, "SSH_ASKPASS"), None);
    assert_eq!(args.last().unwrap(), "203.0.113.7");
    drop(p);
    assert!(!Path::new(&key).exists());
}

#[test]
fn a_key_file_is_used_where_it_is() {
    let (dir, mut store, vault) = ssh_store(SshAuth::Password, "console-pw");
    let key = dir.path().join("id_test");
    std::fs::write(
        &key,
        "-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n-----END OPENSSH PRIVATE KEY-----\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("id_test.pub"), "ssh-ed25519 AAAA test").unwrap();
    std::fs::write(dir.path().join("notes.txt"), "hello").unwrap();
    let pubkey = dir.path().join("id_test.pub");
    let notes = dir.path().join("notes.txt");
    let t = SshTarget::new("203.0.113.7", Some(2222), "root", SshAuth::Password).unwrap();
    assert!(
        t.clone().with_key_file(pubkey.to_str().unwrap()).is_err(),
        "a .pub"
    );
    assert!(
        t.clone().with_key_file(notes.to_str().unwrap()).is_err(),
        "not a key"
    );
    assert!(t.clone().with_key_file("relative/id").is_err());
    let t = t.with_key_file(key.to_str().unwrap()).unwrap();
    assert_eq!(t.auth, SshAuth::KeyFile);
    // Sealed with the password it already had.
    let mut sealed = Sealed::load(&vault, "vps", store.get("vps").unwrap()).unwrap();
    sealed.ssh = Some(t.clone());
    sealed.save(&vault, "vps").unwrap();
    store.creds.get_mut("vps").unwrap().ssh = Some(t);
    let yes = |_: &str| -> Result<bool> { Ok(true) };
    let p = prepare(
        &vault,
        &store,
        "vps",
        &[],
        dir.path(),
        &yes,
        false,
        Path::new("/x"),
    )
    .unwrap();
    let args = args_of(&p.cmd);
    let i = args.iter().position(|a| a == "-i").unwrap();
    assert_eq!(
        Path::new(&args[i + 1]),
        key.canonicalize().unwrap(),
        "the file itself, not a copy"
    );
    assert_eq!(env_of(&p.cmd, "SSH_ASKPASS"), None);
    assert_eq!(p.scrub, "console-pw", "the kept password is still scrubbed");
    drop(p);
    assert!(key.exists(), "never deleted");
    std::fs::remove_file(&key).unwrap();
    let err = prepare(
        &vault,
        &store,
        "vps",
        &[],
        dir.path(),
        &yes,
        false,
        Path::new("/x"),
    )
    .err()
    .unwrap();
    assert!(err.to_string().contains("can't find a key file"), "{err}");
}

#[test]
fn an_edited_file_cant_move_the_login() {
    let (dir, mut store, vault) = ssh_store(SshAuth::Password, "pw");
    store.creds.get_mut("vps").unwrap().ssh =
        Some(SshTarget::new("evil.example", None, "root", SshAuth::Password).unwrap());
    let yes = |m: &str| -> Result<bool> {
        assert!(!m.contains("evil"), "{m}");
        Ok(true)
    };
    let p = prepare(
        &vault,
        &store,
        "vps",
        &[],
        dir.path(),
        &yes,
        false,
        Path::new("/x"),
    )
    .unwrap();
    assert_eq!(args_of(&p.cmd).last().unwrap(), "203.0.113.7");
}

#[test]
fn refused_without_a_yes_and_for_other_kinds() {
    let (dir, mut store, vault) = ssh_store(SshAuth::Password, "pw");
    let no = |_: &str| -> Result<bool> { Ok(false) };
    let err = prepare(
        &vault,
        &store,
        "vps",
        &[],
        dir.path(),
        &no,
        false,
        Path::new("/x"),
    )
    .err()
    .unwrap();
    assert!(err.to_string().contains("didn't allow"), "{err}");
    store.creds.get_mut("vps").unwrap().service = "password".into();
    let yes = |_: &str| -> Result<bool> { Ok(true) };
    let err = prepare(
        &vault,
        &store,
        "vps",
        &[],
        dir.path(),
        &yes,
        false,
        Path::new("/x"),
    )
    .err()
    .unwrap();
    assert!(err.to_string().contains("isn't an SSH login"), "{err}");
}

fn server_store(value: &str, used: bool) -> (tempfile::TempDir, Store, MemVault) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::load_from(dir.path().join("credentials.toml")).unwrap();
    let vault = MemVault::default();
    let mut b = Binding::default();
    b.env.insert("SSHPASS".into(), value.into());
    b.save(&vault, "vultr-ssh").unwrap();
    store.servers.insert(
        "vultr-ssh".into(),
        ServerMeta {
            env: vec!["SSHPASS".into()],
            used_by: if used {
                vec!["~/.claude.json · x".into()]
            } else {
                vec![]
            },
            description: "Vultr VPS".into(),
            ..Default::default()
        },
    );
    store.save().unwrap();
    (dir, store, vault)
}

#[test]
fn convert_moves_the_value_without_showing_it() {
    let (dir, mut store, vault) = server_store("s3cret-pw", false);
    let yes = |m: &str| -> Result<bool> {
        assert!(
            m.contains("root@203.0.113.7") && !m.contains("s3cret"),
            "{m}"
        );
        Ok(true)
    };
    let to = ConvertTo::parse("ssh", Some("203.0.113.7"), None, Some("root")).unwrap();
    let msg = convert(&vault, &mut store, "vultr-ssh", to, None, &yes).unwrap();
    assert!(
        !msg.contains("s3cret") && msg.contains("keymule ssh vultr-ssh"),
        "{msg}"
    );
    assert!(
        Binding::load(&vault, "vultr-ssh").is_err(),
        "server item gone"
    );
    let again = Store::load_from(dir.path().join("credentials.toml")).unwrap();
    assert!(!again.servers.contains_key("vultr-ssh"));
    let cred = again.get("vultr-ssh").unwrap();
    assert_eq!(cred.service, "ssh");
    assert_eq!(cred.description, "Vultr VPS", "description kept");
    let sealed = Sealed::load(&vault, "vultr-ssh", cred).unwrap();
    assert_eq!(sealed.secret, "s3cret-pw");
    assert_eq!(sealed.ssh.unwrap().auth, SshAuth::Password);
}

#[test]
fn convert_needs_a_yes_and_an_unused_server() {
    let (_d, mut store, vault) = server_store("pw", false);
    let no = |_: &str| -> Result<bool> { Ok(false) };
    let to = ConvertTo::parse("ssh", Some("h"), None, Some("root")).unwrap();
    assert!(convert(&vault, &mut store, "vultr-ssh", to, None, &no).is_err());
    assert!(store.servers.contains_key("vultr-ssh"));
    assert!(Binding::load(&vault, "vultr-ssh").is_ok(), "nothing moved");

    let (_d, mut store, vault) = server_store("pw", true);
    let never = |_: &str| -> Result<bool> { panic!("no dialog") };
    let err = convert(
        &vault,
        &mut store,
        "vultr-ssh",
        ConvertTo::Password,
        None,
        &never,
    )
    .err()
    .unwrap();
    assert!(err.to_string().contains("still run by"), "{err}");
    assert!(ConvertTo::parse("ssh", None, None, Some("root")).is_err());
    assert!(ConvertTo::parse("http", None, None, None).is_err());
}
