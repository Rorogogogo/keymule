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
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::{Value, json};

mod api;
mod pairing;
mod scan;
#[cfg(test)]
mod tests;

use api::*;
use pairing::*;
use scan::*;

const INDEX: &str = include_str!("ui/web/index.html");
const APP_JS: &str = include_str!("ui/web/app.js");
const APP_CSS: &str = include_str!("ui/web/app.css");

const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; \
img-src 'self' data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

struct Ctx {
    port: u16,
    token: String,
    exe: std::path::PathBuf,
    /// Pairs the first browser for page approvals; printed in the terminal
    /// that ran `keymule ui`.
    code: String,
    /// `keymule ui` was started at a terminal, so the code can also pair a
    /// browser when others are already paired. Without this, losing your
    /// only paired browser (cleared site data, another address) would lock
    /// you out for good.
    code_repairs: bool,
    /// One-off challenges for a paired browser to log in with, and when
    /// each runs out.
    challenges: Mutex<Vec<(String, Instant)>>,
}

/// Bumped when a browser is paired or unpaired, so a waiting page hears.
static DEVICES_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn approvals_version() -> String {
    format!(
        "{}:{}",
        DEVICES_GEN.load(std::sync::atomic::Ordering::Relaxed),
        crate::approvals::version()
    )
}

/// How long the page's request for news may wait. Chrome slows timers in
/// background tabs to once a minute; a waiting request is answered at once.
const WAIT_SECS: u64 = 25;

/// The page's usual port. A fixed address matters: a browser keeps its
/// pairing key per address, so a new port each run would lose it.
pub const DEFAULT_PORT: u16 = 7420;

pub fn run(open: bool, port: u16) -> Result<()> {
    // The menu-bar app already serves the page: show its window instead.
    #[cfg(target_os = "macos")]
    if open && port == DEFAULT_PORT && crate::menubar::open_running() {
        println!("Opened keymule's window (from the menu bar app).");
        return Ok(());
    }
    let at_terminal = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .is_ok();
    crate::app::update::remember_binary();
    let (listener, ctx) = bind(port, at_terminal)?;
    let url = ctx.url();
    println!(
        "keymule ui is running at {url}\nOnly this machine can reach it. Press Ctrl-C to stop."
    );
    if let Some(v) = crate::app::update::available() {
        println!(
            "keymule {v} is out (this is {}). Upgrade: {}",
            crate::app::update::CURRENT,
            crate::app::update::upgrade_command(&std::env::current_exe().unwrap_or_default())
        );
    }
    crate::app::update::watch();
    if crate::dialog::page_mode() {
        println!(
            "Questions from keymule wait in this page. To answer them, click \"Pair this \
             browser\" there and enter: {}",
            ctx.code
        );
    }
    if open {
        let _ = std::process::Command::new("open").arg(&url).status();
    }
    serve(listener, ctx);
    Ok(())
}

/// Start the server on a background thread; returns the page's address
/// (with its token). Used by the macOS menu-bar app.
#[cfg(target_os = "macos")]
pub fn start() -> Result<String> {
    crate::app::update::remember_binary();
    let (listener, ctx) = bind(DEFAULT_PORT, false)?;
    let url = ctx.url();
    crate::app::update::watch();
    std::thread::spawn(move || serve(listener, ctx));
    Ok(url)
}

fn bind(port: u16, code_repairs: bool) -> Result<(TcpListener, Arc<Ctx>)> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .or_else(|_| TcpListener::bind(("127.0.0.1", 0)))
        .context("can't listen on 127.0.0.1")?;
    let port = listener.local_addr()?.port();
    let t = random_token()?;
    let n = u32::from_str_radix(&t[..8], 16).unwrap_or(0) % 1_000_000;
    let ctx = Arc::new(Ctx {
        port,
        token: random_token()?,
        exe: std::env::current_exe()?,
        code: format!("{:03}-{:03}", n / 1000, n % 1000),
        code_repairs,
        challenges: Mutex::new(Vec::new()),
    });
    Ok((listener, ctx))
}

impl Ctx {
    fn url(&self) -> String {
        format!("http://127.0.0.1:{}/#t={}", self.port, self.token)
    }
}

fn serve(listener: TcpListener, ctx: Arc<Ctx>) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = handle(&ctx, stream);
        });
    }
}

/// `GET /api/approvals?wait=<version>` holds until something changes (or
/// [`WAIT_SECS`] pass). Without `wait` it answers straight away.
fn wait_for_news(target: &str) {
    let Some(seen) = target
        .split_once('?')
        .and_then(|(_, q)| q.split('&').find_map(|kv| kv.strip_prefix("wait=")))
    else {
        return;
    };
    let until = std::time::Instant::now() + Duration::from_secs(WAIT_SECS);
    while std::time::Instant::now() < until && approvals_version() == seen {
        std::thread::sleep(Duration::from_millis(250));
    }
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
        // A paired browser opens the page without the link's token: it signs
        // a one-off challenge with its pairing key (which can't be read out
        // of the browser) and gets the token back. No cookie: a browser sends
        // 127.0.0.1's cookies to every port, so any local server would get it.
        ("POST", "/login/challenge" | "/login") => {
            let origin_ok = headers
                .get("origin")
                .is_some_and(|o| here.iter().any(|h| *o == format!("http://{h}")));
            if !origin_ok {
                return respond(
                    &mut stream,
                    403,
                    "application/json",
                    b"{\"error\":\"forbidden\"}",
                );
            }
            let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            let out = if path == "/login" {
                login(ctx, &body)
            } else {
                challenge(ctx)
            };
            respond(
                &mut stream,
                200,
                "application/json",
                serde_json::to_string(&out)?.as_bytes(),
            )
        }
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
            if method == "GET" && p == "/api/approvals" {
                wait_for_news(&target);
            }
            if method == "GET" && p == "/api/stats" {
                let q = |k: &str| {
                    target.split_once('?').and_then(|(_, q)| {
                        q.split('&')
                            .find_map(|kv| kv.strip_prefix(k)?.strip_prefix('='))
                    })
                };
                let tz = q("tz").and_then(|v| v.parse().ok()).unwrap_or(0);
                let days = q("days").and_then(|v| v.parse().ok()).unwrap_or(30);
                let out = crate::app::stats::load(tz, days);
                return respond(
                    &mut stream,
                    200,
                    "application/json",
                    serde_json::to_string(&out)?.as_bytes(),
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

// ---- leak check in the background ------------------------------------------
//
// A scan can take a while (gigabytes of transcripts), so it runs on its own
// thread as `keymule doctor --json --progress`, and the page polls how far it
// has got. The progress lines hold only counts, phases and file places.
