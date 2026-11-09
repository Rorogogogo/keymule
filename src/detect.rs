//! Spot secrets in text a user is about to send to a model.

use std::sync::LazyLock;

use regex::Regex;

use crate::presets::PRESETS;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub value: String,
    /// Preset id when we can tell which service it belongs to.
    pub service: Option<&'static str>,
}

/// Tokens with a recognisable prefix. (prefix regex, service)
static PREFIXED: LazyLock<Vec<(Regex, Option<&'static str>)>> = LazyLock::new(|| {
    [
        (r"\b(github_pat_[A-Za-z0-9_]{30,})", Some("github")),
        (r"\b(gh[pousr]_[A-Za-z0-9]{30,})", Some("github")),
        (r"\b(glpat-[A-Za-z0-9_-]{20,})", Some("gitlab")),
        (r"\b(sk-ant-[A-Za-z0-9_-]{20,})", None),
        (r"\b(sk-[A-Za-z0-9_-]{20,})", None),
        (r"\b(xox[abprs]-[A-Za-z0-9-]{10,})", None),
        (r"\b(AKIA[0-9A-Z]{16})\b", None),
        (r"\b(AIza[0-9A-Za-z_-]{35})", None),
        (r"\b(ntn_[A-Za-z0-9]{20,})", None),
        (r"\b(npm_[A-Za-z0-9]{30,})", None),
    ]
    .into_iter()
    .map(|(re, svc)| (Regex::new(re).unwrap(), svc))
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

pub fn scan(text: &str) -> Vec<Found> {
    let mut found: Vec<Found> = Vec::new();
    let mut push = |value: &str, service: Option<&'static str>| {
        let value = value.trim_end_matches(['.', ',']);
        if found
            .iter()
            .any(|f| f.value == value || f.value.contains(value))
        {
            return;
        }
        found.push(Found {
            value: value.to_string(),
            service,
        });
    };
    for (re, svc) in PREFIXED.iter() {
        for caps in re.captures_iter(text) {
            push(&caps[1], svc.or_else(|| service_near(text)));
        }
    }
    for caps in CONTEXTUAL.captures_iter(text) {
        let value = &caps[1];
        if looks_random(value) {
            push(value, service_near(text));
        }
    }
    found
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
    fn unknown_service_is_none() {
        let f = scan("my token is 8f3kd92mfa0q9w8e7r6t5y4u");
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].service, None);
    }
}
