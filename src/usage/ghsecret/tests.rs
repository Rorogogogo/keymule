use super::*;

#[test]
fn sealed_value_opens_with_the_repo_key_only() {
    let sk = crypto_box::SecretKey::generate(&mut crypto_box::aead::OsRng);
    let pk = B64.encode(sk.public_key().as_bytes());
    let ct = seal(&pk, "sk_live_example_value").unwrap();
    let opened = sk.unseal(&B64.decode(ct).unwrap()).unwrap();
    assert_eq!(opened, b"sk_live_example_value");
    let other = crypto_box::SecretKey::generate(&mut crypto_box::aead::OsRng);
    assert!(
        other
            .unseal(&B64.decode(seal(&pk, "x").unwrap()).unwrap())
            .is_err()
    );
}

#[test]
fn rejects_bad_keys_and_names() {
    assert!(seal("not base64!", "x").is_err());
    assert!(seal(&B64.encode([0u8; 16]), "x").is_err());
    assert!(validate_repo("acme/app").is_ok() && validate_secret("STRIPE_SECRET_KEY").is_ok());
    assert!(validate_repo("acme").is_err());
    assert!(validate_secret("1BAD").is_err());
    assert!(validate_secret("github_token").is_err());
    assert!(validate_secret("has-dash").is_err());
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

#[test]
fn parses_one_or_many_transfers() {
    let one = parse_transfers(&s(&["stripe-acme"]), Some("STRIPE_SECRET_KEY")).unwrap();
    assert_eq!(
        one,
        vec![Transfer {
            secret: "STRIPE_SECRET_KEY".into(),
            cred: "stripe-acme".into()
        }]
    );
    let many = parse_transfers(
        &s(&["CWS_CLIENT_ID=cws-id", "CWS_CLIENT_SECRET=cws-secret"]),
        None,
    )
    .unwrap();
    assert_eq!(many.len(), 2);
    assert_eq!(many[1].secret, "CWS_CLIENT_SECRET");
    assert_eq!(many[1].cred, "cws-secret");

    for (items, as_) in [
        (s(&["a", "b"]), Some("X")),
        (s(&["X=a"]), Some("X")),
        (s(&["no-equals"]), None),
        (s(&["=a"]), None),
        (s(&["X="]), None),
        (s(&["X=a", "X=b"]), None),
        (s(&["bad-name=a"]), None),
        (s(&[]), None),
    ] {
        assert!(parse_transfers(&items, as_).is_err(), "{items:?} {as_:?}");
    }
}

#[test]
fn via_names_gh_or_a_credential() {
    assert_eq!(Via::parse("gh"), Via::Gh(None));
    assert_eq!(Via::parse("gh:octo"), Via::Gh(Some("octo".into())));
    assert_eq!(Via::parse("github-acme"), Via::Cred("github-acme".into()));
    assert_eq!(Via::parse("gh:"), Via::Cred("gh:".into()));
}

#[test]
fn reads_repo_permissions() {
    let j = |v: serde_json::Value| Access::from_repo(&serde_json::json!({ "permissions": v }));
    assert_eq!(
        j(serde_json::json!({"admin": true, "push": true})),
        Access::Admin
    );
    assert_eq!(
        j(serde_json::json!({"admin": false, "push": true})),
        Access::Write
    );
    assert_eq!(
        j(serde_json::json!({"admin": false, "pull": true})),
        Access::Read
    );
    assert_eq!(Access::from_repo(&serde_json::json!({})), Access::Unknown);
}

fn up(source: &str, gh: bool, login: &str, access: Access) -> Uploader {
    Uploader {
        source: source.into(),
        login: Some(login.into()),
        access,
        program: gh.then(|| PathBuf::from("/opt/homebrew/bin/gh")),
        cred: github_cred(),
        token: "t".into(),
    }
}

const REPO: &str = "octo/ext";

#[test]
fn picks_an_admin_login_and_explains_when_there_is_none() {
    // The session this was built from: gh is the owner, the stored
    // token belongs to another account.
    let got = choose(
        vec![
            up("gh", true, "octo", Access::Admin),
            up("github-work", false, "workbot", Access::Hidden),
        ],
        false,
        REPO,
    )
    .unwrap();
    assert_eq!(got.source, "gh");

    // Admin wins over "didn't say", wherever it is.
    let got = choose(
        vec![
            up("gh", true, "octo", Access::Unknown),
            up("github-work", false, "workbot", Access::Admin),
        ],
        false,
        REPO,
    )
    .unwrap();
    assert_eq!(got.source, "github-work");

    let err = choose(
        vec![
            up("gh", true, "octo", Access::Write),
            up("github-work", false, "workbot", Access::Hidden),
        ],
        false,
        REPO,
    )
    .err()
    .unwrap()
    .to_string();
    assert!(
        err.contains("gh: signed in as octo, write but not admin on octo/ext"),
        "{err}"
    );
    assert!(
        err.contains("github-work: signed in as workbot, can't see octo/ext"),
        "{err}"
    );
    assert!(err.contains("gh auth login"), "{err}");
}

#[test]
fn a_login_asked_for_must_work_itself_and_the_error_names_one_that_does() {
    let err = choose(
        vec![
            up("github-work", false, "workbot", Access::Hidden),
            up("gh", true, "octo", Access::Admin),
        ],
        true,
        REPO,
    )
    .err()
    .unwrap()
    .to_string();
    assert!(err.contains("can't see octo/ext"), "{err}");
    assert!(err.contains("gh can: use --via gh"), "{err}");

    let got = choose(
        vec![
            up("github-work", false, "workbot", Access::Admin),
            up("gh", true, "octo", Access::Admin),
        ],
        true,
        REPO,
    )
    .unwrap();
    assert_eq!(got.source, "github-work");
}

#[test]
fn one_dialog_lists_every_secret_and_the_real_account() {
    let t = parse_transfers(
        &s(&[
            "CWS_CLIENT_ID=cws-id",
            "CWS_CLIENT_SECRET=cws-secret",
            "CWS_REFRESH_TOKEN=cws-refresh",
        ]),
        None,
    )
    .unwrap();
    let q = question(
        &t,
        REPO,
        &up("gh", true, "octo", Access::Admin),
        &["CWS_REFRESH_TOKEN".into()],
    );
    assert!(
        q.starts_with("Copy 3 secrets into octo/ext's GitHub Actions?"),
        "{q}"
    );
    assert!(q.contains("        CWS_CLIENT_ID  ←  cws-id\n"), "{q}");
    assert!(
        q.contains("CWS_REFRESH_TOKEN  ←  cws-refresh  (replaces)"),
        "{q}"
    );
    assert!(
        q.contains("Uploaded as octo (admin there), using your gh login"),
        "{q}"
    );
    assert!(!q.contains(" t "), "no token in the dialog");
}
