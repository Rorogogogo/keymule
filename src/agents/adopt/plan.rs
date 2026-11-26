//! Deciding what moves into keymule, and under which name.

use super::*;

/// A credential name from a server name: `@magicuidesign/mcp` → `magicuidesign-mcp`.
pub(super) fn binding_name(server: &str) -> String {
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
            let server_name = keys.last().map(String::as_str).unwrap_or("");
            let force = opts.force.iter().any(|f| f == server_name);
            match analyze(server, force) {
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

    for name in &opts.force {
        let known = configs.iter().any(|c| {
            servers_in(&c.value)
                .iter()
                .any(|(k, _)| k.last() == Some(name))
        });
        if !known {
            bail!("no MCP server named \"{name}\" in your agent configs");
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
                    headers: t.secret_headers.clone(),
                    ..Default::default()
                },
            };
            validate_name(&name)?;
            b.args.extend(t.arg_secrets.clone());
            match &t.url {
                Some(url) => b.allow_url(url),
                None => b.allow(&t.argv),
            }
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

pub(super) fn fits(b: &Binding, t: &Target) -> bool {
    b.env == t.secret_env
        && b.headers == t.secret_headers
        && t.arg_secrets
            .iter()
            .all(|(ph, v)| b.args.get(ph).is_none_or(|old| old == v))
}
