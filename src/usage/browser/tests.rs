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
