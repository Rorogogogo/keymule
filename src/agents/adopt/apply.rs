//! Rewriting the configs, with a backup next to each.

use super::*;

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
        meta.headers = b.headers.keys().cloned().collect();
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

pub(super) fn rewrite_json(text: &str, targets: &[&Target], plan: &Plan) -> Result<String> {
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
        if t.url.is_some() {
            obj.remove("url");
            for k in HEADER_KEYS {
                obj.remove(k);
            }
            if obj.contains_key("type") {
                obj.insert("type".into(), Value::String("stdio".into()));
            }
        }
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

pub(super) fn rewrite_toml(text: &str, targets: &[&Target], plan: &Plan) -> Result<String> {
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
        if t.url.is_some() {
            table.remove("url");
            for k in HEADER_KEYS {
                table.remove(k);
            }
        }
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

/// `Bearer abc…` → `Bearer ab…yz`-style, keeping the scheme readable.
pub(super) fn mask_header(v: &str) -> String {
    match v.split_once(' ') {
        Some((scheme, token)) if !token.contains(' ') => format!("{scheme} {}", mask(token)),
        _ => mask(v),
    }
}

pub(crate) fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".keymule-bak");
    path.with_file_name(name)
}

pub(crate) fn write_atomic(path: &Path, text: &str) -> Result<()> {
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

pub(super) fn display(home: &Path, path: &Path) -> String {
    let home = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
    match path.strip_prefix(&home) {
        Ok(rel) => format!("~/{}", rel.display()),
        Err(_) => path.display().to_string(),
    }
}

pub(super) fn tilde(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(h) if path == h => "~/".into(),
        Ok(h) if path.starts_with(&format!("{h}/")) => format!("~{}", &path[h.len()..]),
        _ => path.to_string(),
    }
}
