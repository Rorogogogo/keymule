//! The page's JSON API: every button runs a keymule command.

use super::*;

pub(super) fn simple_name(s: &str) -> bool {
    s.len() <= 64
        && s.starts_with(|c: char| c.is_ascii_alphanumeric())
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// A host name or an IP address (no spaces, can't be taken for an option).
pub(super) fn host_like(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 253
        && !s.starts_with('-')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '_' | '[' | ']'))
}

pub(super) fn api(ctx: &Ctx, method: &str, path: &str, body: &[u8]) -> Value {
    let body: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let field = |k: &str| body.get(k).and_then(Value::as_str).unwrap_or("").trim();
    match (method, path) {
        ("GET", "/api/state") => {
            let list = keymule(ctx, &["list", "--json"]);
            let parsed: Value = list["stdout"]
                .as_str()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or(Value::Null);
            json!({
                "version": env!("CARGO_PKG_VERSION"),
                "update": crate::app::update::status(),
                // Who serves the page decides how to restart it.
                "host": if crate::app::update::in_menubar() { "menubar" } else { "ui" },
                "list": parsed,
                "error": if list["ok"] == true { Value::Null } else { list["stderr"].clone() },
                "audit": audit_tail(),
            })
        }
        ("GET", "/api/audit") => json!({"audit": audit_tail()}),
        ("GET", "/api/approvals") => {
            let vault = crate::store::vault();
            let devices: Vec<Value> = crate::approvals::devices(vault.as_ref())
                .iter()
                .map(|d| json!({"id": d.id, "name": d.name, "added": d.added}))
                .collect();
            json!({
                "version": approvals_version(),
                "page_mode": crate::dialog::page_mode(),
                "pending": crate::approvals::pending(),
                "shows": crate::approvals::shows(),
                "devices": devices,
            })
        }
        ("POST", "/api/approvals/answer") => {
            let reply: Result<crate::approvals::Reply, _> =
                serde_json::from_value(body.get("reply").cloned().unwrap_or(Value::Null));
            let Ok(reply) = reply else {
                return json!({"ok": false, "stderr": "bad answer"});
            };
            let vault = crate::store::vault();
            match crate::approvals::answer(vault.as_ref(), field("id"), &reply) {
                Ok(()) => json!({"ok": true}),
                Err(e) => json!({"ok": false, "stderr": e.to_string()}),
            }
        }
        ("POST", "/api/restart") => {
            #[cfg(target_os = "macos")]
            if crate::app::update::in_menubar() {
                crate::menubar::restart_detached();
                return json!({"ok": true});
            }
            json!({"ok": false, "stderr": "stop `keymule ui` and start it again"})
        }
        ("POST", "/api/approvals/deny") => match crate::approvals::deny(field("id")) {
            Ok(()) => json!({"ok": true}),
            Err(e) => json!({"ok": false, "stderr": e.to_string()}),
        },
        ("POST", "/api/pair") => pair(ctx, field("name"), field("key"), field("code")),
        ("POST", "/api/unpair") => {
            let id = field("id").to_string();
            let vault = crate::store::vault();
            let Some(d) = crate::approvals::devices(vault.as_ref())
                .into_iter()
                .find(|d| d.id == id)
            else {
                return json!({"ok": false, "stderr": "no such browser"});
            };
            // Unpairing needs a yes too: otherwise anything could unpair
            // your browser and pair its own as the first one.
            match crate::dialog::confirm(&format!(
                "Unpair \"{}\"?\n\nIt won't be able to answer keymule's questions any more.",
                d.name
            )) {
                Ok(true) => match crate::approvals::remove_device(vault.as_ref(), &id) {
                    Ok(_) => {
                        DEVICES_GEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        json!({"ok": true})
                    }
                    Err(e) => json!({"ok": false, "stderr": e.to_string()}),
                },
                Ok(false) => json!({"ok": false, "stderr": "not unpaired"}),
                Err(e) => json!({"ok": false, "stderr": e.to_string()}),
            }
        }
        ("POST", "/api/plan") => keymule(ctx, &["adopt", "--dry-run"]),
        ("POST", "/api/adopt") => keymule(ctx, &["adopt", "--yes"]),
        ("POST", "/api/doctor") => {
            start_scan(ctx);
            scan_status()
        }
        ("GET", "/api/doctor") => scan_status(),
        ("POST", "/api/rotate") => {
            let name = field("name");
            if !simple_name(name) {
                return json!({"ok": false, "stderr": "bad name"});
            }
            keymule(ctx, &["rotate", name, "--dialog"])
        }
        ("POST", "/api/delete") => {
            let name = field("name");
            if !simple_name(name) {
                return json!({"ok": false, "stderr": "bad name"});
            }
            keymule(ctx, &["rm", name, "--dialog"])
        }
        ("POST", "/api/reveal") => {
            let name = field("name");
            if !simple_name(name) {
                return json!({"ok": false, "stderr": "bad name"});
            }
            keymule(ctx, &["reveal", name])
        }
        ("POST", "/api/add") if field("service") == "ssh" => {
            let (host, user, port) = (field("host"), field("username"), field("port"));
            if !host_like(host)
                || !simple_name(user)
                || !(port.is_empty() || port.parse::<u16>().is_ok())
            {
                return json!({"ok": false, "stderr": "give the server's host or IP, the username, and a port number (or leave it empty)"});
            }
            let mut args = vec!["add", "ssh", "--dialog", "--host", host, "--username", user];
            let name = field("name");
            if !name.is_empty() {
                if !simple_name(name) {
                    return json!({"ok": false, "stderr": "a name uses letters, digits, '-' and '_'"});
                }
                args.extend(["--name", name]);
            }
            if !port.is_empty() {
                args.extend(["--port", port]);
            }
            let key_file = format!("--key-file={}", field("key_file"));
            match field("auth") {
                "key" => args.push("--key"),
                "keyfile" => {
                    let f = field("key_file");
                    if f.is_empty() || f.len() > 1024 || f.contains(['\n', '\r']) {
                        return json!({"ok": false, "stderr": "give the key file's path, like ~/.ssh/id_ed25519"});
                    }
                    args.push(&key_file);
                }
                _ => {}
            }
            let description = format!("--description={}", field("description"));
            if !field("description").is_empty() {
                args.push(&description);
            }
            keymule(ctx, &args)
        }
        ("POST", "/api/convert") => {
            let (name, to) = (field("name"), field("to"));
            if !simple_name(name) || !["ssh", "password"].contains(&to) {
                return json!({"ok": false, "stderr": "bad request"});
            }
            let mut args = vec!["convert", name, "--to", to];
            let (host, user, port) = (field("host"), field("username"), field("port"));
            if to == "ssh" {
                if !host_like(host)
                    || !simple_name(user)
                    || !(port.is_empty() || port.parse::<u16>().is_ok())
                {
                    return json!({"ok": false, "stderr": "give the server's host or IP, the username, and a port number (or leave it empty)"});
                }
                args.extend(["--host", host, "--username", user]);
                if !port.is_empty() {
                    args.extend(["--port", port]);
                }
            }
            let description = format!("--description={}", field("description"));
            if !field("description").is_empty() {
                args.push(&description);
            }
            keymule(ctx, &args)
        }
        ("POST", "/api/describe") => {
            let name = field("name");
            let text = body
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if !simple_name(name) || text.len() > 300 || text.contains('\n') {
                return json!({"ok": false, "stderr": "a description is one line, up to 300 characters"});
            }
            keymule(ctx, &["edit", name, &format!("--description={text}")])
        }
        ("POST", "/api/add") => {
            let service = field("service");
            let url = field("url");
            if !simple_name(service) {
                return json!({"ok": false, "stderr": "bad service"});
            }
            if url.is_empty() {
                keymule(ctx, &["add", service, "--dialog"])
            } else if url.starts_with("https://") || url.starts_with("http://") {
                keymule(ctx, &["add", service, "--dialog", "--url", url])
            } else {
                json!({"ok": false, "stderr": "the URL should start with https://"})
            }
        }
        _ => json!({"ok": false, "stderr": "unknown endpoint"}),
    }
}

/// The last 200 audit log lines, newest last. The log holds no secrets.
pub(super) fn audit_tail() -> Value {
    let Ok(path) = crate::store::config_dir().map(|d| d.join("audit.log")) else {
        return json!([]);
    };
    let Ok(mut f) = std::fs::File::open(path) else {
        return json!([]);
    };
    use std::io::Seek;
    let size = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = size.saturating_sub(256 * 1024);
    let _ = f.seek(std::io::SeekFrom::Start(start));
    let mut text = String::new();
    let _ = f.read_to_string(&mut text);
    let lines: Vec<&str> = text.lines().skip(usize::from(start > 0)).collect();
    let rows: Vec<Value> = lines[lines.len().saturating_sub(200)..]
        .iter()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.len() >= 5).then(|| {
                json!({
                    "ts": f[0].parse::<u64>().unwrap_or(0),
                    "name": f[1],
                    "method": f[2],
                    "url": f[3],
                    "status": f[4].parse::<u64>().unwrap_or(0),
                })
            })
        })
        .collect();
    Value::Array(rows)
}
