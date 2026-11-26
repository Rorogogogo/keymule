//! Agent and MCP config files.

use super::*;

pub(super) fn scan_configs(opts: &Options, collector: &Mutex<Collector>, stats: &mut Stats) {
    let home = &opts.home;
    let json_files = [
        home.join(".claude.json"),
        home.join(".claude").join("settings.json"),
        home.join(".cursor").join("mcp.json"),
        home.join("Library/Application Support/Claude/claude_desktop_config.json"),
        opts.dir.join(".mcp.json"),
        opts.dir.join(".cursor").join("mcp.json"),
        opts.dir.join(".claude").join("settings.json"),
        opts.dir.join(".claude").join("settings.local.json"),
    ];
    let mut seen = Vec::new();
    for path in json_files {
        let Ok(canon) = path.canonicalize() else {
            continue;
        };
        if seen.contains(&canon) {
            continue;
        }
        seen.push(canon);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let name = display(home, &path);
        scan_value(&value, &mut Vec::new(), &name, collector);
        stats.configs.push(name);
    }
    let codex = home.join(".codex").join("config.toml");
    if let Ok(text) = std::fs::read_to_string(&codex)
        && let Ok(value) = toml::from_str::<toml::Value>(&text)
        && let Ok(json) = serde_json::to_value(value)
    {
        let name = display(home, &codex);
        scan_value(&json, &mut Vec::new(), &name, collector);
        stats.configs.push(name);
    }
}

pub fn is_secret_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    [
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "API_KEY",
        "APIKEY",
        "ACCESS_KEY",
        "PRIVATE_KEY",
        "AUTHORIZATION",
        "CREDENTIAL",
    ]
    .iter()
    .any(|k| upper.contains(k))
        && !upper.contains("AUTHOR_")
        // maxTokens, tokenCount, FirstTokenDate, GITHUB_API_URL: settings, not secrets.
        && ![
            "DATE", "COUNT", "LIMIT", "USAGE", "TIME", "MAX", "SIZE", "URL", "PATH", "FILE",
            "ENDPOINT",
        ]
        .iter()
        .any(|k| upper.contains(k))
}

/// Walk a parsed config; report secret-named values and anything the
/// detector recognises in other strings.
pub(super) fn scan_value(
    value: &Value,
    path: &mut Vec<String>,
    file: &str,
    collector: &Mutex<Collector>,
) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                path.push(k.clone());
                scan_value(v, path, file, collector);
                path.pop();
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                path.push(i.to_string());
                scan_value(v, path, file, collector);
                path.pop();
            }
        }
        Value::String(s) => {
            let key = path.last().map(String::as_str).unwrap_or("");
            let mut hits = detect::scan_deep(s);
            let value = s.trim().trim_start_matches("Bearer ").trim();
            if is_secret_name(key)
                && detect::plausible_value(value)
                && !hits.iter().any(|h| h.value == value)
            {
                hits.push(Found {
                    value: value.to_string(),
                    service: None,
                    label: Some(key.to_string()),
                    confident: true,
                });
            }
            if hits.is_empty() {
                return;
            }
            let place = format!("{file} · {}", config_path(path));
            let mut c = collector.lock().unwrap();
            for mut h in hits {
                if h.label.is_none() {
                    h.label = Some(key.to_string());
                }
                c.add(
                    h,
                    Location {
                        source: place.clone(),
                        severity: Severity::Plaintext,
                        place: place.clone(),
                        date: None,
                    },
                );
            }
        }
        _ => {}
    }
}

/// `projects./Users/x/app.mcpServers.github.env.TOKEN` → `project ~/app: github.env.TOKEN`
pub(super) fn config_path(path: &[String]) -> String {
    let mut parts: Vec<&str> = path.iter().map(String::as_str).collect();
    let mut prefix = String::new();
    if parts.first() == Some(&"projects") && parts.len() > 2 {
        prefix = format!("project {}: ", tilde(parts[1]));
        parts.drain(..2);
    }
    if matches!(parts.first(), Some(&"mcpServers") | Some(&"mcp_servers")) && parts.len() > 1 {
        parts.remove(0);
    }
    format!("{prefix}{}", parts.join("."))
}
