use super::*;

#[test]
fn logins_are_for_one_exact_origin() {
    assert_eq!(
        origin_for("github.com", "https://github.com/login"),
        "https://github.com"
    );
    // An http URL in the file can't downgrade a real site.
    assert_eq!(
        origin_for("github.com", "http://github.com"),
        "https://github.com"
    );
    assert_eq!(
        origin_for("localhost:3000", "http://localhost:3000/login"),
        "http://localhost:3000"
    );
    assert_eq!(
        origin_of("https://github.com/login?x=1").as_deref(),
        Some("https://github.com")
    );
    assert_ne!(
        origin_of("https://github.com.evil.io/login").as_deref(),
        Some("https://github.com")
    );
    assert_ne!(
        origin_of("http://github.com/login").as_deref(),
        Some("https://github.com")
    );
    assert!(login_host("http://github.com/login").is_err());
    assert_eq!(
        login_host("https://GitHub.com/login").unwrap(),
        "github.com"
    );
    assert_eq!(
        login_host("http://localhost:3000").unwrap(),
        "localhost:3000"
    );
}

#[test]
fn the_browser_tool_is_pointed_at_keymule_chrome() {
    let ws = "ws://127.0.0.1:9/devtools/browser/x";
    let pw = ["npx".to_string(), "@playwright/mcp@latest".into()];
    assert_eq!(endpoint_args(&pw, ws).unwrap(), ["--cdp-endpoint", ws]);
    let cd = [
        "npx".to_string(),
        "-y".into(),
        "chrome-devtools-mcp@latest".into(),
    ];
    assert_eq!(
        endpoint_args(&cd, ws).unwrap(),
        [format!("--wsEndpoint={ws}")]
    );
    assert!(endpoint_args(&["some-other-mcp".to_string()], ws).is_err());
}

#[test]
fn stored_passwords_are_hidden_from_replies() {
    let mut h = Hidden {
        forms: vec![
            "hunter2-secret".into(),
            "hunter2-secret".replace('-', "%2D"),
        ],
        stamp: Hidden::files_stamp(),
    };
    let line = r#"{"result":{"content":[{"text":"value: hunter2-secret"}]}}"#;
    let out = h.scrub(line);
    assert!(!out.contains("hunter2-secret"));
    assert!(out.contains(HIDDEN));
}

#[test]
fn only_claudes_native_hosts_are_copied_into_the_profile() {
    let chrome = tempfile::tempdir().unwrap();
    let edge = tempfile::tempdir().unwrap();
    let profile = tempfile::tempdir().unwrap();
    let claude = r#"{"name":"com.anthropic.claude_code_browser_extension","path":"/a"}"#;
    std::fs::write(
        chrome
            .path()
            .join("com.anthropic.claude_code_browser_extension.json"),
        claude,
    )
    .unwrap();
    std::fs::write(
        chrome.path().join("com.example.password_manager.json"),
        "{}",
    )
    .unwrap();
    // The same host in a later browser's folder doesn't replace the first.
    std::fs::write(
        edge.path()
            .join("com.anthropic.claude_code_browser_extension.json"),
        "{}",
    )
    .unwrap();
    let from = [
        chrome.path().to_path_buf(),
        profile.path().join("missing"),
        edge.path().to_path_buf(),
    ];
    copy_native_hosts(&from, profile.path());
    let hosts = profile.path().join("NativeMessagingHosts");
    let names: Vec<_> = std::fs::read_dir(&hosts)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, ["com.anthropic.claude_code_browser_extension.json"]);
    let copy = hosts.join(&names[0]);
    assert_eq!(std::fs::read_to_string(&copy).unwrap(), claude);
    // A reinstalled Claude Code (new path) is picked up on the next start.
    let moved = r#"{"name":"com.anthropic.claude_code_browser_extension","path":"/b"}"#;
    std::fs::write(
        chrome
            .path()
            .join("com.anthropic.claude_code_browser_extension.json"),
        moved,
    )
    .unwrap();
    copy_native_hosts(&from, profile.path());
    assert_eq!(std::fs::read_to_string(&copy).unwrap(), moved);
}

#[test]
fn chrome_never_offers_to_save_a_password_sign_in_typed() {
    let profile = tempfile::tempdir().unwrap();
    let prefs = profile.path().join("Default").join("Preferences");
    // A new profile.
    no_saved_passwords(profile.path());
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&prefs).unwrap()).unwrap();
    assert_eq!(v["credentials_enable_service"], false);
    assert_eq!(v["profile"]["password_manager_enabled"], false);
    // One Chrome already wrote to: its other settings stay.
    std::fs::write(
        &prefs,
        r#"{"credentials_enable_service":true,"profile":{"name":"x"},"homepage":"h"}"#,
    )
    .unwrap();
    no_saved_passwords(profile.path());
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&prefs).unwrap()).unwrap();
    assert_eq!(v["credentials_enable_service"], false);
    assert_eq!(v["profile"]["password_manager_enabled"], false);
    assert_eq!(v["profile"]["name"], "x");
    assert_eq!(v["homepage"], "h");
    // A file keymule can't read is left alone.
    std::fs::write(&prefs, "not json").unwrap();
    no_saved_passwords(profile.path());
    assert_eq!(std::fs::read_to_string(&prefs).unwrap(), "not json");
}

#[test]
fn browser_takes_a_server_or_open() {
    use clap::Parser;
    let parse = |a: &[&str]| crate::cli::Cli::try_parse_from(a).map(|c| c.cmd);
    assert!(parse(&["keymule", "browser"]).is_err());
    assert!(parse(&["keymule", "browser", "--open", "--", "npx", "x"]).is_err());
    for (args, want) in [
        (&["keymule", "browser", "--open"][..], ""),
        (
            &["keymule", "browser", "--open", "https://github.com/login"][..],
            "https://github.com/login",
        ),
    ] {
        match parse(args).unwrap() {
            crate::cli::Cmd::Browser { open, command } => {
                assert_eq!(open.as_deref(), Some(want));
                assert!(command.is_empty());
            }
            _ => panic!("not browser"),
        }
    }
}
