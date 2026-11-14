//! The prompt hook for Claude Code (`UserPromptSubmit`), Codex
//! (`UserPromptSubmit`) and Cursor (`beforeSubmitPrompt`). It runs locally
//! before a prompt is sent. A hook can't edit a prompt, only let it through or stop it, so when
//! a prompt has a secret keymule holds it back (in the keychain, for a few
//! minutes) and asks, right in the terminal, what to do. The next prompt is
//! the answer: `y` saves the secret and sends the held-back message with the
//! secret swapped for its keymule name, `y <name>` saves it under that name,
//! and `n` sends the message as it was.

use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::detect;
use crate::presets::{self, AuthKind};
use crate::store::{Auth, Credential, Sealed, Store, Vault, host_of, validate_name};

/// Put this anywhere in a prompt to send it as-is.
pub const IGNORE_MARKER: &str = "#keymule-ignore";

/// Where the held-back prompt waits for the answer.
const PENDING: &str = "pending:prompt";
/// How long it waits.
const PENDING_SECS: u64 = 15 * 60;

/// Which agent runs the hook: each reads a different JSON answer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Agent {
    #[default]
    Claude,
    Codex,
    /// Cursor can only let a prompt through or stop it: no added context.
    Cursor,
}

#[derive(Deserialize)]
struct HookInput {
    #[serde(default)]
    prompt: String,
    /// Cursor calls it `conversation_id`.
    #[serde(default, alias = "conversation_id")]
    session_id: String,
}

#[derive(Serialize, Deserialize)]
struct Pending {
    session: String,
    at: u64,
    prompt: String,
}

#[derive(Debug, PartialEq, Eq)]
enum Reply {
    Save(Option<String>),
    Send,
}

/// What the hook does with a prompt.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Pass,
    /// Stop the prompt and show the user this.
    Block(String),
    /// Let the prompt through with this context for the model, and show the
    /// user the notice.
    Context {
        context: String,
        notice: String,
    },
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
fn run(stdin: &str, store: &mut Store, vault: &dyn Vault) -> Option<String> {
    run_as(Agent::Claude, stdin, store, vault)
}

/// Returns the JSON to print on stdout, or None to let the prompt through.
pub fn run_as(agent: Agent, stdin: &str, store: &mut Store, vault: &dyn Vault) -> Option<String> {
    let input: HookInput = serde_json::from_str(stdin).ok()?;
    let outcome = handle_as(agent, &input.prompt, &input.session_id, store, vault, now());
    let out = match (agent, outcome) {
        (Agent::Cursor, Outcome::Pass) => serde_json::json!({ "continue": true }),
        (_, Outcome::Pass) => return None,
        (Agent::Cursor, Outcome::Block(reason)) => serde_json::json!({
            "continue": false,
            "user_message": reason,
        }),
        (Agent::Claude, Outcome::Block(reason)) => serde_json::json!({
            "decision": "block",
            "reason": reason,
            // Claude Code would otherwise repeat the prompt, secret and all.
            "hookSpecificOutput": {
                "hookEventName": "UserPromptSubmit",
                "suppressOriginalPrompt": true,
            },
        }),
        (Agent::Codex, Outcome::Block(reason)) => serde_json::json!({
            "decision": "block",
            "reason": reason,
        }),
        // Cursor never gets here: its answers are blocks (see `answer`).
        (_, Outcome::Context { context, notice }) => serde_json::json!({
            "systemMessage": notice,
            "hookSpecificOutput": {
                "hookEventName": "UserPromptSubmit",
                "additionalContext": context,
            },
        }),
    };
    Some(out.to_string())
}

/// `y`, `y <name>`, `n` and their spelled-out forms.
fn parse_reply(prompt: &str) -> Option<Reply> {
    let mut words = prompt.split_whitespace();
    let first = words.next()?.to_ascii_lowercase();
    let name = words.next().map(str::to_string);
    if words.next().is_some() {
        return None;
    }
    match first.as_str() {
        "y" | "yes" | "save" => Some(Reply::Save(name)),
        "n" | "no" | "send" if name.is_none() => Some(Reply::Send),
        _ => None,
    }
}

#[cfg(test)]
fn handle(prompt: &str, session: &str, store: &mut Store, vault: &dyn Vault, at: u64) -> Outcome {
    handle_as(Agent::Claude, prompt, session, store, vault, at)
}

pub fn handle_as(
    agent: Agent,
    prompt: &str,
    session: &str,
    store: &mut Store,
    vault: &dyn Vault,
    at: u64,
) -> Outcome {
    if let Some(reply) = parse_reply(prompt)
        && let Some(p) = take_pending(vault, session, at)
    {
        return answer(agent, &p.prompt, reply, store, vault);
    }
    if prompt.contains(IGNORE_MARKER) {
        return Outcome::Pass;
    }
    let found = detect::scan(prompt);
    if found.is_empty() {
        return Outcome::Pass;
    }
    let held = serde_json::to_string(&Pending {
        session: session.to_string(),
        at,
        prompt: prompt.to_string(),
    })
    .unwrap_or_default();
    if let Err(e) = vault.set(PENDING, &held) {
        // Can't hold it for an answer: keep the old behaviour and save now.
        let (_, notes) = save_all(prompt, None, store, vault);
        return Outcome::Block(format!(
            "keymule couldn't hold your message ({e:#}), so it saved the secret and stopped the message.\nkeymule saved {}.\nSend your message again without the secret.",
            notes.join(", ")
        ));
    }
    Outcome::Block(question(agent, prompt, &found, store, vault))
}

fn take_pending(vault: &dyn Vault, session: &str, at: u64) -> Option<Pending> {
    let p: Pending = serde_json::from_str(&vault.get(PENDING).ok()?).ok()?;
    let _ = vault.delete(PENDING);
    (p.session == session && at.saturating_sub(p.at) <= PENDING_SECS).then_some(p)
}

fn describe(f: &detect::Found) -> String {
    format!(
        "{} ({})",
        f.label.as_deref().unwrap_or("secret"),
        crate::doctor::mask(&f.value)
    )
}

fn question(
    agent: Agent,
    prompt: &str,
    found: &[detect::Found],
    store: &Store,
    vault: &dyn Vault,
) -> String {
    // Cursor can't send a changed message, so `y` there saves and hands
    // back the message to send again.
    let resend = agent == Agent::Cursor;
    let hint = context_hint(prompt, found);
    let what = if found.len() == 1 {
        format!("it has a {}", describe(&found[0]))
    } else {
        let list: Vec<String> = found.iter().map(describe).collect();
        format!("it has {} secrets: {}", found.len(), list.join(", "))
    };
    let mut lines = vec![format!("keymule held back your message: {what}.")];
    if found.len() == 1 {
        let f = &found[0];
        match known_as(f, store, vault) {
            Some(n) => {
                lines.push(format!("It's already in keymule as `{n}`."));
                lines.push(if resend {
                    format!("  y           get your message back with `{n}` in its place")
                } else {
                    format!("  y           send your message with `{n}` in its place")
                });
            }
            None => {
                let name = suggested_name(f, store, hint.as_deref());
                lines.push(if resend {
                    format!(
                        "  y           save it as `{name}` and get your message back without it"
                    )
                } else {
                    format!("  y           save it as `{name}` and send your message without it")
                });
                lines.push("  y <name>    save it under another name".to_string());
            }
        }
    } else {
        lines.push(if resend {
            "  y           save them and get your message back without them".into()
        } else {
            "  y           save them and send your message without them".into()
        });
    }
    if resend {
        lines.push(format!(
            "To send it as it was, add {IGNORE_MARKER} to it and send it again."
        ));
    } else {
        lines.push("  n           send your message as it was".into());
    }
    lines.push(format!(
        "(It waits {} minutes. Anything else you type is sent as usual.)",
        PENDING_SECS / 60
    ));
    lines.join("\n")
}

/// Act on the answer to a held-back prompt.
fn answer(
    agent: Agent,
    prompt: &str,
    reply: Reply,
    store: &mut Store,
    vault: &dyn Vault,
) -> Outcome {
    let typed = match reply {
        Reply::Send if agent == Agent::Cursor => {
            return Outcome::Block(format!(
                "Cursor can't send a held-back message, so add {IGNORE_MARKER} to it and send it again."
            ));
        }
        Reply::Send => {
            return Outcome::Context {
                context: format!(
                    "The user's previous message was held back by keymule because it contained a secret; \
                     they chose to send it as it was. Treat it as their request:\n\n{prompt}"
                ),
                notice: "keymule sent your message as it was.".into(),
            };
        }
        Reply::Save(t) => t,
    };
    let (message, notes) = save_all(prompt, typed.as_deref(), store, vault);
    if agent == Agent::Cursor {
        return Outcome::Block(format!(
            "keymule saved {}. Cursor can't send your message for you, so send this instead:\n\n{message}",
            notes.join(", ")
        ));
    }
    Outcome::Context {
        context: format!(
            "The user's previous message was held back by keymule because it contained a secret. \
             keymule saved it ({}); use it through keymule by that name and never ask for the \
             value. Treat this as the user's request:\n\n{message}",
            notes.join(", ")
        ),
        notice: format!(
            "keymule saved {} and sent your message without it.",
            notes.join(", ")
        ),
    }
}

/// Save every secret in `prompt` (the only one under `typed`, if given) and
/// return the prompt with each swapped for its keymule name, plus a note per
/// secret.
fn save_all(
    prompt: &str,
    typed: Option<&str>,
    store: &mut Store,
    vault: &dyn Vault,
) -> (String, Vec<String>) {
    let found = detect::scan(prompt);
    let hint = context_hint(prompt, &found);
    let mut notes = Vec::new();
    let mut message = prompt.to_string();
    for f in &found {
        let typed = if found.len() == 1 { typed } else { None };
        let saved = match known_as(f, store, vault) {
            Some(n) if typed.is_none_or(|t| t == n) => Ok((n, "already saved".to_string())),
            _ => {
                let name = pick_name(typed, suggested_name(f, store, hint.as_deref()), store);
                if f.service == Some(detect::DATABASE) {
                    save_database(f, &name, store, vault)
                        .map(|n| (n, "an MCP server secret".into()))
                } else {
                    let desc = description(f, prompt, &found, hint.as_deref());
                    save(f, &name, &desc, store, vault).map(|(n, todo)| {
                        let title = presets::get_or_generic(&store.creds[&n].service).title;
                        (n, format!("{title}{todo}"))
                    })
                }
            }
        };
        match saved {
            Ok((name, note)) => {
                notes.push(format!("`{name}` ({note})"));
                message = message.replace(&f.value, &format!("[saved in keymule as `{name}`]"));
            }
            Err(e) => {
                notes.push(format!("one secret couldn't be saved ({e:#})"));
                message = message.replace(&f.value, "[secret removed by keymule]");
            }
        }
    }
    (message, notes)
}

/// `keymule hook scan`: what a prompt holds, for the Claude Code mod that
/// asks inline. No secret values in the output: masked ones only, and the
/// prompt with them taken out.
pub fn scan_json(prompt: &str, store: &Store, vault: &dyn Vault) -> serde_json::Value {
    let found = if prompt.contains(IGNORE_MARKER) {
        vec![]
    } else {
        detect::scan(prompt)
    };
    let hint = context_hint(prompt, &found);
    let mut stripped = prompt.to_string();
    let items: Vec<serde_json::Value> = found
        .iter()
        .map(|f| {
            stripped = stripped.replace(&f.value, "[secret removed by keymule]");
            serde_json::json!({
                "label": f.label.as_deref().unwrap_or("secret"),
                "masked": crate::doctor::mask(&f.value),
                "known": known_as(f, store, vault),
                "suggested": suggested_name(f, store, hint.as_deref()),
            })
        })
        .collect();
    serde_json::json!({ "found": items, "stripped": stripped, "ignore_marker": IGNORE_MARKER })
}

/// `keymule hook save`: save the secrets in a prompt and return it with each
/// swapped for its keymule name, and a note for the model.
pub fn save_json(
    prompt: &str,
    name: Option<&str>,
    store: &mut Store,
    vault: &dyn Vault,
) -> serde_json::Value {
    let (text, notes) = save_all(prompt, name, store, vault);
    serde_json::json!({
        "text": text,
        "saved": notes,
        "context": format!(
            "keymule saved the secret from this message ({}); use it through keymule by that name \
             and never ask the user for the value.",
            notes.join(", ")
        ),
    })
}

/// The name a secret already has in keymule, if it's stored.
fn known_as(found: &detect::Found, store: &Store, vault: &dyn Vault) -> Option<String> {
    if found.service == Some(detect::DATABASE) {
        return store
            .servers
            .keys()
            .find(|n| {
                crate::servers::Binding::load(vault, n)
                    .is_ok_and(|b| b.env.values().any(|v| *v == found.value))
            })
            .cloned();
    }
    store
        .creds
        .keys()
        .find(|n| Sealed::peek_secret(vault, n).as_deref() == Some(found.value.as_str()))
        .cloned()
}

/// Words that never name an account or project.
const NOT_A_NAME: &[&str] = &[
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
static ACCOUNT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b([a-z0-9][a-z0-9_.-]{1,38})\s+(?:(?:github|gitlab|bamboo|jenkins|aws|npm|openai|stripe|slack)\s+)?(?:account|org|organisation|organization|team|workspace|project|repo|company|client)\b",
    )
    .unwrap()
});
/// "for <name>", "of <name>", "from <name>".
static FOR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:for|of|from|at)\s+(?:the\s+|my\s+|our\s+)?([a-z0-9][a-z0-9_.-]{1,38})\b")
        .unwrap()
});

/// What the message says the secret is for, e.g. "launchstory" in "this is
/// the PAT for the launchstory GitHub account".
fn context_hint(prompt: &str, found: &[detect::Found]) -> Option<String> {
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

/// A short description from the message itself, secrets taken out, so the
/// agent can later tell "github-launchstory" from "github-work".
fn description(
    f: &detect::Found,
    prompt: &str,
    found: &[detect::Found],
    hint: Option<&str>,
) -> String {
    let label = f.label.as_deref().unwrap_or("Secret");
    let mut d = match hint {
        Some(h) => format!("{label} for {h}"),
        None => label.to_string(),
    };
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

fn suggested_name(found: &detect::Found, store: &Store, hint: Option<&str>) -> String {
    let base = if found.service == Some(detect::DATABASE) {
        db_base(&found.value).to_string()
    } else {
        presets::get_or_generic(found.service.unwrap_or("generic"))
            .id
            .to_string()
    };
    match hint {
        Some(h) if h != base => store.free_name(&format!("{base}-{h}")),
        _ => store.free_name(&base),
    }
}

/// The name to save under: the typed one if it's valid and free.
fn pick_name(typed: Option<&str>, fallback: String, store: &Store) -> String {
    match typed {
        Some(n) if validate_name(n).is_ok() => store.free_name(n),
        _ => fallback,
    }
}

fn db_base(url: &str) -> &str {
    match url.split("://").next().unwrap_or("db") {
        "postgres" | "postgresql" => "postgres",
        "mongodb+srv" => "mongodb",
        "rediss" => "redis",
        s => s,
    }
}

/// A database URL becomes an MCP server secret (env `DATABASE_URL`, which
/// `keymule exec --env` can rename). No command may use it until the user
/// approves one.
fn save_database(
    found: &detect::Found,
    name: &str,
    store: &mut Store,
    vault: &dyn Vault,
) -> Result<String> {
    let name = name.to_string();
    let mut binding = crate::servers::Binding::default();
    binding
        .env
        .insert("DATABASE_URL".into(), found.value.clone());
    binding.save(vault, &name)?;
    store.servers.insert(
        name.clone(),
        crate::store::ServerMeta {
            env: vec!["DATABASE_URL".into()],
            ..Default::default()
        },
    );
    if let Err(e) = store.save() {
        let _ = crate::servers::Binding::delete(vault, &name);
        store.servers.remove(&name);
        return Err(e);
    }
    Ok(name)
}

fn save(
    found: &detect::Found,
    name: &str,
    description: &str,
    store: &mut Store,
    vault: &dyn Vault,
) -> Result<(String, String)> {
    let preset = presets::get_or_generic(found.service.unwrap_or("generic"));
    let name = name.to_string();
    let url = preset.default_url.unwrap_or("").to_string();
    let hosts = if url.is_empty() {
        vec![]
    } else {
        vec![host_of(&url)?]
    };
    let auth = match preset.auth {
        AuthKind::Bearer => Auth::Bearer,
        AuthKind::Basic => Auth::Basic {
            username: String::new(),
        },
        AuthKind::Header(h) => Auth::Header { name: h.into() },
    };

    let mut todo = Vec::new();
    if url.is_empty() {
        todo.push(format!(
            "--url {}",
            preset
                .url_example
                .split_whitespace()
                .next()
                .unwrap_or("<url>")
        ));
    }
    if matches!(auth, Auth::Basic { .. }) {
        todo.push("--username <you>".to_string());
    }
    if preset.id == "generic" {
        todo.push("--service <bamboo|github|...>".to_string());
    }

    let cred = Credential {
        service: preset.id.into(),
        url,
        description: description.to_string(),
        auth,
        hosts,
        added_by: "paste-hook".into(),
    };
    Sealed::new(found.value.clone(), &cred).save(vault, &name)?;
    store.creds.insert(name.clone(), cred);
    if let Err(e) = store.save() {
        let _ = vault.delete(&name);
        store.creds.remove(&name);
        return Err(e);
    }

    let todo = if todo.is_empty() {
        String::new()
    } else {
        format!(
            "; finish setup: ask your agent, or run `keymule edit {name} {}`",
            todo.join(" ")
        )
    };
    Ok((name, todo))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::testing::MemVault;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::load_from(dir.path().join("credentials.toml")).unwrap();
        (dir, s)
    }

    const GH: &str = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";

    fn context(o: Outcome) -> (String, String) {
        match o {
            Outcome::Context { context, notice } => (context, notice),
            other => panic!("expected context, got {other:?}"),
        }
    }

    #[test]
    fn holds_the_prompt_and_asks_in_the_terminal() {
        let (_d, mut s) = store();
        let v = MemVault::default();
        let Outcome::Block(q) = handle(&format!("use {GH} please"), "s1", &mut s, &v, 100) else {
            panic!("should block")
        };
        assert!(q.contains("y <name>") && q.contains("`github`"), "{q}");
        assert!(!q.contains(GH), "the question must not echo the secret");
        assert!(s.creds.is_empty(), "nothing is saved before the answer");
    }

    #[test]
    fn y_saves_and_sends_the_message_without_the_secret() {
        let (_d, mut s) = store();
        let v = MemVault::default();
        handle(&format!("use {GH} for launchstory"), "s1", &mut s, &v, 100);
        let (ctx, notice) = context(handle("y github-launchstory", "s1", &mut s, &v, 130));
        assert_eq!(Sealed::peek_secret(&v, "github-launchstory").unwrap(), GH);
        assert!(ctx.contains("use [saved in keymule as `github-launchstory`] for launchstory"));
        assert!(!ctx.contains(GH) && !notice.contains(GH));
        assert!(v.get(PENDING).is_err(), "the held prompt is gone");
    }

    #[test]
    fn names_and_describes_it_from_the_message() {
        let (_d, mut s) = store();
        let v = MemVault::default();
        let msg =
            format!("this is the pat for launchstory github account {GH}, keep my own one too");
        let Outcome::Block(q) = handle(&msg, "s", &mut s, &v, 1) else {
            panic!()
        };
        assert!(q.contains("`github-launchstory`"), "{q}");
        handle("y", "s", &mut s, &v, 2);
        let c = &s.creds["github-launchstory"];
        assert!(
            c.description.starts_with("GitHub token for launchstory"),
            "{}",
            c.description
        );
        assert!(c.description.contains("pat for launchstory github account"));
        assert!(!c.description.contains(GH));
        assert_eq!(
            context_hint("use it for the acme org", &[]).as_deref(),
            Some("acme")
        );
        assert_eq!(context_hint("check my actions runs", &[]), None);
    }

    #[test]
    fn scan_and_save_for_the_inline_question() {
        let (_d, mut s) = store();
        let v = MemVault::default();
        let msg = format!("the pat for the acme org is {GH} thanks");
        let scan = scan_json(&msg, &s, &v);
        let txt = scan.to_string();
        assert!(!txt.contains(GH), "scan output must not hold the secret");
        assert_eq!(scan["found"][0]["suggested"], "github-acme");
        assert_eq!(
            scan["stripped"],
            "the pat for the acme org is [secret removed by keymule] thanks"
        );
        let saved = save_json(&msg, Some("gh-acme"), &mut s, &v);
        assert_eq!(
            saved["text"],
            "the pat for the acme org is [saved in keymule as `gh-acme`] thanks"
        );
        assert!(!saved.to_string().contains(GH));
        assert_eq!(Sealed::peek_secret(&v, "gh-acme").unwrap(), GH);
        assert_eq!(scan_json(&msg, &s, &v)["found"][0]["known"], "gh-acme");
    }

    #[test]
    fn n_sends_it_as_it_was() {
        let (_d, mut s) = store();
        let v = MemVault::default();
        handle(GH, "s1", &mut s, &v, 100);
        let (ctx, _) = context(handle("n", "s1", &mut s, &v, 110));
        assert!(ctx.contains(GH));
        assert!(s.creds.is_empty());
    }

    #[test]
    fn an_answer_from_another_session_or_too_late_is_a_normal_prompt() {
        let (_d, mut s) = store();
        let v = MemVault::default();
        handle(GH, "s1", &mut s, &v, 100);
        assert_eq!(handle("y", "s2", &mut s, &v, 110), Outcome::Pass);
        handle(GH, "s1", &mut s, &v, 100);
        assert_eq!(
            handle("y", "s1", &mut s, &v, 100 + PENDING_SECS + 1),
            Outcome::Pass
        );
        assert!(s.creds.is_empty());
    }

    #[test]
    fn bamboo_token_needs_its_url_later() {
        let (_d, mut s) = store();
        let v = MemVault::default();
        handle(
            "my bamboo token is NjQ3MjE4MzQ1Njc4OkxQmPq9Zr2Wd+Hs8 check the build",
            "s",
            &mut s,
            &v,
            1,
        );
        let (_, notice) = context(handle("y", "s", &mut s, &v, 2));
        assert!(
            notice.contains("`bamboo`") && notice.contains("keymule edit bamboo --url"),
            "{notice}"
        );
        assert_eq!(s.creds["bamboo"].added_by, "paste-hook");
    }

    #[test]
    fn github_token_is_ready_to_use() {
        let (_d, mut s) = store();
        let v = MemVault::default();
        handle(GH, "s", &mut s, &v, 1);
        let (_, notice) = context(handle("yes", "s", &mut s, &v, 2));
        assert!(!notice.contains("finish setup"));
        assert_eq!(s.creds["github"].hosts, vec!["api.github.com"]);
    }

    #[test]
    fn database_url_becomes_an_mcp_secret() {
        let (_d, mut s) = store();
        let v = MemVault::default();
        let url = "postgres://app:Vq7Lp2Xk9Rt4@db.internal:5432/app";
        handle(&format!("add a pg mcp for {url}"), "s", &mut s, &v, 1);
        let (ctx, _) = context(handle("y", "s", &mut s, &v, 2));
        assert!(
            ctx.contains("`postgres`") && !ctx.contains("Vq7Lp2Xk9Rt4"),
            "{ctx}"
        );
        let b = crate::servers::Binding::load(&v, "postgres").unwrap();
        assert_eq!(b.env["DATABASE_URL"], url);
        assert!(b.commands.is_empty(), "no command is approved yet");
    }

    #[test]
    fn a_secret_already_saved_is_not_saved_twice() {
        let (_d, mut s) = store();
        let v = MemVault::default();
        handle(GH, "s", &mut s, &v, 1);
        handle("y", "s", &mut s, &v, 2);
        let Outcome::Block(q) = handle(GH, "s", &mut s, &v, 3) else {
            panic!()
        };
        assert!(q.contains("already in keymule as `github`"), "{q}");
        let (_, notice) = context(handle("y", "s", &mut s, &v, 4));
        assert!(notice.contains("already saved"));
        assert_eq!(s.creds.len(), 1);
    }

    #[test]
    fn lets_clean_prompts_and_ignore_marker_through() {
        let (_d, mut s) = store();
        let v = MemVault::default();
        assert!(run(r#"{"prompt":"check my bamboo pipeline"}"#, &mut s, &v).is_none());
        assert_eq!(
            handle("y", "s", &mut s, &v, 1),
            Outcome::Pass,
            "a lone y with nothing held"
        );
        assert_eq!(
            handle(&format!("{GH} {IGNORE_MARKER}"), "s", &mut s, &v, 1),
            Outcome::Pass
        );
        assert!(s.creds.is_empty());
    }

    #[test]
    fn hook_json_hides_the_prompt_and_carries_the_context() {
        let (_d, mut s) = store();
        let v = MemVault::default();
        let block = run(
            &format!(r#"{{"prompt":"{GH}","session_id":"s"}}"#),
            &mut s,
            &v,
        )
        .unwrap();
        let j: serde_json::Value = serde_json::from_str(&block).unwrap();
        assert_eq!(j["decision"], "block");
        assert_eq!(j["hookSpecificOutput"]["suppressOriginalPrompt"], true);
        let ok = run(r#"{"prompt":"y","session_id":"s"}"#, &mut s, &v).unwrap();
        let j: serde_json::Value = serde_json::from_str(&ok).unwrap();
        assert!(j.get("decision").is_none());
        assert!(
            j["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .unwrap()
                .contains("`github`")
        );
    }

    #[test]
    fn codex_gets_a_plain_block_then_the_context() {
        let (_d, mut s) = store();
        let v = MemVault::default();
        let held = format!(r#"{{"prompt":"{GH}","session_id":"c1","turn_id":"t"}}"#);
        let j: serde_json::Value =
            serde_json::from_str(&run_as(Agent::Codex, &held, &mut s, &v).unwrap()).unwrap();
        assert_eq!(j["decision"], "block");
        assert!(j.get("hookSpecificOutput").is_none(), "{j}");
        assert!(!j.to_string().contains(GH));
        let ok = run_as(
            Agent::Codex,
            r#"{"prompt":"y","session_id":"c1"}"#,
            &mut s,
            &v,
        )
        .unwrap();
        let j: serde_json::Value = serde_json::from_str(&ok).unwrap();
        assert_eq!(j["hookSpecificOutput"]["hookEventName"], "UserPromptSubmit");
        assert!(
            j["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .unwrap()
                .contains("`github`")
        );
        assert_eq!(s.creds.len(), 1);
    }

    #[test]
    fn cursor_saves_and_hands_back_the_message_to_send() {
        let (_d, mut s) = store();
        let v = MemVault::default();
        let cursor = |prompt: &str, s: &mut Store| -> serde_json::Value {
            let input = serde_json::json!({ "prompt": prompt, "conversation_id": "k1" });
            serde_json::from_str(&run_as(Agent::Cursor, &input.to_string(), s, &v).unwrap())
                .unwrap()
        };
        assert_eq!(cursor("check my actions runs", &mut s)["continue"], true);
        let j = cursor(&format!("use {GH} for the acme repo"), &mut s);
        assert_eq!(j["continue"], false);
        let q = j["user_message"].as_str().unwrap();
        assert!(
            q.contains("get your message back") && q.contains(IGNORE_MARKER),
            "{q}"
        );
        assert!(!q.contains(GH));
        let j = cursor("y", &mut s);
        assert_eq!(j["continue"], false);
        let back = j["user_message"].as_str().unwrap();
        assert!(
            back.contains("send this instead") && back.contains("[saved in keymule as `github"),
            "{back}"
        );
        assert!(!back.contains(GH));
        assert_eq!(s.creds.len(), 1);
        // `n` can't send it in Cursor: it says how.
        cursor(GH, &mut s);
        let j = cursor("n", &mut s);
        assert!(j["user_message"].as_str().unwrap().contains(IGNORE_MARKER));
    }
}
