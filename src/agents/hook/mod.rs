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
use crate::dialog::PasteChoice;
use crate::presets::{self, AuthKind};
use crate::store::{Auth, Credential, Sealed, Store, Vault, validate_name};

mod conversation;
mod held;
mod naming;
mod save;
#[cfg(test)]
mod tests;

use conversation::*;
pub use conversation::{conversation_from_transcript, mod_input, set_conversation};
#[cfg(test)]
use held::*;
pub use held::{discard_held, hold_json, list_held, save_held};
use naming::*;
use save::*;

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
    /// The folder the agent works in; names a secret's project when the
    /// message doesn't.
    #[serde(default)]
    cwd: Option<String>,
    /// The session's transcript (Claude Code, Codex): the conversation says
    /// what a pasted secret is for when the message itself doesn't.
    #[serde(default)]
    transcript_path: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Pending {
    session: String,
    at: u64,
    prompt: String,
    #[serde(default)]
    cwd: Option<String>,
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

/// Asks what to do with a pasted secret (a dialog in real runs).
pub type Ask<'a> = &'a dyn Fn(&str) -> Option<PasteChoice>;

#[cfg(test)]
fn run(stdin: &str, store: &mut Store, vault: &dyn Vault) -> Option<String> {
    run_as(Agent::Claude, stdin, store, vault, &|_| None)
}

/// Returns the JSON to print on stdout, or None to let the prompt through.
pub fn run_as(
    agent: Agent,
    stdin: &str,
    store: &mut Store,
    vault: &dyn Vault,
    ask: Ask,
) -> Option<String> {
    let input: HookInput = serde_json::from_str(stdin).ok()?;
    set_conversation(
        input
            .transcript_path
            .as_deref()
            .map(conversation_from_transcript)
            .unwrap_or_default(),
    );
    let outcome = handle_with(
        agent,
        &input.prompt,
        &input.session_id,
        input.cwd.as_deref(),
        store,
        vault,
        now(),
        ask,
    );
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
    handle_as(Agent::Claude, prompt, session, None, store, vault, at)
}

#[cfg(test)]
pub fn handle_as(
    agent: Agent,
    prompt: &str,
    session: &str,
    cwd: Option<&str>,
    store: &mut Store,
    vault: &dyn Vault,
    at: u64,
) -> Outcome {
    handle_with(agent, prompt, session, cwd, store, vault, at, &|_| None)
}

/// [`handle_as`], asking with `ask` (Codex and Cursor only: Claude Code
/// asks inline through the mod) and saving straight away under allow-all.
#[allow(clippy::too_many_arguments)]
pub fn handle_with(
    agent: Agent,
    prompt: &str,
    session: &str,
    cwd: Option<&str>,
    store: &mut Store,
    vault: &dyn Vault,
    at: u64,
    ask: Ask,
) -> Outcome {
    if let Some(reply) = parse_reply(prompt)
        && let Some(p) = take_pending(vault, session, at)
    {
        return answer(agent, &p.prompt, p.cwd.as_deref(), reply, store, vault);
    }
    if prompt.contains(IGNORE_MARKER) {
        return Outcome::Pass;
    }
    let found = detect::scan(prompt);
    if found.is_empty() {
        return Outcome::Pass;
    }
    let allow_all = cwd.is_some_and(|d| {
        crate::trust::skips(vault, std::path::Path::new(d), "save a pasted secret")
    });
    let choice = if allow_all {
        Some(PasteChoice::Save)
    } else if agent == Agent::Claude {
        None
    } else {
        ask(&choice_question(agent, &found))
    };
    match choice {
        Some(PasteChoice::SendAsIs) => return Outcome::Pass,
        Some(PasteChoice::DontSend) => {
            return Outcome::Block(
                "Not sent. Your message had a secret, so keymule kept it back; nothing reached \
                 the agent."
                    .into(),
            );
        }
        Some(PasteChoice::Save) => {
            return save_now(agent, prompt, session, cwd, store, vault, at, allow_all);
        }
        None => {} // no dialog: ask in the conversation instead
    }
    let held = serde_json::to_string(&Pending {
        session: session.to_string(),
        at,
        prompt: prompt.to_string(),
        cwd: cwd.map(str::to_string),
    })
    .unwrap_or_default();
    if let Err(e) = vault.set(PENDING, &held) {
        // Can't hold it for an answer: keep the old behaviour and save now.
        let (_, notes) = save_all(prompt, cwd, None, store, vault);
        return Outcome::Block(format!(
            "keymule couldn't hold your message ({e:#}), so it saved the secret and stopped the message.\nkeymule saved {}.\nSend your message again without the secret.",
            notes.join(", ")
        ));
    }
    Outcome::Block(question(agent, prompt, cwd, &found, store, vault))
}

fn agent_name(agent: Agent) -> &'static str {
    match agent {
        Agent::Claude => "Claude",
        Agent::Codex => "Codex",
        Agent::Cursor => "Cursor",
    }
}

/// The dialog's question.
fn choice_question(agent: Agent, found: &[detect::Found]) -> String {
    let who = agent_name(agent);
    let what = if found.len() == 1 {
        format!("a {}", describe(&found[0]))
    } else {
        let list: Vec<String> = found.iter().map(describe).collect();
        format!("{} secrets: {}", found.len(), list.join(", "))
    };
    let then = if agent == Agent::Cursor {
        "you get your message back to send"
    } else {
        "type y to send your message"
    };
    format!(
        "Your message to {who} has {what}.\n\n\
         Save it in keymule and {who} sees only its name. Then {then}."
    )
}

/// Save the secrets now. The agent can't take a changed message, so the
/// original is held back: `y` sends it with the names in place (Cursor:
/// here's the message to send instead).
#[allow(clippy::too_many_arguments)]
fn save_now(
    agent: Agent,
    prompt: &str,
    session: &str,
    cwd: Option<&str>,
    store: &mut Store,
    vault: &dyn Vault,
    at: u64,
    allow_all: bool,
) -> Outcome {
    let (message, notes) = save_all(prompt, cwd, None, store, vault);
    let why = if allow_all {
        " (allow all is on here, so it didn't ask)"
    } else {
        ""
    };
    if agent == Agent::Cursor {
        return Outcome::Block(format!(
            "keymule saved {}{why}. Cursor can't send a changed message, so send this instead:\n\n{message}",
            notes.join(", ")
        ));
    }
    let held = serde_json::to_string(&Pending {
        session: session.to_string(),
        at,
        prompt: prompt.to_string(),
        cwd: cwd.map(str::to_string),
    })
    .unwrap_or_default();
    let next = if vault.set(PENDING, &held).is_ok() {
        "Type y and press Enter to send your message with that name in place of the secret."
    } else {
        "Send your message again without the secret."
    };
    Outcome::Block(format!(
        "keymule saved {}{why}; nothing reached the agent. {next}",
        notes.join(", ")
    ))
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
    cwd: Option<&str>,
    found: &[detect::Found],
    store: &Store,
    vault: &dyn Vault,
) -> String {
    // Cursor can't send a changed message, so `y` there saves and hands
    // back the message to send again.
    let resend = agent == Agent::Cursor;
    let hint = name_hint(prompt, found, cwd);
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
                let name = suggested_name(f, store, hint.name());
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
    cwd: Option<&str>,
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
    let (message, notes) = save_all(prompt, cwd, typed.as_deref(), store, vault);
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
    cwd: Option<&str>,
    typed: Option<&str>,
    store: &mut Store,
    vault: &dyn Vault,
) -> (String, Vec<String>) {
    let found = detect::scan(prompt);
    let hint = name_hint(prompt, &found, cwd);
    let mut notes = Vec::new();
    let mut message = prompt.to_string();
    for f in &found {
        let typed = if found.len() == 1 { typed } else { None };
        let saved = match known_as(f, store, vault) {
            Some(n) if typed.is_none_or(|t| t == n) => Ok((n, "already saved".to_string())),
            _ => {
                let name = pick_name(typed, suggested_name(f, store, hint.name()), store);
                if f.service == Some(detect::DATABASE) {
                    save_database(f, &name, store, vault)
                        .map(|n| (n, "an MCP server secret".into()))
                } else {
                    let desc = description(f, prompt, &found, &hint);
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
pub fn scan_json(
    prompt: &str,
    cwd: Option<&str>,
    store: &Store,
    vault: &dyn Vault,
) -> serde_json::Value {
    let found = if prompt.contains(IGNORE_MARKER) {
        vec![]
    } else {
        detect::scan(prompt)
    };
    let hint = name_hint(prompt, &found, cwd);
    let mut stripped = prompt.to_string();
    let items: Vec<serde_json::Value> = found
        .iter()
        .map(|f| {
            stripped = stripped.replace(&f.value, "[secret removed by keymule]");
            let known = known_as(f, store, vault);
            let suggested = match &known {
                Some(n) => n.clone(),
                None => suggested_name(f, store, hint.name()),
            };
            serde_json::json!({
                "label": f.label.as_deref().unwrap_or("secret"),
                "masked": crate::doctor::mask(&f.value),
                "known": known,
                "suggested": suggested,
                "why": hint.why,
            })
        })
        .collect();
    // Allow all on in this folder: the mod saves without asking.
    let allowed = !found.is_empty()
        && cwd.is_some_and(|d| {
            crate::trust::skips(vault, std::path::Path::new(d), "save a pasted secret")
        });
    serde_json::json!({
        "found": items,
        "stripped": stripped,
        "ignore_marker": IGNORE_MARKER,
        "allowed": allowed,
    })
}

/// `keymule hook save`: save the secrets in a prompt and return it with each
/// swapped for its keymule name, and a note for the model.
pub fn save_json(
    prompt: &str,
    cwd: Option<&str>,
    name: Option<&str>,
    store: &mut Store,
    vault: &dyn Vault,
) -> serde_json::Value {
    let (text, notes) = save_all(prompt, cwd, name, store, vault);
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
