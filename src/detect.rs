//! Spot secrets in text a user is about to send to a model.

use std::sync::LazyLock;

use regex::Regex;

use crate::presets::PRESETS;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub value: String,
    /// Preset id when we can tell which service it belongs to.
    pub service: Option<&'static str>,
    /// What it is: "GitHub token", or the variable it was assigned to.
    pub label: Option<String>,
    /// Recognisable format (prefix, env var, URL password) rather than a guess.
    pub confident: bool,
}

/// Tokens with a recognisable prefix. (regex, service, label)
static PREFIXED: LazyLock<Vec<(Regex, Option<&'static str>, &'static str)>> = LazyLock::new(|| {
    [
        (
            r"\b(github_pat_[A-Za-z0-9_]{30,})",
            Some("github"),
            "GitHub token",
        ),
        (
            r"\b(gh[pousr]_[A-Za-z0-9]{30,})",
            Some("github"),
            "GitHub token",
        ),
        (
            r"\b(glpat-[A-Za-z0-9_-]{20,})",
            Some("gitlab"),
            "GitLab token",
        ),
        (r"\b(sk-ant-[A-Za-z0-9_-]{20,})", None, "Anthropic API key"),
        (r"\b(sk-[A-Za-z0-9_-]{20,})", None, "OpenAI-style API key"),
        (r"\b(xox[abprs]-[A-Za-z0-9-]{10,})", None, "Slack token"),
        (r"\b(AKIA[0-9A-Z]{16})\b", None, "AWS access key ID"),
        (r"\b(AIza[0-9A-Za-z_-]{35})", None, "Google API key"),
        (r"\b(ntn_[A-Za-z0-9]{20,})", None, "Notion token"),
        (r"\b(npm_[A-Za-z0-9]{30,})", None, "npm token"),
        (
            r"\b((?:sk|rk)_live_[A-Za-z0-9]{20,})",
            None,
            "Stripe live key",
        ),
        (
            r"\b((?:sk|rk)_test_[A-Za-z0-9]{20,})",
            None,
            "Stripe test key",
        ),
    ]
    .into_iter()
    .map(|(re, svc, label)| (Regex::new(re).unwrap(), svc, label))
    .collect()
});

/// "token is X", "api key: X", "password = X" for tokens without a prefix
/// (Bamboo and Jenkins tokens, for example).
static CONTEXTUAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)\b(?:token|pat|api[ _-]?key|apikey|secret|password|passwd)\b(?:\s*(?:is|=|:)\s*|\s+)["'`]?([A-Za-z0-9_\-.+/=~]{16,})"#,
    )
    .unwrap()
});

/// `GITHUB_TOKEN=x`, `"api_key": "x"`: assignments to secret-looking names.
static ASSIGNED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)\b([A-Z0-9_]*(?:TOKEN|SECRET|PASSWORD|PASSWD|API_?KEY|ACCESS_?KEY|PRIVATE_?KEY|AUTH_?KEY|CLIENT_?SECRET)[A-Z0-9_]*)["']?\s*[:=]\s*["']?([A-Za-z0-9_\-.+/=~]{12,})"#,
    )
    .unwrap()
});

/// `scheme://user:password@host`
static URL_PASSWORD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\b([a-z][a-z0-9+.-]*)://[^/\s:@"'<>]+:([^@\s/"'<>]{4,})@"#).unwrap()
});

static BEARER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bBearer\s+([A-Za-z0-9_\-.=+/]{20,})").unwrap());

fn push(
    found: &mut Vec<Found>,
    value: &str,
    service: Option<&'static str>,
    label: Option<String>,
    confident: bool,
) {
    let value = value.trim_end_matches(['.', ',', '\\']);
    if let Some(existing) = found.iter_mut().find(|f| f.value == value) {
        existing.confident |= confident;
        if existing.label.is_none() {
            existing.label = label;
        }
        return;
    }
    if found.iter().any(|f| f.value.contains(value)) {
        return;
    }
    found.push(Found {
        value: value.to_string(),
        service,
        label,
        confident,
    });
}

/// For prompts: prefixed tokens plus "my X token is ..." phrasing.
pub fn scan(text: &str) -> Vec<Found> {
    let mut found = Vec::new();
    for (re, svc, label) in PREFIXED.iter() {
        for caps in re.captures_iter(text) {
            let svc = svc.or_else(|| service_near(text));
            push(&mut found, &caps[1], svc, Some(label.to_string()), true);
        }
    }
    for caps in CONTEXTUAL.captures_iter(text) {
        let value = &caps[1];
        if looks_random(value) {
            push(&mut found, value, service_near(text), None, false);
        }
    }
    found
}

/// For files and transcripts: everything `scan` finds plus assignments,
/// URL passwords and bearer tokens, minus obvious placeholders.
pub fn scan_deep(text: &str) -> Vec<Found> {
    let mut found = Vec::new();
    for (re, svc, label) in PREFIXED.iter() {
        for caps in re.captures_iter(text) {
            push(&mut found, &caps[1], *svc, Some(label.to_string()), true);
        }
    }
    for caps in ASSIGNED.captures_iter(text) {
        let (name, value) = (&caps[1], &caps[2]);
        if looks_random(value) && !name.to_ascii_uppercase().contains("AUTHOR") {
            // UPPER_SNAKE names are env vars; camelCase hits are often code.
            let env_style = name.chars().all(|c| !c.is_ascii_lowercase());
            push(
                &mut found,
                value,
                None,
                Some(name.to_string()),
                env_style && value.len() >= 16,
            );
        }
    }
    for caps in URL_PASSWORD.captures_iter(text) {
        let password = &caps[2];
        if !password.starts_with('$') && !password.starts_with('{') {
            push(
                &mut found,
                password,
                None,
                Some(format!("{} URL password", &caps[1])),
                true,
            );
        }
    }
    for caps in BEARER.captures_iter(text) {
        let value = &caps[1];
        if looks_random(value) {
            push(
                &mut found,
                value,
                None,
                Some("bearer token".into()),
                value.len() >= 24,
            );
        }
    }
    for caps in CONTEXTUAL.captures_iter(text) {
        let value = &caps[1];
        if looks_random(value) {
            push(&mut found, value, None, None, false);
        }
    }
    found.retain(|f| !looks_fake(&f.value));
    found
}

/// Whether a value assigned to a secret-looking name could be a credential:
/// one opaque word with letters and digits, not a date, number or sentence.
pub fn plausible_value(value: &str) -> bool {
    static DATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d{4}-\d{2}-\d{2}").unwrap());
    value.len() >= 8
        && !value.contains(char::is_whitespace)
        && !value.starts_with('$')
        && !DATE.is_match(value)
        && !value
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == '-')
        && !matches!(value, "true" | "false" | "null")
        && (looks_random(value) || value.len() >= 24)
        && !looks_fake(value)
}

/// Placeholders and test values that are not worth reporting.
pub fn looks_fake(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    const MARKERS: &[&str] = &[
        "your",
        "zyxwvu",
        "9876543",
        "qwerty",
        "secretkey",
        "supersecret",
        "abcdefgh",
        "0123456",
        "123456789",
        "xxxxxx",
        "example",
        "placeholder",
        "your_",
        "your-",
        "yourtoken",
        "redacted",
        "dummy",
        "changeme",
        "replace",
        "****",
        "…",
        "<",
        ">",
        "${",
        "fake",
        "sample",
    ];
    if MARKERS.iter().any(|m| lower.contains(m)) {
        return true;
    }
    // "password", "postgres", "secret": a word, not a credential.
    let distinct: std::collections::HashSet<char> = lower.chars().collect();
    distinct.len() <= 3
        || matches!(
            lower.as_str(),
            "password" | "postgres" | "secret" | "admin" | "root" | "pass"
        )
}

/// The first preset keyword mentioned in the text.
fn service_near(text: &str) -> Option<&'static str> {
    let lower = text.to_ascii_lowercase();
    PRESETS
        .iter()
        .filter_map(|p| {
            p.keywords
                .iter()
                .filter_map(|k| lower.find(k))
                .min()
                .map(|pos| (pos, p.id))
        })
        .min()
        .map(|(_, id)| id)
}

/// Filters out ordinary words like "authentication_settings": a token
/// has letters and digits and isn't a URL or path.
fn looks_random(s: &str) -> bool {
    let has_digit = s.chars().any(|c| c.is_ascii_digit());
    let has_alpha = s.chars().any(|c| c.is_ascii_alphabetic());
    let looks_like_path = s.starts_with('/') || s.contains("://") || s.matches('/').count() > 2;
    has_digit && has_alpha && !looks_like_path && shannon(s) >= 3.0
}

fn shannon(s: &str) -> f64 {
    let mut counts = [0usize; 256];
    for b in s.bytes() {
        counts[b as usize] += 1;
    }
    let n = s.len() as f64;
    counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / n;
            -p * p.log2()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn unknown_service_is_none() {
        let f = scan("my token is 8f3kd92mfa0q9w8e7r6t5y4u");
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].service, None);
    }
}
