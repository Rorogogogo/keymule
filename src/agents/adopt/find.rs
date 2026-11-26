//! Finding MCP servers in agent configs, and which of their values are secrets.

use super::*;

pub(super) fn load_configs(opts: &Options) -> Vec<Config> {
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
pub(super) fn servers_in(value: &Value) -> Vec<(Vec<String>, String)> {
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

pub(super) fn get_path<'a>(value: &'a Value, keys: &[String]) -> Option<&'a Value> {
    keys.iter().try_fold(value, |v, k| v.get(k))
}

pub(super) fn is_secret(key: &str, value: &str) -> bool {
    (is_secret_name(key) && detect::plausible_value(value.trim()))
        || !detect::scan_deep(value).is_empty()
}

/// A URL with a password in it, e.g. postgres://user:pass@host/db.
pub(super) static CRED_URL: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"://[^/\s:@]+:[^@\s]+@").unwrap());

/// For a server the user named with `--server`: anything secret-named or a
/// URL with a password, however default it looks. `${VAR}` references stay.
pub(super) fn is_forced_secret(key: &str, value: &str) -> bool {
    let v = value.trim();
    !v.is_empty() && !v.contains("${") && (is_secret_name(key) || CRED_URL.is_match(v))
}

pub(super) enum Analysis {
    Nothing,
    Skip(&'static str),
    Adopt(Box<Target>),
}

/// Characters that make a command line more than "VAR=x program args".
pub(super) const SHELL_CHARS: &[char] =
    &['\'', '"', '$', '`', '\\', '|', ';', '&', '<', '>', '(', ')'];

pub(super) fn analyze(server: &Value, force: bool) -> Analysis {
    let is_secret = |k: &str, v: &str| is_secret(k, v) || (force && is_forced_secret(k, v));
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

    if let Some(url) = obj.get("url") {
        return analyze_remote(obj, url.as_str().unwrap_or(""), has_secret_env, force);
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
        url: None,
        secret_headers: BTreeMap::new(),
        keep_headers: Vec::new(),
        binding: String::new(),
    }))
}

/// Header keys an entry may use: Claude Code and Cursor say `headers`,
/// Codex `http_headers`.
pub(super) const HEADER_KEYS: [&str; 2] = ["headers", "http_headers"];

pub(super) fn analyze_remote(
    obj: &Map<String, Value>,
    url: &str,
    has_secret_env: bool,
    force: bool,
) -> Analysis {
    let mut secret_headers = BTreeMap::new();
    let mut keep_headers = Vec::new();
    for key in HEADER_KEYS {
        let Some(headers) = obj.get(key).and_then(Value::as_object) else {
            continue;
        };
        for (k, v) in headers {
            let Some(v) = v.as_str() else {
                return Analysis::Nothing;
            };
            let forced =
                force && !v.trim().is_empty() && !v.contains("${") && secret_header_name(k);
            if is_secret_header(k, v) || forced {
                secret_headers.insert(k.clone(), v.trim().to_string());
            } else {
                keep_headers.push(format!("{k}: {v}"));
            }
        }
    }
    if secret_headers.is_empty() {
        return if has_secret_env {
            Analysis::Skip(
                "HTTP server with a secret in env, which a remote server doesn't read: remove it",
            )
        } else {
            Analysis::Nothing
        };
    }
    if url.contains("${") || !crate::remote::url_ok(url) {
        return Analysis::Skip(
            "keymule only sends headers to a fixed https URL (or http on localhost)",
        );
    }
    if secret_headers.keys().chain(keep_headers.iter()).any(|h| {
        crate::remote::RESERVED.contains(
            &h.split(':')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
                .as_str(),
        )
    }) {
        return Analysis::Skip("it sets a header keymule manages itself");
    }
    Analysis::Adopt(Box::new(Target {
        config: 0,
        keys: Vec::new(),
        place: String::new(),
        argv: Vec::new(),
        secret_env: BTreeMap::new(),
        arg_secrets: BTreeMap::new(),
        keep_env: Map::new(),
        had_env: false,
        url: Some(url.to_string()),
        secret_headers,
        keep_headers,
        binding: String::new(),
    }))
}

pub(super) fn secret_header_name(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    k == "authorization"
        || k.contains("key")
        || k.contains("token")
        || k.contains("secret")
        || is_secret_name(key)
}

pub(super) fn is_secret_header(key: &str, value: &str) -> bool {
    if value.contains("${") {
        return false;
    }
    let v = value.trim().trim_start_matches("Bearer ").trim();
    (secret_header_name(key) && detect::plausible_value(v)) || !detect::scan_deep(value).is_empty()
}

pub(super) fn is_env_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub(super) fn is_keymule_exec(obj: &Map<String, Value>, args: &[String]) -> bool {
    let cmd = obj.get("command").and_then(Value::as_str).unwrap_or("");
    let program = Path::new(cmd)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();
    (program == "keymule" || program == "keymule.exe")
        && matches!(args.first().map(String::as_str), Some("exec" | "remote"))
}
