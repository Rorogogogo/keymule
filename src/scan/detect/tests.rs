use super::*;

#[test]
fn stripe_and_openai_keys_map_to_their_presets() {
    // Split so the source never holds something that looks like a real key.
    let f = scan(concat!(
        "sk_",
        "live_",
        "51ExampleExampleExample0123456789abcd"
    ));
    assert_eq!(f[0].service, Some("stripe"));
    let f = scan(concat!(
        "rk_",
        "test_",
        "51ExampleExampleExample0123456789abcd"
    ));
    assert_eq!(f[0].service, Some("stripe"));
    let f = scan("sk-proj-Example_Example-Example0123456789");
    assert_eq!(
        (f[0].service, f[0].label.as_deref()),
        (Some("openai"), Some("OpenAI API key"))
    );
    // Plain sk- keys are used by other services too: no guess.
    assert_eq!(scan("sk-ExampleExampleExample0123456789")[0].service, None);
}

#[test]
fn finds_prefixed_github_token() {
    let f = scan("use ghp_abcdefghijklmnopqrstuvwxyz0123456789 to check actions");
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].service, Some("github"));
}

#[test]
fn finds_bamboo_token_by_context() {
    let f = scan("here's my bamboo token NjQ3MjE4MzQ1Njc4OkxQmPq9Zr2Wd+Hs8 check the pipeline");
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].value, "NjQ3MjE4MzQ1Njc4OkxQmPq9Zr2Wd+Hs8");
    assert_eq!(f[0].service, Some("bamboo"));
}

#[test]
fn ignores_ordinary_prose() {
    assert!(scan("the token authentication_middleware is broken").is_empty());
    assert!(scan("explain how a password reset flow works").is_empty());
    assert!(scan("api key rotation docs: https://example.com/docs/keys/rotation").is_empty());
}

#[test]
fn deep_scan_finds_env_vars_url_passwords_and_bearer() {
    let text = r#"ACME_API_KEY=ac_9fK2mQ7xL4pR8vT1wZ3c postgres://app:Hunter2Secret9@db:5432/x curl -H "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.k3Jd8Qp2Lm""#;
    let f = scan_deep(text);
    let labels: Vec<_> = f
        .iter()
        .map(|x| x.label.clone().unwrap_or_default())
        .collect();
    assert!(labels.contains(&"ACME_API_KEY".to_string()), "{labels:?}");
    assert!(
        labels.contains(&"postgres URL password".to_string()),
        "{labels:?}"
    );
    assert!(labels.contains(&"bearer token".to_string()), "{labels:?}");
    assert!(f.iter().all(|x| x.confident));
}

#[test]
fn deep_scan_drops_placeholders() {
    let text = "GITHUB_TOKEN=ghp_abcdefghijklmnopqrstuvwxyz0123456789 API_KEY=your_api_key_here_123 postgres://u:password@h/db TOKEN=${GITHUB_TOKEN}";
    assert!(scan_deep(text).is_empty(), "{:?}", scan_deep(text));
}

#[test]
fn deep_scan_handles_json_quoting() {
    let f = scan_deep(r#"{"env":{"STRIPE_SECRET_KEY":"Zq8vL2mK9xR4tW7pN3bY6cJ1"}}"#);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].label.as_deref(), Some("STRIPE_SECRET_KEY"));
}

#[test]
fn database_urls_in_prompts() {
    let f = scan("add a pg mcp for postgres://app:Vq7Lp2Xk9Rt4@db.internal:5432/app please");
    assert_eq!(f.len(), 1, "{f:?}");
    assert_eq!(
        f[0].value,
        "postgres://app:Vq7Lp2Xk9Rt4@db.internal:5432/app"
    );
    assert_eq!(f[0].service, Some(DATABASE));
    assert_eq!(f[0].label.as_deref(), Some("postgres URL with password"));
    // Defaults and placeholders are fine to discuss.
    assert!(scan("postgres://postgres:postgres@localhost:5432/dev").is_empty());
    assert!(scan("postgresql://user:${PGPASSWORD}@localhost/db").is_empty());
    assert!(scan("mysql://root:<password>@localhost/db").is_empty());
}

#[test]
fn unknown_service_is_none() {
    let f = scan("my token is 8f3kd92mfa0q9w8e7r6t5y4u");
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].service, None);
}

#[test]
fn private_keys_are_caught_whole() {
    let pem = "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQ\nAAAAMwAAAAtzc2gtZWQ\n-----END OPENSSH PRIVATE KEY-----";
    let f = scan(&format!("here is my deploy key\n{pem}\nthanks"));
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].value, pem);
    assert_eq!(f[0].label.as_deref(), Some("SSH private key"));

    let json = r#"{"type": "service_account", "private_key": "-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASC\n-----END PRIVATE KEY-----\n", "client_email": "x@y.iam.gserviceaccount.com"}"#;
    let f = scan(json);
    assert_eq!(f[0].label.as_deref(), Some("Google service account key"));
    assert!(f[0].value.starts_with("-----BEGIN PRIVATE KEY-----"));
    assert!(scan("-----BEGIN PUBLIC KEY-----\nMIIB\n-----END PUBLIC KEY-----").is_empty());
}

#[test]
fn aws_secret_keys_are_caught() {
    let secret = fake::aws_secret();
    let id = fake::aws_id();
    let f = scan(&format!("aws_secret_access_key = {secret}"));
    assert!(
        f.iter()
            .any(|x| x.value == secret && x.label.as_deref() == Some("AWS secret access key"))
    );
    let f = scan(&format!("id {id} and {secret} for prod"));
    assert!(f.iter().any(|x| x.value == secret), "{f:?}");
    assert!(f.iter().any(|x| x.value == id));
    // Without an access key ID, a lone 40-character string isn't assumed.
    assert!(scan(&format!("commit {secret}")).is_empty());
}

#[test]
fn labelled_passwords_are_caught_and_prose_is_not() {
    for (msg, pw) in [
        ("the staging password is Tr0ub4dor&3", "Tr0ub4dor&3"),
        ("login admin, password: Secr3t!x", "Secr3t!x"),
        ("DB_PASSWORD=hunter2Rocks9", "hunter2Rocks9"),
        (r#"{"user": "app", "password": "pa55-Word"}"#, "pa55-Word"),
        ("pw is kX9!m2#Qp7", "kX9!m2#Qp7"),
        (
            "the password for our acme account is Tr0ub4dor&3x",
            "Tr0ub4dor&3x",
        ),
    ] {
        let f = scan(msg);
        assert!(
            f.iter()
                .any(|x| x.value == pw && x.label.as_deref() == Some("password")),
            "{msg}: {f:?}"
        );
    }
    for msg in [
        "the password is required",
        "password: please reset it tomorrow",
        "password - the field below",
        "set PASSWORD=$DB_PASS in the env",
        "password is hunter",
        "the password for the admin account is required",
    ] {
        assert!(scan(msg).is_empty(), "{msg}: {:?}", scan(msg));
    }
}
