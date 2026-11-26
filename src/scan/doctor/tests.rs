use super::*;

#[test]
fn dates() {
    assert_eq!(civil_from_days(0), (1970, 1, 1));
    assert_eq!(civil_from_days(20_401), (2025, 11, 9));
}

#[test]
fn chunked_scan_matches_whole_file_scan() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.jsonl");
    let mut text = String::new();
    let tokens: Vec<String> = (0..40)
        .map(|i| format!("ghp_Q8mZ2vK7xR4tW9pN3bY6cJ1hL5dF0gS2a{i:03}"))
        .collect();
    for (i, t) in tokens.iter().enumerate() {
        text.push_str(&format!(
            "{{\"n\":{i},\"pad\":\"{}\",\"t\":\"{t}\"}}\n",
            "x".repeat(i * 7)
        ));
    }
    std::fs::write(&path, &text).unwrap();
    let size = text.len() as u64;
    for chunk in [1, 7, 64, 500, size] {
        let mut got: Vec<String> = Vec::new();
        let mut start = 0;
        while start < size {
            got.extend(
                scan_range(&path, start, (start + chunk).min(size))
                    .into_iter()
                    .map(|f| f.value),
            );
            start += chunk;
        }
        got.sort();
        let mut want = tokens.clone();
        want.sort();
        assert_eq!(got, want, "chunk size {chunk}");
    }
}

#[test]
fn date_and_counter_names_are_not_secrets() {
    assert!(!is_secret_name("claudeCodeFirstTokenDate"));
    assert!(!is_secret_name("maxTokens"));
    assert!(!is_secret_name("GITHUB_API_URL"));
    assert!(is_secret_name("GITHUB_PERSONAL_ACCESS_TOKEN"));
    assert!(!detect::plausible_value("2025-01-01T00:00:00Z"));
    assert!(!detect::plausible_value("YourSecretKeyForSecurity"));
}

#[test]
fn masks() {
    assert_eq!(mask("ghp_1234567890abcdefXYZ"), "ghp_…XYZ");
    assert_eq!(mask("short123"), "sh…");
}

#[test]
fn env_file_names() {
    assert!(is_env_file(".env") && is_env_file(".env.local") && is_env_file("jj-stripe-test.env"));
    assert!(!is_env_file(".env.example") && !is_env_file("environment.ts"));
}

#[test]
fn config_paths_read_well() {
    let p: Vec<String> = ["projects", "/x/app", "mcpServers", "github", "env", "TOKEN"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(config_path(&p), "project /x/app: github.env.TOKEN");
}

#[test]
fn end_to_end_on_a_fake_home() {
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    let leaked = "ghp_Q8mZ2vK7xR4tW9pN3bY6cJ1hL5dF0gS2aE7u";
    let plain = "Zq8vL2mK9xR4tW7pN3bY6cJ1";
    std::fs::create_dir_all(h.join(".claude/projects/-Users-me-app")).unwrap();
    std::fs::write(
        h.join(".claude/projects/-Users-me-app/s1.jsonl"),
        format!("{{\"message\":\"use {leaked} please\"}}\n"),
    )
    .unwrap();
    std::fs::write(
        h.join(".claude.json"),
        format!(
            r#"{{"mcpServers":{{"jj":{{"env":{{"JJ_API_KEY":"{plain}","URL":"https://x"}}}}}}}}"#
        ),
    )
    .unwrap();
    std::fs::write(
        h.join(".zsh_history"),
        format!("curl -H 'Authorization: Bearer {plain}'\n"),
    )
    .unwrap();
    let report = run(&Options {
        home: h.to_path_buf(),
        dir: h.to_path_buf(),
        check_stored: false,
    });
    assert_eq!(report.entries.len(), 2, "{report:#?}");
    assert_eq!(report.entries[0].severity(), Severity::Leaked);
    assert_eq!(report.entries[0].label.as_deref(), Some("GitHub token"));
    assert_eq!(report.entries[1].severity(), Severity::Plaintext);
    assert_eq!(report.entries[1].locations.len(), 2, "config + history");
    let text = render(&report, false);
    assert!(
        !text.contains(leaked) && !text.contains(plain),
        "secrets must be masked:\n{text}"
    );
    assert!(text.contains("jj.env.JJ_API_KEY"), "{text}");
    assert!(
        text.contains("Claude Code session in ~/app") || text.contains("Claude Code session in"),
        "{text}"
    );
}
