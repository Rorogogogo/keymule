//! `keymule init`: in every agent it finds (Claude Code, Claude Desktop,
//! Codex, Cursor, Antigravity) register the MCP server, install the paste
//! hook where the agent has one, and tell the agent to use keymule instead
//! of asking for tokens (CLAUDE.md, AGENTS.md, GEMINI.md).

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};

const MD_START: &str = "<!-- keymule:start -->";
const MD_END: &str = "<!-- keymule:end -->";
const MD_BLOCK: &str = "## Credentials (keymule)
- API tokens and passwords are stored in keymule. Use the `keymule` MCP tools (`list_credentials`, then `http_request`) for any service that needs auth, e.g. Bamboo, GitHub Actions, GitLab, Jenkins.
- Never ask me to paste a token, password or API key into the chat. If a credential is missing, call `add_credential`: it opens a secure dialog where I type the secret. Ask me for the base URL first if the service needs one.
- When setting up an MCP server that needs a password, API key or database URL, never write it into the MCP config: call `add_mcp_secret` (I type it in a dialog), then run the server as `keymule exec <name> [--env VAR] -- <server command>`. For a remote (HTTP) server whose API key goes in a header, call `add_mcp_secret` with `header`, then run it as a stdio server: `keymule remote <name> <url>`.
- When a command-line tool needs a stored credential (gh, glab…), run it through keymule: `keymule exec <name> --env GH_TOKEN -- gh pr create …`. I approve each program and folder once; the token is hidden from its output.
- For git over HTTPS as a stored account, call `install_credential` (name, host, username, and dir: the repo folder, so gh is approved in the same dialog), then point the repo's remote at `https://<username>@<host>/<owner>/<repo>.git`.
- Never try to read secrets from the keychain, `security`, `git credential fill`, `gh auth token`, or keymule's files.";

fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("HOME is not set"))
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The other agents `init` sets up, when they're on this machine.
#[derive(Default)]
pub struct Found {
    pub claude_desktop: bool,
    pub codex: bool,
    pub cursor: bool,
    pub antigravity: bool,
}

fn on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
}

fn codex_home(home: &Path) -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"))
}

pub fn detect(home: &Path) -> Found {
    Found {
        claude_desktop: home.join("Library/Application Support/Claude").is_dir(),
        codex: codex_home(home).is_dir() || on_path("codex"),
        cursor: home.join(".cursor").is_dir() || on_path("cursor"),
        antigravity: home.join(".gemini/antigravity-cli").is_dir() || on_path("agy"),
    }
}

pub fn run(dry_run: bool) -> Result<()> {
    let exe = std::env::current_exe()?.canonicalize()?;
    let exe_s = exe.to_string_lossy().to_string();
    let home = home()?;
    let say = |done: &str, plan: &str| {
        println!(
            "{} {}",
            if dry_run { "would" } else { "✓" },
            if dry_run { plan } else { done }
        )
    };

    println!("Claude Code");
    // 1. MCP server
    let has_claude = Command::new("claude").arg("--version").output().is_ok();
    if has_claude {
        let exists = Command::new("claude")
            .args(["mcp", "get", "keymule"])
            .output()
            .is_ok_and(|o| o.status.success());
        if exists {
            println!("· MCP server `keymule` already registered");
        } else {
            if !dry_run {
                let status = Command::new("claude")
                    .args([
                        "mcp", "add", "--scope", "user", "keymule", "--", &exe_s, "mcp",
                    ])
                    .status()
                    .context("running `claude mcp add`")?;
                if !status.success() {
                    return Err(anyhow!("`claude mcp add` failed"));
                }
            }
            say(
                "registered MCP server `keymule` (user scope)",
                &format!("run: claude mcp add --scope user keymule -- {exe_s} mcp"),
            );
        }
    } else {
        println!(
            "· `claude` not found; add the MCP server manually: command {exe_s}, args [\"mcp\"]"
        );
    }

    // 2. Paste hook
    let settings = home.join(".claude").join("settings.json");
    let command = format!("{} hook prompt", shell_quote(&exe_s));
    if install_hook(&settings, &command, dry_run)? {
        say(
            &format!("installed paste-to-save hook in {}", settings.display()),
            &format!(
                "add a UserPromptSubmit hook to {}: {command}",
                settings.display()
            ),
        );
    } else {
        println!("· paste-to-save hook already installed");
    }

    let guard = json!({
        "matcher": "Bash|Read",
        "hooks": [{ "type": "command", "command": format!("{} hook bash", shell_quote(&exe_s)), "timeout": 5 }],
    });
    if install_hook_entry(&settings, "PreToolUse", guard, false, dry_run)? {
        say(
            &format!(
                "installed the token read-back guard in {}",
                settings.display()
            ),
            &format!(
                "add a PreToolUse hook to {}: keymule hook bash",
                settings.display()
            ),
        );
    } else {
        println!("· token read-back guard already installed");
    }

    // 3. Agent instructions
    let md = home.join(".claude").join("CLAUDE.md");
    if install_md(&md, dry_run)? {
        say(
            &format!("added keymule instructions to {}", md.display()),
            &format!("add or refresh the keymule section in {}", md.display()),
        );
    } else {
        println!("· {} already mentions keymule", md.display());
    }

    for line in setup_others(&home, &exe_s, &detect(&home), dry_run)? {
        println!("{line}");
    }
    if !dry_run {
        println!("\nRestart your agents so they pick up keymule, then try: keymule add bamboo");
    }
    Ok(())
}

/// Set keymule up in each agent in `found`. Returns the lines to print.
pub fn setup_others(home: &Path, exe: &str, found: &Found, dry_run: bool) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mark = |changed: bool, done: String| {
        if !changed {
            format!("· {done} (already there)")
        } else if dry_run {
            format!("would {done}")
        } else {
            format!("✓ {done}")
        }
    };
    let short = |p: &Path| match p.strip_prefix(home) {
        Ok(rel) => format!("~/{}", rel.display()),
        Err(_) => p.display().to_string(),
    };
    let hook = |agent: &str| format!("{} hook prompt --agent {agent}", shell_quote(exe));
    let guard = |agent: &str| format!("{} hook bash --agent {agent}", shell_quote(exe));

    if found.claude_desktop {
        out.push("\nClaude Desktop".into());
        let p = home.join("Library/Application Support/Claude/claude_desktop_config.json");
        let c = install_mcp_json(&p, exe, dry_run)?;
        out.push(mark(
            c,
            format!("add MCP server `keymule` to {}", short(&p)),
        ));
        out.push(
            "· Claude Desktop has no prompt hook, so a pasted secret isn't caught there".into(),
        );
    }

    if found.codex {
        out.push("\nCodex".into());
        let dir = codex_home(home);
        let p = dir.join("config.toml");
        let c = install_mcp_toml(&p, exe, dry_run)?;
        out.push(mark(
            c,
            format!("add MCP server `keymule` to {}", short(&p)),
        ));
        let p = dir.join("hooks.json");
        let entry =
            json!({ "hooks": [{ "type": "command", "command": hook("codex"), "timeout": 10 }] });
        let hooked = install_hook_entry(&p, "UserPromptSubmit", entry, false, dry_run)?;
        out.push(mark(
            hooked,
            format!("add the paste-to-save hook to {}", short(&p)),
        ));
        let entry =
            json!({ "hooks": [{ "type": "command", "command": guard("codex"), "timeout": 5 }] });
        let guarded = install_hook_entry(&p, "PreToolUse", entry, false, dry_run)?;
        out.push(mark(
            guarded,
            format!("add the token read-back guard to {}", short(&p)),
        ));
        let hooked = hooked || guarded;
        // Codex reads AGENTS.override.md instead of AGENTS.md when it's there.
        let over = dir.join("AGENTS.override.md");
        let p = if over.is_file() {
            over
        } else {
            dir.join("AGENTS.md")
        };
        let c = install_md(&p, dry_run)?;
        out.push(mark(
            c,
            format!("add keymule instructions to {}", short(&p)),
        ));
        if hooked {
            out.push("  Codex runs a new hook only once you trust it: type /hooks in Codex and trust keymule's.".into());
        }
    }

    if found.cursor {
        out.push("\nCursor".into());
        let p = home.join(".cursor/mcp.json");
        let c = install_mcp_json(&p, exe, dry_run)?;
        out.push(mark(
            c,
            format!("add MCP server `keymule` to {}", short(&p)),
        ));
        let p = home.join(".cursor/hooks.json");
        let entry = json!({ "command": hook("cursor"), "timeout": 10 });
        let c = install_hook_entry(&p, "beforeSubmitPrompt", entry, true, dry_run)?;
        out.push(mark(
            c,
            format!("add the paste-to-save hook to {}", short(&p)),
        ));
        let entry = json!({ "command": guard("cursor"), "timeout": 5 });
        let c = install_hook_entry(&p, "beforeShellExecution", entry, true, dry_run)?;
        out.push(mark(
            c,
            format!("add the token read-back guard to {}", short(&p)),
        ));
        out.push("· Cursor keeps its own rules in Customize → Rules. Paste this there so it uses keymule:".into());
        out.push(
            MD_BLOCK
                .lines()
                .skip(1)
                .map(|l| format!("    {l}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }

    if found.antigravity {
        out.push("\nAntigravity".into());
        let p = home.join(".gemini/config/mcp_config.json");
        let c = install_mcp_json(&p, exe, dry_run)?;
        out.push(mark(
            c,
            format!("add MCP server `keymule` to {}", short(&p)),
        ));
        let p = home.join(".gemini/GEMINI.md");
        let c = install_md(&p, dry_run)?;
        out.push(mark(
            c,
            format!("add keymule instructions to {}", short(&p)),
        ));
        out.push("· Antigravity has no prompt hook, so a pasted secret isn't caught there".into());
    }
    Ok(out)
}

fn read_json(path: &Path) -> Result<Value> {
    match std::fs::read_to_string(path) {
        Ok(text) if !text.trim().is_empty() => {
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
        }
        Ok(_) => Ok(json!({})),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e.into()),
    }
}

/// Write `text` to `path`, keeping a backup of what was there.
fn write_config(path: &Path, text: &str) -> Result<()> {
    if path.exists() {
        // Write through a symlink (dotfile setups) instead of replacing it.
        let path = &path.canonicalize()?;
        std::fs::copy(path, crate::adopt::backup_path(path))?;
        crate::adopt::write_atomic(path, text)
    } else {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, text)?;
        Ok(())
    }
}

/// `"mcpServers": { "keymule": { "command": exe, "args": ["mcp"] } }`, the
/// shape Claude Desktop, Cursor and Antigravity share. An entry already
/// called `keymule` is left alone.
pub fn install_mcp_json(path: &Path, exe: &str, dry_run: bool) -> Result<bool> {
    let mut root = read_json(path)?;
    let obj = root
        .as_object_mut()
        .ok_or_else(|| anyhow!("{} is not a JSON object", path.display()))?;
    let servers = obj
        .entry("mcpServers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| anyhow!("`mcpServers` in {} is not an object", path.display()))?;
    if servers.contains_key("keymule") {
        return Ok(false);
    }
    servers.insert("keymule".into(), json!({ "command": exe, "args": ["mcp"] }));
    if !dry_run {
        write_config(path, &(serde_json::to_string_pretty(&root)? + "\n"))?;
    }
    Ok(true)
}

/// `[mcp_servers.keymule]` in Codex's config.toml, the rest of the file as it was.
pub fn install_mcp_toml(path: &Path, exe: &str, dry_run: bool) -> Result<bool> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .with_context(|| format!("parsing {}", path.display()))?;
    if doc
        .get("mcp_servers")
        .and_then(|t| t.get("keymule"))
        .is_some()
    {
        return Ok(false);
    }
    if doc.get("mcp_servers").is_none() {
        let mut t = toml_edit::Table::new();
        t.set_implicit(true);
        doc.insert("mcp_servers", toml_edit::Item::Table(t));
    }
    let servers = doc["mcp_servers"]
        .as_table_mut()
        .ok_or_else(|| anyhow!("`mcp_servers` in {} is not a table", path.display()))?;
    let mut t = toml_edit::Table::new();
    t.insert("command", toml_edit::value(exe));
    let mut args = toml_edit::Array::new();
    args.push("mcp");
    t.insert("args", toml_edit::value(args));
    servers.insert("keymule", toml_edit::Item::Table(t));
    if !dry_run {
        write_config(path, &doc.to_string())?;
    }
    Ok(true)
}

fn is_keymule_hook(h: &Value, marker: &str) -> bool {
    h["command"]
        .as_str()
        .is_some_and(|c| c.contains("keymule") && c.contains(marker))
}

/// The part of a hook command that tells keymule's hooks apart.
fn marker_of(entry: &Value) -> &'static str {
    let text = entry.to_string();
    if text.contains("hook bash") {
        "hook bash"
    } else {
        "hook prompt"
    }
}

/// Add `entry` under `hooks.<event>` unless the same keymule hook is there.
/// Claude Code and Codex nest commands in `{ "hooks": [...] }` groups;
/// Cursor lists them directly and wants `"version": 1` (`cursor`).
pub fn install_hook_entry(
    path: &Path,
    event: &str,
    entry: Value,
    cursor: bool,
    dry_run: bool,
) -> Result<bool> {
    let mut root = read_json(path)?;
    let obj = root
        .as_object_mut()
        .ok_or_else(|| anyhow!("{} is not a JSON object", path.display()))?;
    if cursor {
        obj.entry("version").or_insert(json!(1));
    }
    let arr = obj
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| anyhow!("`hooks` in {} is not an object", path.display()))?
        .entry(event)
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| anyhow!("`hooks.{event}` in {} is not a list", path.display()))?;
    let marker = marker_of(&entry);
    let already = arr.iter().any(|item| {
        is_keymule_hook(item, marker)
            || item["hooks"]
                .as_array()
                .is_some_and(|hs| hs.iter().any(|h| is_keymule_hook(h, marker)))
    });
    if already {
        return Ok(false);
    }
    arr.push(entry);
    if !dry_run {
        write_config(path, &(serde_json::to_string_pretty(&root)? + "\n"))?;
    }
    Ok(true)
}

/// Claude Code's paste hook in settings.json. Returns whether it changed anything.
pub fn install_hook(path: &Path, command: &str, dry_run: bool) -> Result<bool> {
    let entry = json!({ "hooks": [{ "type": "command", "command": command, "timeout": 10 }] });
    install_hook_entry(path, "UserPromptSubmit", entry, false, dry_run)
}

pub fn install_md(path: &Path, dry_run: bool) -> Result<bool> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    if let (Some(start), Some(end)) = (existing.find(MD_START), existing.find(MD_END)) {
        // Already there: refresh it if this version's text differs.
        let wanted = format!("{MD_START}\n{MD_BLOCK}\n{MD_END}");
        let end = end + MD_END.len();
        if start > end || existing[start..end] == wanted {
            return Ok(false);
        }
        if !dry_run {
            let updated = format!("{}{wanted}{}", &existing[..start], &existing[end..]);
            std::fs::write(path, updated)?;
        }
        return Ok(true);
    }
    if !dry_run {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let sep = if existing.is_empty() || existing.ends_with("\n\n") {
            ""
        } else if existing.ends_with('\n') {
            "\n"
        } else {
            "\n\n"
        };
        std::fs::write(
            path,
            format!("{existing}{sep}{MD_START}\n{MD_BLOCK}\n{MD_END}\n"),
        )?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_install_preserves_other_settings_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(
            &p,
            r#"{"model":"opus","hooks":{"UserPromptSubmit":[{"hooks":[{"type":"command","command":"other"}]}]}}"#,
        )
        .unwrap();
        assert!(install_hook(&p, "'/bin/keymule' hook prompt", false).unwrap());
        assert!(!install_hook(&p, "'/bin/keymule' hook prompt", false).unwrap());
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["model"], "opus");
        assert_eq!(v["hooks"]["UserPromptSubmit"].as_array().unwrap().len(), 2);
        assert!(p.with_extension("json.keymule-bak").exists());
    }

    #[test]
    fn md_block_added_once() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("CLAUDE.md");
        std::fs::write(&p, "# mine\n").unwrap();
        assert!(install_md(&p, false).unwrap());
        assert!(!install_md(&p, false).unwrap());
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.starts_with("# mine\n\n<!-- keymule:start -->"));
    }

    #[test]
    fn outdated_md_block_is_replaced_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("CLAUDE.md");
        std::fs::write(
            &p,
            format!("# mine\n\n{MD_START}\nold text\n{MD_END}\n\n# after\n"),
        )
        .unwrap();
        assert!(install_md(&p, false).unwrap());
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(!text.contains("old text") && text.contains("add_credential"));
        assert!(text.starts_with("# mine\n\n") && text.ends_with("\n\n# after\n"));
        assert!(!install_md(&p, false).unwrap());
    }

    #[test]
    fn quotes_paths_with_spaces() {
        assert_eq!(shell_quote("/a b/keymule"), "'/a b/keymule'");
    }

    #[test]
    fn sets_up_every_other_agent_once() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::write(
            home.join(".codex/config.toml"),
            "# mine\nmodel = \"x\"\n\n[mcp_servers.acme]\ncommand = \"npx\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(home.join(".cursor")).unwrap();
        std::fs::write(
            home.join(".cursor/hooks.json"),
            r#"{"version":1,"hooks":{"afterFileEdit":[{"command":"./fmt.sh"}]}}"#,
        )
        .unwrap();
        let all = Found {
            claude_desktop: true,
            codex: true,
            cursor: true,
            antigravity: true,
        };
        let exe = "/opt/km/keymule";
        let lines = setup_others(home, exe, &all, false).unwrap().join("\n");
        assert!(lines.contains("type /hooks"), "{lines}");

        let codex = std::fs::read_to_string(home.join(".codex/config.toml")).unwrap();
        assert!(codex.starts_with("# mine\nmodel = \"x\"\n"), "{codex}");
        assert!(codex.contains("[mcp_servers.acme]"), "{codex}");
        let t: toml::Table = codex.parse().unwrap();
        assert_eq!(t["mcp_servers"]["keymule"]["command"].as_str(), Some(exe));
        assert_eq!(t["mcp_servers"]["keymule"]["args"][0].as_str(), Some("mcp"));
        let ch: Value = read_json(&home.join(".codex/hooks.json")).unwrap();
        assert_eq!(
            ch["hooks"]["UserPromptSubmit"][0]["hooks"][0]["command"],
            "'/opt/km/keymule' hook prompt --agent codex"
        );
        assert!(
            std::fs::read_to_string(home.join(".codex/AGENTS.md"))
                .unwrap()
                .contains(MD_START)
        );

        let cm = read_json(&home.join(".cursor/mcp.json")).unwrap();
        assert_eq!(cm["mcpServers"]["keymule"]["args"][0], "mcp");
        let chk = read_json(&home.join(".cursor/hooks.json")).unwrap();
        assert_eq!(chk["version"], 1);
        assert_eq!(chk["hooks"]["afterFileEdit"][0]["command"], "./fmt.sh");
        assert_eq!(
            chk["hooks"]["beforeSubmitPrompt"][0]["command"],
            "'/opt/km/keymule' hook prompt --agent cursor"
        );
        assert!(home.join(".cursor/hooks.json.keymule-bak").exists());

        let desk =
            read_json(&home.join("Library/Application Support/Claude/claude_desktop_config.json"))
                .unwrap();
        assert_eq!(desk["mcpServers"]["keymule"]["command"], exe);
        let ag = read_json(&home.join(".gemini/config/mcp_config.json")).unwrap();
        assert_eq!(ag["mcpServers"]["keymule"]["command"], exe);
        assert!(
            std::fs::read_to_string(home.join(".gemini/GEMINI.md"))
                .unwrap()
                .contains(MD_START)
        );

        // Again: nothing changes.
        let again = setup_others(home, exe, &all, false).unwrap();
        assert!(
            again.iter().all(|l| !l.starts_with('✓')),
            "{}",
            again.join("\n")
        );
        assert!(!again.join("\n").contains("type /hooks"));
    }

    #[test]
    fn codex_override_file_wins_and_dry_run_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::write(home.join(".codex/AGENTS.override.md"), "# team\n").unwrap();
        let codex = Found {
            codex: true,
            ..Found::default()
        };
        let lines = setup_others(home, "/k", &codex, true).unwrap();
        assert!(lines.iter().any(|l| l.starts_with("would")));
        assert!(!home.join(".codex/config.toml").exists());
        assert!(!home.join(".codex/hooks.json").exists());
        setup_others(home, "/k", &codex, false).unwrap();
        assert!(!home.join(".codex/AGENTS.md").exists());
        assert!(
            std::fs::read_to_string(home.join(".codex/AGENTS.override.md"))
                .unwrap()
                .contains(MD_START)
        );
    }

    #[test]
    fn keeps_an_existing_keymule_entry_and_writes_through_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("dotfiles-mcp.json");
        std::fs::write(&real, r#"{"mcpServers":{"other":{"command":"x"}}}"#).unwrap();
        let link = dir.path().join("mcp.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(install_mcp_json(&link, "/k", false).unwrap());
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(
            std::fs::read_to_string(&real)
                .unwrap()
                .contains("\"keymule\"")
        );
        assert!(!install_mcp_json(&link, "/other/keymule", false).unwrap());
    }
}
