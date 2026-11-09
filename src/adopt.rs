//! `keymule adopt`: move secrets that MCP server entries keep in plain text
//! (env vars, inline `VAR=value` commands, arguments) into the keychain, and
//! rewrite each entry to `keymule exec <name> -- <its original command>`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};

use crate::detect;
use crate::doctor::{is_secret_name, mask};
use crate::servers::{Binding, placeholder};
use crate::store::{Store, Vault, validate_name};

pub struct Options {
    pub home: PathBuf,
    /// Project directory, to report (not rewrite) its .mcp.json.
    pub dir: PathBuf,
    /// Path of the keymule binary to put into configs.
    pub exe: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Json,
    Toml,
}

struct Config {
    path: PathBuf,
    display: String,
    format: Format,
    /// A project's own file (likely committed), reported but not rewritten.
    project: bool,
    text: String,
    /// Parsed contents (TOML converted to JSON for reading).
    value: Value,
}

/// One config entry to rewrite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    config: usize,
    /// Keys leading to the server's object, e.g. ["projects", "/x", "mcpServers", "gh"].
    keys: Vec<String>,
    pub place: String,
    /// Program and arguments, with `{keymule:N}` placeholders for secrets.
    argv: Vec<String>,
    secret_env: BTreeMap<String, String>,
    arg_secrets: BTreeMap<String, String>,
    /// The env object to leave in the config (non-secret vars).
    keep_env: Map<String, Value>,
    had_env: bool,
    pub binding: String,
}

#[derive(Debug)]
pub struct Skipped {
    pub place: String,
    pub reason: &'static str,
}

pub struct Plan {
    configs: Vec<Config>,
    pub targets: Vec<Target>,
    pub skipped: Vec<Skipped>,
    /// Bindings to save: existing ones with new commands, and new ones.
    bindings: BTreeMap<String, Binding>,
    exe: String,
}

// ---- finding servers --------------------------------------------------------

fn load_configs(opts: &Options) -> Vec<Config> {
    let home = &opts.home;
    let files = [
        (home.join(".claude.json"), Format::Json, false),
        (
            home.join(".claude").join("settings.json"),
            Format::Json,
            false,
        ),
        (home.join(".cursor").join("mcp.json"), Format::Json, false),
        (
            home.join("Library/Application Support/Claude/claude_desktop_config.json"),
            Format::Json,
            false,
        ),
        (home.join(".codex").join("config.toml"), Format::Toml, false),
        (opts.dir.join(".mcp.json"), Format::Json, true),
        (
            opts.dir.join(".cursor").join("mcp.json"),
            Format::Json,
            true,
        ),
    ];
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for (path, format, project) in files {
        let Ok(canon) = path.canonicalize() else {
            continue;
        };
        if seen.contains(&canon) {
            continue;
        }
        seen.push(canon.clone());
        let Ok(text) = std::fs::read_to_string(&canon) else {
            continue;
        };
        let value = match format {
            Format::Json => serde_json::from_str::<Value>(&text).ok(),
            Format::Toml => toml::from_str::<toml::Value>(&text)
                .ok()
                .and_then(|v| serde_json::to_value(v).ok()),
        };
        let Some(value) = value else {
            continue;
        };
        out.push(Config {
            display: display(home, &path),
            path: canon,
            format,
            project,
            text,
            value,
        });
    }
    out
}

/// (keys to the server object, label) for every MCP server in a config.
fn servers_in(value: &Value) -> Vec<(Vec<String>, String)> {
    let mut out = Vec::new();
    let mut push = |prefix: Vec<String>, label: String, map: &Value| {
        if let Some(obj) = map.as_object() {
            for name in obj.keys() {
                let mut keys = prefix.clone();
                keys.push(name.clone());
                out.push((keys, format!("{label}{name}")));
            }
        }
    };
    for key in ["mcpServers", "mcp_servers"] {
        if let Some(m) = value.get(key) {
            push(vec![key.into()], String::new(), m);
        }
    }
    if let Some(projects) = value.get("projects").and_then(Value::as_object) {
        for (dir, p) in projects {
            if let Some(m) = p.get("mcpServers") {
                push(
                    vec!["projects".into(), dir.clone(), "mcpServers".into()],
                    format!("project {}: ", tilde(dir)),
                    m,
                );
            }
        }
    }
    out
}

fn get_path<'a>(value: &'a Value, keys: &[String]) -> Option<&'a Value> {
    keys.iter().try_fold(value, |v, k| v.get(k))
}

fn is_secret(key: &str, value: &str) -> bool {
    (is_secret_name(key) && detect::plausible_value(value.trim()))
        || !detect::scan_deep(value).is_empty()
}

enum Analysis {
    Nothing,
    Skip(&'static str),
    Adopt(Box<Target>),
}

/// Characters that make a command line more than "VAR=x program args".
const SHELL_CHARS: &[char] = &['\'', '"', '$', '`', '\\', '|', ';', '&', '<', '>', '(', ')'];

fn analyze(server: &Value) -> Analysis {
    let Some(obj) = server.as_object() else {
        return Analysis::Nothing;
    };
    let args: Option<Vec<String>> = match obj.get("args") {
        None => Some(Vec::new()),
        Some(Value::Array(a)) => a.iter().map(|v| v.as_str().map(String::from)).collect(),
        Some(_) => None,
    };
    if is_keymule_exec(obj, args.as_deref().unwrap_or_default()) {
        return Analysis::Nothing;
    }

    let env = obj
        .get("env")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let has_secret_env = env
        .iter()
        .any(|(k, v)| v.as_str().is_some_and(|v| is_secret(k, v)));

    if obj.get("url").is_some() {
        let headers = obj.get("headers").and_then(Value::as_object);
        let secret_header = headers.is_some_and(|h| {
            h.iter()
                .any(|(k, v)| v.as_str().is_some_and(|v| is_secret_header(k, v)))
        });
        return if secret_header || has_secret_env {
            Analysis::Skip("HTTP server: keymule can't hold its headers yet")
        } else {
            Analysis::Nothing
        };
    }

    let Some(command) = obj.get("command").and_then(Value::as_str) else {
        return Analysis::Nothing;
    };
    let Some(args) = args else {
        return Analysis::Nothing;
    };

    let mut inline_env = Vec::new();
    let argv: Vec<String> = if args.is_empty() && command.trim().contains(char::is_whitespace) {
        if command.contains(SHELL_CHARS) {
            return if is_secret("", command) || has_secret_env {
                Analysis::Skip("the command is a shell line keymule can't rewrite safely")
            } else {
                Analysis::Nothing
            };
        }
        let mut tokens = command.split_whitespace().peekable();
        while let Some(t) = tokens.peek() {
            match t.split_once('=') {
                Some((k, v)) if is_env_name(k) => {
                    inline_env.push((k.to_string(), v.to_string()));
                    tokens.next();
                }
                _ => break,
            }
        }
        tokens.map(String::from).collect()
    } else {
        std::iter::once(command.to_string()).chain(args).collect()
    };
    if argv.is_empty() {
        return Analysis::Nothing;
    }

    let mut secret_env = BTreeMap::new();
    let mut keep_env = Map::new();
    for (k, v) in env.iter() {
        match v.as_str() {
            Some(s) if is_secret(k, s) => {
                secret_env.insert(k.clone(), s.to_string());
            }
            _ => {
                keep_env.insert(k.clone(), v.clone());
            }
        }
    }
    for (k, v) in inline_env {
        if is_secret(&k, &v) {
            secret_env.insert(k, v);
        } else {
            keep_env.insert(k, Value::String(v));
        }
    }

    // Secrets inside arguments become placeholders.
    let mut arg_secrets: BTreeMap<String, String> = BTreeMap::new();
    let mut by_value: BTreeMap<String, String> = BTreeMap::new();
    let mut new_argv = vec![argv[0].clone()];
    for arg in &argv[1..] {
        let mut arg = arg.clone();
        let mut hits: Vec<String> = detect::scan_deep(&arg)
            .into_iter()
            .filter(|f| f.confident)
            .map(|f| f.value)
            .collect();
        hits.sort_by_key(|h| std::cmp::Reverse(h.len()));
        for secret in hits {
            let ph = by_value
                .entry(secret.clone())
                .or_insert_with(|| placeholder(arg_secrets.len() + 1))
                .clone();
            arg_secrets.insert(ph.clone(), secret.clone());
            arg = arg.replace(&secret, &ph);
        }
        new_argv.push(arg);
    }

    if secret_env.is_empty() && arg_secrets.is_empty() {
        return Analysis::Nothing;
    }
    Analysis::Adopt(Box::new(Target {
        config: 0,
        keys: Vec::new(),
        place: String::new(),
        argv: new_argv,
        secret_env,
        arg_secrets,
        keep_env,
        had_env: obj.contains_key("env"),
        binding: String::new(),
    }))
}

fn is_secret_header(key: &str, value: &str) -> bool {
    let k = key.to_ascii_lowercase();
    let v = value.trim().trim_start_matches("Bearer ").trim();
    ((k == "authorization" || k.contains("key") || k.contains("token") || is_secret_name(key))
        && detect::plausible_value(v))
        || !detect::scan_deep(value).is_empty()
}

fn is_env_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_keymule_exec(obj: &Map<String, Value>, args: &[String]) -> bool {
    let cmd = obj.get("command").and_then(Value::as_str).unwrap_or("");
    let program = Path::new(cmd)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();
    (program == "keymule" || program == "keymule.exe")
        && args.first().map(String::as_str) == Some("exec")
}

// ---- planning ---------------------------------------------------------------

/// A credential name from a server name: `@magicuidesign/mcp` → `magicuidesign-mcp`.
fn binding_name(server: &str) -> String {
    let mut s: String = server
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    let s = s.trim_matches('-').chars().take(60).collect::<String>();
    if s.is_empty() { "mcp".into() } else { s }
}

pub fn plan(opts: &Options, store: &Store, vault: &dyn Vault) -> Result<Plan> {
    let configs = load_configs(opts);
    let mut targets = Vec::new();
    let mut skipped = Vec::new();

    for (ci, config) in configs.iter().enumerate() {
        let project_file = config.project;
        let is_settings = config.path.ends_with(".claude/settings.json");

        if is_settings && let Some(env) = config.value.get("env").and_then(Value::as_object) {
            for (k, v) in env {
                if v.as_str().is_some_and(|v| is_secret(k, v)) {
                    skipped.push(Skipped {
                        place: format!("{} · env.{k}", config.display),
                        reason: "Claude Code passes it to every session, not one server: remove it, or store it with `keymule add`",
                    });
                }
            }
        }

        for (keys, label) in servers_in(&config.value) {
            let Some(server) = get_path(&config.value, &keys) else {
                continue;
            };
            let place = format!("{} · {label}", config.display);
            match analyze(server) {
                Analysis::Nothing => {}
                Analysis::Skip(reason) => skipped.push(Skipped { place, reason }),
                Analysis::Adopt(_) if project_file => skipped.push(Skipped {
                    place,
                    reason: "project file, probably shared with your team: use ${VAR} references there instead",
                }),
                Analysis::Adopt(mut t) => {
                    t.config = ci;
                    t.place = place;
                    t.keys = keys;
                    targets.push(*t);
                }
            }
        }
    }

    // Give each target a binding: same server name and same secrets share one.
    let mut bindings: BTreeMap<String, Binding> = BTreeMap::new();
    for t in &mut targets {
        let server = t.keys.last().cloned().unwrap_or_default();
        let base = binding_name(&server);
        let mut n = 1;
        loop {
            let name = if n == 1 {
                base.clone()
            } else {
                format!("{base}-{n}")
            };
            n += 1;
            let existing = match bindings.get(&name) {
                Some(b) => Some(b.clone()),
                None if store.servers.contains_key(&name) => match Binding::load(vault, &name) {
                    Ok(b) => Some(b),
                    Err(_) => continue,
                },
                None if store.creds.contains_key(&name) => continue,
                None => None,
            };
            let mut b = match existing {
                Some(b) if fits(&b, t) => b,
                Some(_) => continue,
                None => Binding {
                    env: t.secret_env.clone(),
                    ..Default::default()
                },
            };
            validate_name(&name)?;
            b.args.extend(t.arg_secrets.clone());
            b.allow(&t.argv);
            bindings.insert(name.clone(), b);
            t.binding = name;
            break;
        }
    }

    Ok(Plan {
        configs,
        targets,
        skipped,
        bindings,
        exe: opts.exe.clone(),
    })
}

fn fits(b: &Binding, t: &Target) -> bool {
    b.env == t.secret_env
        && t.arg_secrets
            .iter()
            .all(|(ph, v)| b.args.get(ph).is_none_or(|old| old == v))
}

// ---- rendering --------------------------------------------------------------

impl Plan {
    fn new_args(&self, t: &Target) -> Vec<String> {
        ["exec", &t.binding, "--"]
            .into_iter()
            .map(String::from)
            .chain(t.argv.iter().cloned())
            .collect()
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        if self.targets.is_empty() {
            out.push_str("✓ No MCP server keeps a secret in plain text.\n");
        } else {
            let files: std::collections::BTreeSet<usize> =
                self.targets.iter().map(|t| t.config).collect();
            out.push_str(&format!(
                "{} MCP server entr{} in {} config file{} keep secrets in plain text:\n",
                self.targets.len(),
                if self.targets.len() == 1 { "y" } else { "ies" },
                files.len(),
                if files.len() == 1 { "" } else { "s" },
            ));
            let mut by_binding: BTreeMap<&str, Vec<&Target>> = BTreeMap::new();
            for t in &self.targets {
                by_binding.entry(&t.binding).or_default().push(t);
            }
            for (name, ts) in &by_binding {
                let t = ts[0];
                let mut what: Vec<String> = t
                    .secret_env
                    .iter()
                    .map(|(k, v)| format!("{k}={}", mask(v)))
                    .collect();
                if !t.arg_secrets.is_empty() {
                    let n = t.arg_secrets.len();
                    what.push(format!("{n} in arguments"));
                }
                out.push_str(&format!("\n  {name}   {}\n", what.join(", ")));
                for t in ts {
                    out.push_str(&format!("      {}\n", t.place));
                }
            }
            let t = &self.targets[0];
            out.push_str(&format!(
                "\nEach entry will start the same command through keymule, which adds the secrets. For example:\n  before: {}\n  after:  {} {}\n",
                t.argv.join(" "),
                self.exe,
                self.new_args(t).join(" ")
            ));
        }
        if !self.skipped.is_empty() {
            out.push_str("\nNot adopted:\n");
            for s in &self.skipped {
                out.push_str(&format!("  {}\n      {}\n", s.place, s.reason));
            }
        }
        out
    }

    /// A stable summary, to check nothing changed between showing the plan
    /// and applying it.
    pub fn fingerprint(&self) -> Vec<(String, String)> {
        self.targets
            .iter()
            .map(|t| (t.place.clone(), t.binding.clone()))
            .collect()
    }

    pub fn files(&self) -> Vec<String> {
        let mut seen = std::collections::BTreeSet::new();
        self.targets
            .iter()
            .filter(|t| seen.insert(t.config))
            .map(|t| self.configs[t.config].display.clone())
            .collect()
    }
}

// ---- applying ---------------------------------------------------------------

pub struct Applied {
    pub secrets: usize,
    pub bindings: Vec<String>,
    pub backups: Vec<String>,
}

pub fn apply(plan: &Plan, store: &mut Store, vault: &dyn Vault, home: &Path) -> Result<Applied> {
    // Keychain first: if a config write fails, the server still has its
    // secret in the old config and nothing breaks.
    let mut secrets = 0;
    for (name, b) in &plan.bindings {
        b.save(vault, name)?;
        secrets += b.secrets().count();
    }
    for t in &plan.targets {
        let meta = store.servers.entry(t.binding.clone()).or_default();
        let b = &plan.bindings[&t.binding];
        meta.env = b.env.keys().cloned().collect();
        meta.arg_secrets = b.args.len();
        if !meta.used_by.contains(&t.place) {
            meta.used_by.push(t.place.clone());
        }
    }
    store.save()?;

    let mut backups = Vec::new();
    for (ci, config) in plan.configs.iter().enumerate() {
        let targets: Vec<&Target> = plan.targets.iter().filter(|t| t.config == ci).collect();
        if targets.is_empty() {
            continue;
        }
        let new_text = match config.format {
            Format::Json => rewrite_json(&config.text, &targets, plan)?,
            Format::Toml => rewrite_toml(&config.text, &targets, plan)?,
        };
        let backup = backup_path(&config.path);
        std::fs::copy(&config.path, &backup)
            .with_context(|| format!("backing up {}", config.display))?;
        backups.push(display(home, &backup));
        write_atomic(&config.path, &new_text)
            .with_context(|| format!("writing {}", config.display))?;
    }
    Ok(Applied {
        secrets,
        bindings: plan.bindings.keys().cloned().collect(),
        backups,
    })
}

fn rewrite_json(text: &str, targets: &[&Target], plan: &Plan) -> Result<String> {
    let mut value: Value = serde_json::from_str(text)?;
    for t in targets {
        let mut obj = &mut value;
        for k in &t.keys {
            obj = obj
                .get_mut(k)
                .with_context(|| format!("{} moved while adopting", t.place))?;
        }
        let Some(obj) = obj.as_object_mut() else {
            bail!("{} is not an object", t.place);
        };
        obj.insert("command".into(), Value::String(plan.exe.clone()));
        obj.insert(
            "args".into(),
            Value::Array(plan.new_args(t).into_iter().map(Value::String).collect()),
        );
        if t.had_env || !t.keep_env.is_empty() {
            obj.insert("env".into(), Value::Object(t.keep_env.clone()));
        }
    }
    let mut out = serde_json::to_string_pretty(&value)?;
    if text.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

fn rewrite_toml(text: &str, targets: &[&Target], plan: &Plan) -> Result<String> {
    let mut doc: toml_edit::DocumentMut = text.parse()?;
    for t in targets {
        let mut item = doc.as_item_mut();
        for k in &t.keys {
            item = item
                .get_mut(k)
                .with_context(|| format!("{} moved while adopting", t.place))?;
        }
        let Some(table) = item.as_table_like_mut() else {
            bail!("{} is not a table", t.place);
        };
        table.insert("command", toml_edit::value(plan.exe.clone()));
        let args: toml_edit::Array = plan.new_args(t).into_iter().collect();
        table.insert("args", toml_edit::value(args));
        let mut env_empty = false;
        if let Some(env) = table.get_mut("env").and_then(|e| e.as_table_like_mut()) {
            for k in t.secret_env.keys() {
                env.remove(k);
            }
            env_empty = env.is_empty();
        }
        if env_empty {
            table.remove("env");
        }
    }
    Ok(doc.to_string())
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".keymule-bak");
    path.with_file_name(name)
}

fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".keymule-tmp");
    let tmp = path.with_file_name(name);
    std::fs::write(&tmp, text)?;
    if let Ok(meta) = std::fs::metadata(path) {
        std::fs::set_permissions(&tmp, meta.permissions())?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn display(home: &Path, path: &Path) -> String {
    let home = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
    match path.strip_prefix(&home) {
        Ok(rel) => format!("~/{}", rel.display()),
        Err(_) => path.display().to_string(),
    }
}

fn tilde(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(h) if path == h => "~/".into(),
        Ok(h) if path.starts_with(&format!("{h}/")) => format!("~{}", &path[h.len()..]),
        _ => path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::testing::MemVault;

    const KEY: &str = "ak_9fK2mQ7vX4pL8nR3tW6yB1cD5hG0sA2eZ";
    const DB: &str = "postgres://app:Vq7Lp2Xk9Rt4@localhost:5432/app";

    fn fake_home() -> (tempfile::TempDir, Options) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().canonicalize().unwrap();
        let claude = serde_json::json!({
            "numStartups": 3,
            "mcpServers": {
                "db": {"type": "stdio", "command": "postgres-mcp", "args": ["--access-mode=unrestricted"],
                       "env": {"DATABASE_URI": DB}},
                "remote": {"type": "http", "url": "https://x.example.com/mcp",
                           "headers": {"x-api-key": "k_8Hq2Vn5Rx7Lm3Pz9Tc4Wb6Yd1"}},
                "plain": {"type": "stdio", "command": "npx", "args": ["-y", "plain-mcp"], "env": {}}
            },
            "projects": {
                "/work/a": {"mcpServers": {"acme": {"type": "stdio", "command": "npx",
                    "args": ["-y", "acme-mcp"], "env": {"ACME_API_URL": "https://acme.dev", "ACME_API_KEY": KEY}}}},
                "/work/b": {"mcpServers": {"acme": {"type": "stdio", "command": "npx",
                    "args": ["-y", "acme-mcp"], "env": {"ACME_API_KEY": KEY}}}},
                "/work/c": {"mcpServers": {"notes": {"type": "stdio",
                    "command": format!("NOTES_TOKEN={KEY}x NOTES_PAGE=a1b2c3 npx notes-mcp"), "args": [], "env": {}}}}
            }
        });
        std::fs::write(
            home.join(".claude.json"),
            serde_json::to_string_pretty(&claude).unwrap() + "\n",
        )
        .unwrap();
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::write(
            home.join(".codex/config.toml"),
            format!(
                "# my codex config\nmodel = \"x\"\n\n[mcp_servers.acme]\ncommand = \"npx\"\nargs = [\"-y\", \"acme-mcp\"]\n\n[mcp_servers.acme.env]\nACME_API_KEY = \"{KEY}\"\n"
            ),
        )
        .unwrap();
        let opts = Options {
            dir: home.join("project"),
            home,
            exe: "/bin/keymule".into(),
        };
        (dir, opts)
    }

    #[test]
    fn plans_and_applies() {
        let (dir, opts) = fake_home();
        let mut store = Store::load_from(dir.path().join("cfg/credentials.toml")).unwrap();
        let vault = MemVault::default();

        let plan = plan(&opts, &store, &vault).unwrap();
        let mut got: Vec<_> = plan
            .targets
            .iter()
            .map(|t| format!("{} → {}", t.place, t.binding))
            .collect();
        got.sort();
        assert_eq!(
            got,
            [
                "~/.claude.json · db → db",
                "~/.claude.json · project /work/a: acme → acme",
                "~/.claude.json · project /work/b: acme → acme",
                "~/.claude.json · project /work/c: notes → notes",
                "~/.codex/config.toml · acme → acme",
            ]
        );
        assert_eq!(plan.skipped.len(), 1, "{:?}", plan.skipped);
        assert!(plan.skipped[0].place.ends_with("remote"));
        let text = plan.render();
        assert!(
            !text.contains(KEY) && !text.contains("Vq7Lp2Xk9Rt4"),
            "{text}"
        );

        let applied = apply(&plan, &mut store, &vault, &opts.home).unwrap();
        assert_eq!(applied.bindings, ["acme", "db", "notes"]);
        assert_eq!(applied.backups.len(), 2);

        let claude = std::fs::read_to_string(opts.home.join(".claude.json")).unwrap();
        assert!(!claude.contains(KEY) && !claude.contains("Vq7Lp2Xk9Rt4"));
        let v: Value = serde_json::from_str(&claude).unwrap();
        let acme = &v["projects"]["/work/a"]["mcpServers"]["acme"];
        assert_eq!(acme["command"], "/bin/keymule");
        assert_eq!(
            acme["args"],
            serde_json::json!(["exec", "acme", "--", "npx", "-y", "acme-mcp"])
        );
        assert_eq!(
            acme["env"],
            serde_json::json!({"ACME_API_URL": "https://acme.dev"})
        );
        let notes = &v["projects"]["/work/c"]["mcpServers"]["notes"];
        assert_eq!(
            notes["args"],
            serde_json::json!(["exec", "notes", "--", "npx", "notes-mcp"])
        );
        assert_eq!(notes["env"], serde_json::json!({"NOTES_PAGE": "a1b2c3"}));
        assert_eq!(v["mcpServers"]["plain"]["command"], "npx");
        assert_eq!(v["numStartups"], 3);

        let codex = std::fs::read_to_string(opts.home.join(".codex/config.toml")).unwrap();
        assert!(
            codex.starts_with("# my codex config\nmodel = \"x\"\n"),
            "{codex}"
        );
        assert!(
            !codex.contains(KEY) && !codex.contains("[mcp_servers.acme.env]"),
            "{codex}"
        );
        assert!(codex.contains(r#"args = ["exec", "acme", "--", "npx", "-y", "acme-mcp"]"#));

        let acme = Binding::load(&vault, "acme").unwrap();
        assert_eq!(acme.env["ACME_API_KEY"], KEY);
        assert!(
            acme.command("acme", &["npx".into(), "-y".into(), "acme-mcp".into()])
                .is_ok()
        );
        assert_eq!(Binding::load(&vault, "db").unwrap().env["DATABASE_URI"], DB);
        assert_eq!(store.servers["acme"].used_by.len(), 3);

        // Running it again finds nothing more to do.
        let again = super::plan(&opts, &store, &vault).unwrap();
        assert!(again.targets.is_empty(), "{:?}", again.fingerprint());
    }

    #[test]
    fn same_name_different_secret_gets_its_own_binding() {
        let (dir, opts) = fake_home();
        let path = opts.home.join(".claude.json");
        let text = std::fs::read_to_string(&path).unwrap().replacen(
            KEY,
            "ak_0Zt5Wq8Rm2Nx6Kp4Lv7Hc3Bd9Fs",
            1,
        );
        std::fs::write(&path, text).unwrap();
        let store = Store::load_from(dir.path().join("cfg/credentials.toml")).unwrap();
        let plan = plan(&opts, &store, &MemVault::default()).unwrap();
        let mut names: Vec<_> = plan.targets.iter().map(|t| t.binding.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names, ["acme", "acme-2", "db", "notes"]);
    }

    #[test]
    fn names_and_shell_lines() {
        assert_eq!(binding_name("@magicuidesign/mcp"), "magicuidesign-mcp");
        assert_eq!(binding_name("pg-mcp-local"), "pg-mcp-local");
        let server = serde_json::json!({"command": format!("TOKEN='{KEY}' npx x"), "args": []});
        assert!(matches!(analyze(&server), Analysis::Skip(_)));
        let reference = serde_json::json!({"command": "npx", "env": {"API_KEY": "${API_KEY}"}});
        assert!(matches!(analyze(&reference), Analysis::Nothing));
    }
}
