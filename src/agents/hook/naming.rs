//! Naming a pasted secret: from the message ("the PAT for our acme
//! account"), else the conversation it was pasted into (`HOMEBREW_TAP_TOKEN`
//! a few messages up), else the git repo the agent works in.

use super::*;

/// Words that never name an account or project.
pub(super) const NOT_A_NAME: &[&str] = &[
    "the",
    "my",
    "our",
    "your",
    "this",
    "that",
    "these",
    "it",
    "me",
    "us",
    "you",
    "a",
    "an",
    "prod",
    "production",
    "dev",
    "staging",
    "local",
    "now",
    "here",
    "there",
    "use",
    "using",
    "token",
    "tokens",
    "pat",
    "key",
    "keys",
    "api",
    "access",
    "personal",
    "secret",
    "password",
    "account",
    "new",
    "old",
    "main",
    "test",
    "later",
    "help",
    "setup",
    "set",
    "check",
    "real",
    "github",
    "gitlab",
    "bamboo",
    "jenkins",
    "aws",
    "npm",
    "openai",
    "stripe",
    "slack",
    "postgres",
    "own",
    "everything",
    "something",
    "all",
    "both",
    "each",
    "free",
    "paid",
    "same",
    "other",
    "another",
];

/// "<name> github account", "<name> org", "<name> project"...
/// A variable named for a secret: `HOMEBREW_TAP_TOKEN`, `CWS_CLIENT_SECRET`.
pub(super) static SECRET_VAR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b[A-Z][A-Z0-9]*(?:_[A-Z0-9]+)*_(?:TOKEN|KEY|SECRET|PAT|PASSWORD)\b").unwrap()
});

pub(super) static ACCOUNT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b([a-z0-9][a-z0-9_.-]{1,38})\s+(?:(?:github|gitlab|bamboo|jenkins|aws|npm|openai|stripe|slack)\s+)?(?:account|org|organisation|organization|team|workspace|project|repo|company|client)\b",
    )
    .unwrap()
});
/// "for <name>", "of <name>", "from <name>".
pub(super) static FOR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:for|of|from|at)\s+(?:the\s+|my\s+|our\s+)?([a-z0-9][a-z0-9_.-]{1,38})\b")
        .unwrap()
});

/// What the message says the secret is for, e.g. "launchstory" in "this is
/// the PAT for the launchstory GitHub account".
pub(super) fn context_hint(prompt: &str, found: &[detect::Found]) -> Option<String> {
    let mut text = prompt.to_string();
    for f in found {
        text = text.replace(&f.value, " ");
    }
    let ok = |w: &str| {
        let w = w.to_ascii_lowercase();
        !NOT_A_NAME.contains(&w.as_str()) && !w.chars().all(|c| c.is_ascii_digit())
    };
    let hit = ACCOUNT_RE
        .captures_iter(&text)
        .chain(FOR_RE.captures_iter(&text))
        .map(|c| c[1].trim_matches(['.', '-', '_']).to_string())
        .find(|w| w.len() >= 2 && ok(w))?;
    let slug: String = hit
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    (!slug.is_empty()).then_some(slug)
}

/// What to add to a secret's name, and where it came from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Hint {
    pub(super) name: Option<String>,
    /// Said when the name came from the conversation, not the message.
    pub(super) why: Option<String>,
}

impl Hint {
    pub(super) fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

/// What to add to a secret's name: what the message says it's for; else
/// what the conversation it was pasted into says ("HOMEBREW_TAP_TOKEN" a
/// few messages up); else the git repo the agent works in
/// ("stripe-launchstory" for a bare Stripe key pasted in
/// ~/Developer/LaunchStory).
pub(super) fn name_hint(prompt: &str, found: &[detect::Found], cwd: Option<&str>) -> Hint {
    if let Some(h) = context_hint(prompt, found) {
        return Hint {
            name: Some(h),
            why: None,
        };
    }
    if let Some((h, why)) = CONVERSATION.with(|c| conversation_hint(&c.borrow(), found)) {
        return Hint {
            name: Some(h),
            why: Some(why),
        };
    }
    Hint {
        name: cwd.and_then(project_hint),
        why: None,
    }
}

/// A secret's name from the conversation: the last secret-style variable
/// named in it (`HOMEBREW_TAP_TOKEN` gives `homebrew-tap`), else the last
/// "token for X" / "X account" phrase. Secrets are taken out first, so
/// none of them can end up in a name.
pub(super) fn conversation_hint(text: &str, found: &[detect::Found]) -> Option<(String, String)> {
    if text.trim().is_empty() {
        return None;
    }
    let mut text = text.to_string();
    for f in found {
        text = text.replace(&f.value, " ");
    }
    for v in detect::scan(&text) {
        text = text.replace(&v.value, " ");
    }
    if let Some((name, var)) = SECRET_VAR_RE
        .find_iter(&text)
        .filter_map(|m| var_hint(m.as_str()).map(|h| (h, m.as_str().to_string())))
        .last()
    {
        return Some((name, format!("named from the conversation: {var}")));
    }
    // The phrase closest to the end of the conversation.
    let tail: String = {
        let start = text.len().saturating_sub(1500);
        let start = (start..=text.len())
            .find(|&i| text.is_char_boundary(i))
            .unwrap_or(text.len());
        text[start..].to_string()
    };
    let h = context_hint(&tail, &[])?;
    Some((h.clone(), format!("named from the conversation: \"{h}\"")))
}

/// `HOMEBREW_TAP_TOKEN` → `homebrew-tap`; None for ones that say nothing
/// (`GH_TOKEN`, `GITHUB_TOKEN`, `API_KEY`).
pub(super) fn var_hint(var: &str) -> Option<String> {
    const SERVICES: &[&str] = &[
        "GITHUB", "GH", "GITLAB", "GL", "STRIPE", "OPENAI", "BAMBOO", "JENKINS", "NPM", "AWS",
    ];
    const FILLER: &[&str] = &[
        "TOKEN", "KEY", "SECRET", "PAT", "PASSWORD", "API", "ACCESS", "AUTH", "PERSONAL", "CLIENT",
        "PRIVATE", "SECRETS",
    ];
    let words: Vec<&str> = var
        .split('_')
        .filter(|w| !w.is_empty() && !SERVICES.contains(w) && !FILLER.contains(w))
        .collect();
    let slug = words.join("-").to_ascii_lowercase();
    (slug.len() >= 2 && !NOT_A_NAME.contains(&slug.as_str())).then_some(slug)
}

/// The name of the git repo that holds `cwd`, as a slug. None outside a repo
/// (a home folder says nothing about which project a secret is for).
pub(super) fn project_hint(cwd: &str) -> Option<String> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let mut dir = std::path::Path::new(cwd);
    let repo = loop {
        if home.as_deref() == Some(dir) {
            return None;
        }
        if dir.join(".git").exists() {
            break dir;
        }
        dir = dir.parent()?;
    };
    let slug: String = repo
        .file_name()?
        .to_str()?
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    (slug.len() >= 2 && !NOT_A_NAME.contains(&slug.as_str())).then_some(slug)
}

/// A short description from the message itself, secrets taken out, so the
/// agent can later tell "github-launchstory" from "github-work". When the
/// name came from the conversation, it says so.
pub(super) fn description(
    f: &detect::Found,
    prompt: &str,
    found: &[detect::Found],
    hint: &Hint,
) -> String {
    let label = f.label.as_deref().unwrap_or("Secret");
    let mut d = match hint.name() {
        Some(h) => format!("{label} for {h}"),
        None => label.to_string(),
    };
    if let Some(why) = &hint.why {
        d.push_str(&format!(" ({why})"));
    }
    let mut text = prompt.to_string();
    for other in found {
        text = text.replace(&other.value, "\u{1}");
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    if let Some(at) = words.iter().position(|w| w.contains('\u{1}')) {
        let from = at.saturating_sub(14);
        let to = (at + 8).min(words.len());
        let quote = words[from..to]
            .iter()
            .filter(|w| !w.contains('\u{1}'))
            .copied()
            .collect::<Vec<_>>()
            .join(" ");
        if !quote.is_empty() {
            let quote: String = quote.chars().take(160).collect();
            let more = if from > 0 { "…" } else { "" };
            let tail = if to < words.len() { "…" } else { "" };
            d.push_str(&format!(
                ". From the message it was pasted in: \"{more}{quote}{tail}\""
            ));
        }
    }
    d
}

pub(super) fn suggested_name(found: &detect::Found, store: &Store, hint: Option<&str>) -> String {
    if found.service == Some(detect::DATABASE) {
        let base = db_base(&found.value);
        return match hint {
            Some(h) if h != base => store.free_name(&format!("{base}-{h}")),
            _ => store.free_name(base),
        };
    }
    let preset = presets::get_or_generic(found.service.unwrap_or("generic"));
    // Passwords and keys read better with what they're for first
    // ("staging-db-password"); services lead with the service
    // ("github-homebrew-tap"). Nothing is named "generic".
    let name = match (preset.kind, preset.id, hint) {
        (presets::Kind::Password, _, Some(h)) => format!("{h}-password"),
        (presets::Kind::PrivateKey, _, Some(h)) => format!("{h}-key"),
        (presets::Kind::AwsKeys, _, Some(h)) => format!("aws-{h}"),
        (_, "generic", Some(h)) => h.to_string(),
        (_, "generic", None) => "secret".to_string(),
        (_, base, Some(h)) if h != base => format!("{base}-{h}"),
        (_, base, _) => base.to_string(),
    };
    store.free_name(&name)
}

/// The name to save under: the typed one if it's valid and free.
pub(super) fn pick_name(typed: Option<&str>, fallback: String, store: &Store) -> String {
    match typed {
        Some(n) if validate_name(n).is_ok() => store.free_name(n),
        _ => fallback,
    }
}

pub(super) fn db_base(url: &str) -> &str {
    match url.split("://").next().unwrap_or("db") {
        "postgres" | "postgresql" => "postgres",
        "mongodb+srv" => "mongodb",
        "rediss" => "redis",
        s => s,
    }
}
