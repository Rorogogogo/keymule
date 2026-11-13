//! `keymule ui`: a local web page for keymule. It shows what `keymule list
//! --json`, the audit log, `keymule doctor --json` and `keymule adopt
//! --dry-run` show, and its buttons run keymule commands that open keymule's
//! own dialogs. No secret ever passes through the page: there is no endpoint
//! that takes or returns one (Reveal shows the value in a keymule window).
//!
//! The server listens on 127.0.0.1 only. Every API call needs a token that
//! is only in the URL keymule opens (in the fragment, so it's never sent in a
//! request line or Referer), the Host header must be the loopback address (so
//! DNS rebinding doesn't work), and other origins are refused.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::{Value, json};

const INDEX: &str = include_str!("ui/index.html");
const APP_JS: &str = include_str!("ui/app.js");
const APP_CSS: &str = include_str!("ui/app.css");

const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; \
img-src 'self' data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

struct Ctx {
    port: u16,
    token: String,
    exe: std::path::PathBuf,
}

pub fn run(open: bool, port: u16) -> Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .or_else(|_| TcpListener::bind(("127.0.0.1", 0)))
        .context("can't listen on 127.0.0.1")?;
    let port = listener.local_addr()?.port();
    let ctx = Arc::new(Ctx {
        port,
        token: random_token()?,
        exe: std::env::current_exe()?,
    });
    let url = format!("http://127.0.0.1:{port}/#t={}", ctx.token);
    println!(
        "keymule ui is running at {url}\nOnly this machine can reach it. Press Ctrl-C to stop."
    );
    if open {
        let _ = std::process::Command::new("open").arg(&url).status();
    }
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = handle(&ctx, stream);
        });
    }
    Ok(())
}

fn random_token() -> Result<String> {
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .context("reading /dev/urandom")?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn handle(ctx: &Ctx, mut stream: TcpStream) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.by_ref().take(8192).read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("").to_string();
    let mut headers: HashMap<String, String> = HashMap::new();
    loop {
        let mut h = String::new();
        reader.by_ref().take(8192).read_line(&mut h)?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if headers.len() > 64 {
            return respond(&mut stream, 431, "text/plain", b"too many headers");
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let len: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if len > 64 * 1024 {
        return respond(&mut stream, 413, "text/plain", b"too large");
    }
    let mut body = vec![0; len];
    reader.read_exact(&mut body)?;

    let here = [
        format!("127.0.0.1:{}", ctx.port),
        format!("localhost:{}", ctx.port),
    ];
    if !headers.get("host").is_some_and(|h| here.contains(h)) {
        return respond(&mut stream, 421, "text/plain", b"wrong host");
    }
    let path = target.split('?').next().unwrap_or("");
    match (method.as_str(), path) {
        ("GET", "/") => respond(
            &mut stream,
            200,
            "text/html; charset=utf-8",
            INDEX.as_bytes(),
        ),
        ("GET", "/app.js") => respond(
            &mut stream,
            200,
            "text/javascript; charset=utf-8",
            APP_JS.as_bytes(),
        ),
        ("GET", "/app.css") => respond(
            &mut stream,
            200,
            "text/css; charset=utf-8",
            APP_CSS.as_bytes(),
        ),
        (_, p) if p.starts_with("/api/") => {
            let token_ok = headers
                .get("x-keymule-token")
                .is_some_and(|t| same(t.as_bytes(), ctx.token.as_bytes()));
            let origin_ok = headers
                .get("origin")
                .is_none_or(|o| here.iter().any(|h| *o == format!("http://{h}")));
            if !token_ok || !origin_ok {
                return respond(
                    &mut stream,
                    403,
                    "application/json",
                    b"{\"error\":\"forbidden\"}",
                );
            }
            let out = api(ctx, &method, p, &body);
            respond(
                &mut stream,
                200,
                "application/json",
                serde_json::to_string(&out)?.as_bytes(),
            )
        }
        _ => respond(&mut stream, 404, "text/plain", b"not found"),
    }
}

fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn respond(stream: &mut TcpStream, code: u16, ctype: &str, body: &[u8]) -> Result<()> {
    let reason = match code {
        200 => "OK",
        403 => "Forbidden",
        404 => "Not Found",
        413 => "Payload Too Large",
        421 => "Misdirected Request",
        431 => "Request Header Fields Too Large",
        _ => "Error",
    };
    write!(
        stream,
        "HTTP/1.1 {code} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n\
         Content-Security-Policy: {CSP}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    stream.flush()?;
    Ok(())
}

/// Run keymule itself, the same way you would in a terminal.
fn keymule(ctx: &Ctx, args: &[&str]) -> Value {
    match std::process::Command::new(&ctx.exe)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
    {
        Ok(o) => json!({
            "ok": o.status.success(),
            "stdout": String::from_utf8_lossy(&o.stdout),
            "stderr": String::from_utf8_lossy(&o.stderr).trim().trim_start_matches("keymule: "),
        }),
        Err(e) => json!({"ok": false, "stdout": "", "stderr": e.to_string()}),
    }
}

fn simple_name(s: &str) -> bool {
    s.len() <= 64
        && s.starts_with(|c: char| c.is_ascii_alphanumeric())
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn api(ctx: &Ctx, method: &str, path: &str, body: &[u8]) -> Value {
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
                "list": parsed,
                "error": if list["ok"] == true { Value::Null } else { list["stderr"].clone() },
                "audit": audit_tail(),
            })
        }
        ("GET", "/api/audit") => json!({"audit": audit_tail()}),
        ("POST", "/api/plan") => keymule(ctx, &["adopt", "--dry-run"]),
        ("POST", "/api/adopt") => keymule(ctx, &["adopt", "--yes"]),
        ("POST", "/api/doctor") => {
            let out = keymule(ctx, &["doctor", "--json"]);
            let report: Value = out["stdout"]
                .as_str()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or(Value::Null);
            json!({"ok": out["ok"], "report": report, "stderr": out["stderr"]})
        }
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
fn audit_tail() -> Value {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_compare_whole() {
        assert!(same(b"abc", b"abc"));
        assert!(!same(b"abc", b"abd"));
        assert!(!same(b"abc", b"ab"));
        assert!(simple_name("pg-mcp_1"));
        assert!(!simple_name("a b"));
        assert!(!simple_name("--url"));
    }
}
