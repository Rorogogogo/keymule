//! The recent conversation a prompt belongs to: read from the session's
//! transcript (Claude Code, Codex) or passed in by the Claude Code mod, and
//! used only to name secrets.

thread_local! {
    /// The last few messages of the conversation the prompt belongs to, set
    /// once per hook run by its entry point.
    pub(super) static CONVERSATION: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// Remember the recent conversation for naming secrets in this run.
pub fn set_conversation(text: String) {
    CONVERSATION.with(|c| *c.borrow_mut() = text);
}

/// How much of the conversation keymule looks at: the recent end of it.
pub(super) const CONVERSATION_CHARS: usize = 6000;

/// The recent conversation's text from a transcript (Claude Code's or
/// Codex's JSON lines): user and assistant text only, not tool calls or
/// their output, newest last.
pub fn conversation_from_transcript(path: &str) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else {
        return String::new();
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = f.seek(SeekFrom::Start(len.saturating_sub(512 * 1024)));
    let mut raw = Vec::new();
    let _ = f.read_to_end(&mut raw);
    let raw = String::from_utf8_lossy(&raw);
    let mut parts: Vec<String> = vec![];
    for line in raw.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        // Claude Code: {"message": {"role", "content"}}; Codex:
        // {"payload": {"type": "message", "role", "content"}}.
        let msg = if v["message"].is_object() {
            &v["message"]
        } else if v["payload"]["type"] == "message" {
            &v["payload"]
        } else {
            continue;
        };
        if !matches!(msg["role"].as_str(), Some("user" | "assistant")) {
            continue;
        }
        let text = match &msg["content"] {
            serde_json::Value::String(t) => t.clone(),
            serde_json::Value::Array(blocks) => blocks
                .iter()
                .filter(|b| {
                    matches!(
                        b["type"].as_str(),
                        Some("text" | "input_text" | "output_text")
                    )
                })
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        };
        if !text.trim().is_empty() {
            parts.push(text);
        }
    }
    recent(&parts)
}

/// What the Claude Code mod sends `keymule hook scan` / `save`: the message,
/// or `{"keymule_input": 1, "prompt": …, "messages": [...]}` with the recent
/// conversation too. Returns the message and remembers the conversation.
pub fn mod_input(stdin: &str) -> String {
    let v = match serde_json::from_str::<serde_json::Value>(stdin) {
        Ok(v) if v.get("keymule_input").is_some() => v,
        _ => return stdin.to_string(),
    };
    if let Some(m) = v["messages"].as_array() {
        set_conversation(conversation_from_messages(m));
    }
    v["prompt"].as_str().unwrap_or_default().to_string()
}

/// The conversation the Claude Code mod passes in: messages in API form.
pub fn conversation_from_messages(messages: &[serde_json::Value]) -> String {
    let parts: Vec<String> = messages
        .iter()
        .filter_map(|m| match &m["content"] {
            serde_json::Value::String(t) => Some(t.clone()),
            serde_json::Value::Array(blocks) => Some(
                blocks
                    .iter()
                    .filter(|b| b["type"] == "text")
                    .filter_map(|b| b["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            _ => None,
        })
        .filter(|t| !t.trim().is_empty())
        .collect();
    recent(&parts)
}

/// The newest messages, up to [`CONVERSATION_CHARS`].
pub(super) fn recent(parts: &[String]) -> String {
    let mut out: Vec<&str> = vec![];
    let mut total = 0;
    for p in parts.iter().rev().take(12) {
        if total + p.len() > CONVERSATION_CHARS && !out.is_empty() {
            break;
        }
        total += p.len();
        out.push(p);
    }
    out.reverse();
    let joined = out.join("\n\n");
    let cut = joined.len().saturating_sub(CONVERSATION_CHARS);
    let cut = (cut..=joined.len())
        .find(|&i| joined.is_char_boundary(i))
        .unwrap_or(joined.len());
    joined[cut..].to_string()
}
