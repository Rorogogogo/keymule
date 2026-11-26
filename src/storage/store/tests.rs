
use super::*;

#[test]
fn host_of_keeps_nondefault_port() {
    assert_eq!(
        host_of("https://Bamboo.Corp.com/bamboo").unwrap(),
        "bamboo.corp.com"
    );
    assert_eq!(host_of("http://localhost:8085").unwrap(), "localhost:8085");
    assert!(host_of("file:///etc/passwd").is_err());
}

#[test]
fn rename_moves_the_secret_and_the_entry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("credentials.toml");
    let vault = testing::MemVault::default();
    let mut store = Store::load_from(path.clone()).unwrap();
    let cred = Credential {
        service: "github".into(),
        url: "https://api.github.com".into(),
        description: "GitHub token".into(),
        auth: Auth::Bearer,
        hosts: vec!["api.github.com".into()],
        added_by: "test".into(),
        ssh: None,
    };
    store.creds.insert("github".into(), cred.clone());
    store.creds.insert("github-work".into(), cred.clone());
    Sealed::new("tok", &cred).save(&vault, "github").unwrap();

    assert!(
        store.rename(&vault, "github", "github-work", None).is_err(),
        "taken"
    );
    assert!(store.rename(&vault, "github", "bad name", None).is_err());
    store
        .rename(&vault, "github", "github-tap", Some("for the tap".into()))
        .unwrap();
    assert!(vault.get("github").is_err());
    assert_eq!(
        Sealed::peek_secret(&vault, "github-tap").as_deref(),
        Some("tok")
    );
    let reloaded = Store::load_from(path).unwrap();
    assert!(!reloaded.creds.contains_key("github"));
    assert_eq!(reloaded.creds["github-tap"].description, "for the tap");
}

#[test]
fn roundtrip_and_free_name() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("credentials.toml");
    let mut store = Store::load_from(path.clone()).unwrap();
    store.creds.insert(
        "bamboo".into(),
        Credential {
            service: "bamboo".into(),
            url: "https://bamboo.corp.com".into(),
            description: String::new(),
            auth: Auth::Basic {
                username: "roro".into(),
            },
            hosts: vec!["bamboo.corp.com".into()],
            added_by: "cli".into(),
            ssh: None,
        },
    );
    store.save().unwrap();
    let again = Store::load_from(path).unwrap();
    assert_eq!(again.creds, store.creds);
    assert_eq!(again.free_name("bamboo"), "bamboo-2");
    assert_eq!(again.free_name("github"), "github");
}

#[test]
fn sealed_rules_beat_an_edited_file() {
    let vault = testing::MemVault::default();
    let mut cred = Credential {
        service: "bamboo".into(),
        url: "https://bamboo.corp".into(),
        description: String::new(),
        auth: Auth::Bearer,
        hosts: vec!["bamboo.corp".into()],
        added_by: "cli".into(),
        ssh: None,
    };
    // An item from before sealing is upgraded on first load.
    vault.set("b", "tok-123").unwrap();
    let sealed = Sealed::load(&vault, "b", &cred).unwrap();
    assert_eq!(sealed.secret, "tok-123");
    assert!(vault.get("b").unwrap().starts_with(SEAL_TAG));
    assert_eq!(Sealed::peek_secret(&vault, "b").unwrap(), "tok-123");

    // Someone adds a host to credentials.toml: the seal still wins.
    cred.hosts.push("evil.example".into());
    cred.auth = Auth::Header {
        name: "X-Echo".into(),
    };
    let sealed = Sealed::load(&vault, "b", &cred).unwrap();
    assert!(sealed.differs_from(&cred));
    let used = sealed.apply(&cred);
    assert_eq!(used.hosts, ["bamboo.corp"]);
    assert_eq!(used.auth, Auth::Bearer);
}

#[test]
fn names_are_validated() {
    assert!(validate_name("github-actions_2").is_ok());
    assert!(validate_name("bad name").is_err());
    assert!(validate_name("").is_err());
}
