//! `keymule hook bash`: an agent's shell commands (and file reads) pass
//! through here before they run (Claude Code and Codex `PreToolUse`, Cursor
//! `beforeShellExecution`). It stops the commands that read a token back out
//! of git's or gh's store, the macOS keychain, or their plain-text files.
//!
//! It always blocks them, whether or not keymule installed anything there:
//! the hook would otherwise need to read keymule's store on every command,
//! and an agent rarely has a good reason to print a stored password. It
//! matches the command text, so it's a guard rail, not a wall: a determined
//! script can get around it.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Value, json};

use crate::hook::Agent;

static RULES: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    [
        (
            r"\bgit\b.*\bcredential\s+fill\b",
            "`git credential fill` prints a stored password",
        ),
        (
            r"\bgit[\s-]+credential-[\w-]+\s+get\b",
            "asking a git credential helper directly prints a stored password",
        ),
        (
            r"\bsecurity\s+find-(?:internet|generic)-password\b.*\s-[A-Za-z]*[wg]",
            "`security find-…-password -w/-g` prints a keychain password",
        ),
        (
            r"\bsecurity\s+dump-keychain\b.*\s-[A-Za-z]*d",
            "`security dump-keychain -d` prints keychain passwords",
        ),
        (
            r"\bgh\s+auth\s+token\b",
            "`gh auth token` prints gh's token",
        ),
        (
            r"\bgh\s+auth\s+status\b.*(?:--show-token|\s-t\b)",
            "`gh auth status --show-token` prints gh's token",
        ),
        (
            r"\.git-credentials\b",
            "~/.git-credentials holds passwords in plain text",
        ),
        (
            r"gh/hosts\.ya?ml\b",
            "gh's hosts.yml can hold its token in plain text",
        ),
    ]
    .into_iter()
    .map(|(re, why)| (Regex::new(re).expect("valid rule"), why))
    .collect()
});

/// Why `command` is blocked, or None to let it run.
pub fn blocked(command: &str) -> Option<&'static str> {
    RULES
        .iter()
        .find(|(re, _)| re.is_match(command))
        .map(|(_, why)| *why)
}

/// The shell command in a hook's input: `tool_input.command` (a string,
/// or Codex's argv list) or Cursor's top-level `command`; for a file tool,
/// the path it reads.
fn command_of(input: &Value) -> Option<String> {
    let c = input
        .get("tool_input")
        .and_then(|t| {
            t.get("command")
                .or_else(|| t.get("cmd"))
                .or_else(|| t.get("file_path"))
        })
        .or_else(|| input.get("command"))?;
    match c {
        Value::String(s) => Some(s.clone()),
        Value::Array(a) => Some(
            a.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" "),
        ),
        _ => None,
    }
}

/// The JSON to print, or None to let the command run.
pub fn run(agent: Agent, stdin: &str) -> Option<String> {
    let input: Value = serde_json::from_str(stdin).ok()?;
    let why = blocked(&command_of(&input)?)?;
    let reason = format!(
        "keymule blocked this: {why}. Use the token through keymule instead: http_request, \
         `keymule exec <name> --env VAR -- <tool>`, or install_credential for git."
    );
    let out = match agent {
        Agent::Claude => json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": reason,
            }
        }),
        Agent::Codex => json!({ "decision": "block", "reason": reason }),
        Agent::Cursor => json!({
            "permission": "deny",
            "user_message": format!("keymule blocked a command: {why}."),
            "agent_message": reason,
        }),
    };
    Some(out.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_reading_tokens_back() {
        for c in [
            "printf 'protocol=https\\nhost=github.com\\n\\n' | git credential fill",
            "git -c credential.helper=osxkeychain credential fill",
            "git credential-osxkeychain get",
            "echo host=github.com | git-credential-osxkeychain get",
            "security find-internet-password -s github.com -w",
            "security find-generic-password -a me -gs svc",
            "security dump-keychain -d login.keychain",
            "gh auth token",
            "gh auth token --hostname github.com",
            "gh auth status --show-token",
            "cat ~/.git-credentials",
            "cat ~/.config/gh/hosts.yml",
        ] {
            assert!(blocked(c).is_some(), "{c}");
        }
    }

    #[test]
    fn lets_everyday_commands_through() {
        for c in [
            "git push origin main",
            "git credential approve",
            "git config credential.helper",
            "gh pr create --title x",
            "gh auth status",
            "security find-internet-password -s github.com",
            "keymule exec github --env GH_TOKEN -- gh pr list",
            "ls ~/.config/gh",
        ] {
            assert!(blocked(c).is_none(), "{c}");
        }
    }

    #[test]
    fn answers_in_each_agents_shape() {
        let claude = r#"{"tool_name":"Bash","tool_input":{"command":"gh auth token"}}"#;
        let j: Value = serde_json::from_str(&run(Agent::Claude, claude).unwrap()).unwrap();
        assert_eq!(j["hookSpecificOutput"]["permissionDecision"], "deny");
        let codex =
            r#"{"tool_name":"Bash","tool_input":{"command":["bash","-lc","git credential fill"]}}"#;
        let j: Value = serde_json::from_str(&run(Agent::Codex, codex).unwrap()).unwrap();
        assert_eq!(j["decision"], "block");
        let cursor = r#"{"command":"cat ~/.git-credentials","cwd":"/x"}"#;
        let j: Value = serde_json::from_str(&run(Agent::Cursor, cursor).unwrap()).unwrap();
        assert_eq!(j["permission"], "deny");
        assert!(
            run(
                Agent::Claude,
                r#"{"tool_name":"Bash","tool_input":{"command":"ls"}}"#
            )
            .is_none()
        );
        assert!(
            run(
                Agent::Claude,
                r#"{"tool_name":"Read","tool_input":{"file_path":"x"}}"#
            )
            .is_none()
        );
        let read =
            r#"{"tool_name":"Read","tool_input":{"file_path":"/Users/me/.git-credentials"}}"#;
        assert!(run(Agent::Claude, read).is_some());
    }
}
