//!
//! A secret pasted into a message is taken out before the message reaches the
//! model and held, sealed in the vault, until the user names it. The agent,
//! which knows the conversation, offers names and a description; the user
//! picks one or types their own, and the agent calls `save_held`. Nothing is
//! usable until then, and a held secret nobody names is thrown away.

use super::*;

pub(super) const HELD_INDEX: &str = "held:index";
pub const HELD_SECS: u64 = 15 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Held {
    pub(super) value: String,
    pub(super) label: String,
    pub(super) service: Option<String>,
    /// keymule's own guess at a name (from the message or the conversation).
    pub(super) guess: String,
    #[serde(default)]
    pub(super) why: Option<String>,
    pub(super) at: u64,
}

pub(super) fn held_ids(vault: &dyn Vault) -> Vec<String> {
    vault
        .get(HELD_INDEX)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub(super) fn set_held_ids(vault: &dyn Vault, ids: &[String]) -> Result<()> {
    if ids.is_empty() {
        let _ = vault.delete(HELD_INDEX);
        return Ok(());
    }
    vault.set(HELD_INDEX, &serde_json::to_string(ids)?)
}

pub(super) fn held_item(id: &str) -> String {
    format!("held:{id}")
}

pub(super) fn load_held(vault: &dyn Vault, id: &str) -> Option<Held> {
    serde_json::from_str(&vault.get(&held_item(id)).ok()?).ok()
}

/// Throw away held secrets older than [`HELD_SECS`] (and stray index rows).
pub(super) fn prune_held(vault: &dyn Vault, at: u64) {
    let ids = held_ids(vault);
    let keep: Vec<String> = ids
        .iter()
        .filter(|id| match load_held(vault, id) {
            Some(h) if at < h.at + HELD_SECS => true,
            _ => {
                let _ = vault.delete(&held_item(id));
                false
            }
        })
        .cloned()
        .collect();
    if keep.len() != ids.len() {
        let _ = set_held_ids(vault, &keep);
    }
}

pub(super) fn new_held_id() -> String {
    let mut b = [0u8; 4];
    let _ = getrandom::fill(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// What `hold_all` did with one secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeldNote {
    /// Held under this id until it's named.
    Held {
        id: String,
        label: String,
        masked: String,
        guess: String,
        why: Option<String>,
    },
    /// Already in keymule under this name.
    Known(String),
}

/// Take every secret out of `prompt` and hold it until it's named. Returns
/// the prompt with each one replaced, and what happened to each.
pub fn hold_all(
    prompt: &str,
    cwd: Option<&str>,
    store: &Store,
    vault: &dyn Vault,
    at: u64,
) -> (String, Vec<HeldNote>) {
    prune_held(vault, at);
    let found = detect::scan(prompt);
    let hint = name_hint(prompt, &found, cwd);
    let mut message = prompt.to_string();
    let mut notes = vec![];
    let mut ids = held_ids(vault);
    for (f, parts) in pair_aws(found) {
        // Every part leaves the message; the first marks where it was.
        let replace = |message: &mut String, with: &str| {
            for (i, p) in parts.iter().enumerate() {
                *message = message.replace(p.as_str(), if i == 0 { with } else { "" });
            }
        };
        if let Some(n) = known_as(&f, store, vault) {
            replace(&mut message, &format!("[saved in keymule as `{n}`]"));
            notes.push(HeldNote::Known(n));
            continue;
        }
        let id = new_held_id();
        let label = f.label.clone().unwrap_or_else(|| "secret".into());
        let masked = crate::doctor::mask(&f.value);
        let held = Held {
            value: f.value.clone(),
            label: label.clone(),
            service: f.service.map(str::to_string),
            guess: suggested_name(&f, store, hint.name()),
            why: hint.why.clone(),
            at,
        };
        let stored = serde_json::to_string(&held)
            .map_err(anyhow::Error::from)
            .and_then(|j| vault.set(&held_item(&id), &j));
        if stored.is_err() {
            replace(&mut message, "[secret removed by keymule]");
            continue;
        }
        ids.push(id.clone());
        replace(
            &mut message,
            &format!("[{label} held by keymule ({masked}), id {id}]"),
        );
        notes.push(HeldNote::Held {
            id,
            label,
            masked,
            guess: held.guess,
            why: held.why,
        });
    }
    let _ = set_held_ids(vault, &ids);
    (message, notes)
}

/// An AWS access key ID and its secret in the same message become one
/// secret ("ID\nSECRET", the `aws` kind); each comes with the text to take
/// out of the message.
fn pair_aws(found: Vec<detect::Found>) -> Vec<(detect::Found, Vec<String>)> {
    let id = found
        .iter()
        .position(|f| f.label.as_deref() == Some("AWS access key ID"));
    let secret = found.iter().position(|f| f.service == Some("aws"));
    let (Some(i), Some(j)) = (id, secret) else {
        return found
            .into_iter()
            .map(|f| {
                let parts = vec![f.value.clone()];
                (f, parts)
            })
            .collect();
    };
    let pair = detect::Found {
        value: format!("{}\n{}", found[i].value, found[j].value),
        service: Some("aws"),
        label: Some("AWS access keys".into()),
        confident: true,
    };
    let parts = vec![found[i].value.clone(), found[j].value.clone()];
    let mut out = vec![(pair, parts)];
    out.extend(
        found
            .into_iter()
            .enumerate()
            .filter(|(k, _)| *k != i && *k != j)
            .map(|(_, f)| {
                let parts = vec![f.value.clone()];
                (f, parts)
            }),
    );
    out
}

/// What the agent is told about held secrets: ask the user, then save.
pub fn hold_context(notes: &[HeldNote]) -> String {
    let held: Vec<String> = notes
        .iter()
        .filter_map(|n| match n {
            HeldNote::Held {
                id,
                label,
                masked,
                guess,
                why,
            } => Some(format!(
                "id `{id}`: a {label} ({masked}); keymule's own guess at a name: `{guess}`{}",
                why.as_deref()
                    .map(|w| format!(" ({w})"))
                    .unwrap_or_default()
            )),
            HeldNote::Known(_) => None,
        })
        .collect();
    let known: Vec<String> = notes
        .iter()
        .filter_map(|n| match n {
            HeldNote::Known(name) => Some(format!("`{name}`")),
            _ => None,
        })
        .collect();
    let mut out = String::new();
    if !held.is_empty() {
        out.push_str(&format!(
            "keymule took {} out of the user's message before it reached you and is holding \
             {} until the user names {}: {}.\n\nBefore anything else, ask the user what to save \
             {} as, with your question tool if you have one: offer two or three names that fit \
             this conversation (lowercase with hyphens, service first, e.g. `github-homebrew-tap`), \
             each with a one-line description of what it's for, your best one first marked \
             (Recommended), and \"Don't save it\". The user can also type their own name. Then \
             call keymule's save_held tool with the id, the chosen name and a description that \
             says what it's for, or discard_held if they don't want it saved. You never see the \
             value; never ask the user for it. Unnamed secrets are thrown away after {} minutes.",
            if held.len() == 1 { "a secret" } else { "secrets" },
            if held.len() == 1 { "it" } else { "them" },
            if held.len() == 1 { "it" } else { "them" },
            held.join("; "),
            if held.len() == 1 { "it" } else { "each" },
            HELD_SECS / 60,
        ));
    }
    if !known.is_empty() {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&format!(
            "The message also had secrets keymule already holds: {}. Use them through keymule by \
             those names and never ask the user for the values.",
            known.join(", ")
        ));
    }
    out
}

/// For the Claude Code mod: hold the secrets in the prompt.
pub fn hold_json(
    prompt: &str,
    cwd: Option<&str>,
    store: &Store,
    vault: &dyn Vault,
) -> serde_json::Value {
    let (text, notes) = hold_all(prompt, cwd, store, vault, now());
    let held = notes
        .iter()
        .filter(|n| matches!(n, HeldNote::Held { .. }))
        .count();
    serde_json::json!({
        "text": text,
        "held": held,
        "context": hold_context(&notes),
    })
}

/// Save a held secret under the name the user chose.
pub fn save_held(
    id: &str,
    name: &str,
    description: &str,
    store: &mut Store,
    vault: &dyn Vault,
    at: u64,
) -> Result<String> {
    prune_held(vault, at);
    let h = load_held(vault, id).ok_or_else(|| {
        anyhow::anyhow!("no held secret with id {id} (it was saved, thrown away, or timed out)")
    })?;
    crate::store::validate_name(name)?;
    if store.creds.contains_key(name) || store.servers.contains_key(name) {
        anyhow::bail!("\"{name}\" is taken; pick another name");
    }
    let service = h.service.as_deref().and_then(|s| {
        if s == detect::DATABASE {
            Some(detect::DATABASE)
        } else {
            presets::PRESETS.iter().find(|p| p.id == s).map(|p| p.id)
        }
    });
    let found = detect::Found {
        value: h.value.clone(),
        service,
        label: Some(h.label.clone()),
        confident: true,
    };
    let note = if service == Some(detect::DATABASE) {
        save_database(&found, name, store, vault)?;
        format!("Saved the {} as MCP server secret \"{name}\".", h.label)
    } else {
        let (saved, todo) = save(&found, name, description, store, vault)?;
        format!("Saved the {} as \"{saved}\"{todo}.", h.label)
    };
    let _ = vault.delete(&held_item(id));
    let ids: Vec<String> = held_ids(vault).into_iter().filter(|x| x != id).collect();
    let _ = set_held_ids(vault, &ids);
    crate::http::audit(name, "SAVE-HELD", &h.label, 0);
    Ok(note)
}

/// Throw a held secret away.
pub fn discard_held(id: &str, vault: &dyn Vault) -> Result<String> {
    let h = load_held(vault, id).ok_or_else(|| anyhow::anyhow!("no held secret with id {id}"))?;
    let _ = vault.delete(&held_item(id));
    let ids: Vec<String> = held_ids(vault).into_iter().filter(|x| x != id).collect();
    let _ = set_held_ids(vault, &ids);
    Ok(format!("Threw away the held {}; it wasn't saved.", h.label))
}

/// The held secrets, masked, for list_credentials.
pub fn list_held(vault: &dyn Vault) -> Vec<serde_json::Value> {
    let at = now();
    prune_held(vault, at);
    held_ids(vault)
        .iter()
        .filter_map(|id| {
            let h = load_held(vault, id)?;
            Some(serde_json::json!({
                "id": id,
                "what": h.label,
                "masked": crate::doctor::mask(&h.value),
                "guess": h.guess,
                "minutes_left": (h.at + HELD_SECS).saturating_sub(at).div_ceil(60),
            }))
        })
        .collect()
}
