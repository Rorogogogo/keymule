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

const INDEX: &str = include_str!("ui/index.html");
const APP_JS: &str = include_str!("ui/app.js");
const APP_CSS: &str = include_str!("ui/app.css");

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
    let (listener, ctx) = bind(port, at_terminal)?;
    let url = ctx.url();
    println!(
        "keymule ui is running at {url}\nOnly this machine can reach it. Press Ctrl-C to stop."
    );
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
    let (listener, ctx) = bind(DEFAULT_PORT, false)?;
    let url = ctx.url();
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

/// How long a login challenge stays good.
const CHALLENGE_SECS: u64 = 60;

fn challenge(ctx: &Ctx) -> Value {
    let Ok(nonce) = random_token() else {
        return json!({"ok": false});
    };
    let mut list = ctx.challenges.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    list.retain(|(_, until)| *until > now);
    if list.len() >= 32 {
        list.remove(0);
    }
    list.push((nonce.clone(), now + Duration::from_secs(CHALLENGE_SECS)));
    json!({"ok": true, "nonce": nonce})
}

fn login(ctx: &Ctx, body: &Value) -> Value {
    let field = |k: &str| body.get(k).and_then(Value::as_str).unwrap_or("");
    let nonce = field("nonce");
    // Each challenge works once, whatever the outcome.
    let fresh = {
        let mut list = ctx.challenges.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let at = list
            .iter()
            .position(|(n, until)| *until > now && same(n.as_bytes(), nonce.as_bytes()));
        at.map(|i| list.remove(i)).is_some()
    };
    if !fresh {
        return json!({"ok": false, "stderr": "that challenge ran out; try again"});
    }
    let vault = crate::store::vault();
    let devices = crate::approvals::devices(vault.as_ref());
    match crate::approvals::verify_login(&devices, field("device"), nonce, field("sig")) {
        Ok(()) => {
            crate::http::audit("-", "PAGE-LOGIN", field("device"), 0);
            json!({"ok": true, "token": ctx.token})
        }
        Err(e) => json!({"ok": false, "stderr": e.to_string()}),
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

#[derive(Default)]
struct Scan {
    running: bool,
    /// Counts scans, so the page can tell a new one from the last.
    id: u64,
    started: Option<Instant>,
    finished: Option<Duration>,
    phase: String,
    /// The latest log lines, oldest first.
    log: Vec<String>,
    /// How many log lines there have been in all.
    lines: usize,
    done: u64,
    total: u64,
    found: u64,
    report: Value,
    error: Option<String>,
}

static SCAN: LazyLock<Mutex<Scan>> = LazyLock::new(|| Mutex::new(Scan::default()));
const SCAN_LOG: usize = 50;

fn start_scan(ctx: &Ctx) {
    let mut s = SCAN.lock().unwrap();
    if s.running {
        return;
    }
    *s = Scan {
        running: true,
        id: s.id + 1,
        started: Some(Instant::now()),
        phase: "transcripts".into(),
        ..Scan::default()
    };
    drop(s);
    let exe = ctx.exe.clone();
    std::thread::spawn(move || {
        let (report, error) = match run_scan(&exe) {
            Ok(report) => (report, None),
            Err(e) => (Value::Null, Some(e.to_string())),
        };
        let mut s = SCAN.lock().unwrap();
        s.running = false;
        s.finished = s.started.map(|t| t.elapsed());
        s.report = report;
        s.error = error;
    });
}

fn run_scan(exe: &std::path::Path) -> Result<Value> {
    let mut child = std::process::Command::new(exe)
        .args(["doctor", "--json", "--progress"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("starting keymule doctor")?;
    // Drain stdout alongside: the report can be bigger than a pipe holds.
    let mut stdout = child.stdout.take().context("no stdout")?;
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        let _ = stdout.read_to_string(&mut out);
        out
    });
    let mut other = Vec::new();
    for line in BufReader::new(child.stderr.take().context("no stderr")?).lines() {
        let Ok(line) = line else { break };
        let Ok(p) = serde_json::from_str::<Value>(&line) else {
            other.push(line);
            continue;
        };
        let mut s = SCAN.lock().unwrap();
        if let Some(phase) = p["phase"].as_str() {
            s.phase = phase.into();
        }
        s.done = p["done"].as_u64().unwrap_or(s.done);
        s.total = p["total"].as_u64().unwrap_or(s.total);
        s.found = p["found"].as_u64().unwrap_or(s.found);
        // Big transcripts are read in pieces: say each place once in a row.
        if let Some(msg) = p["msg"].as_str()
            && s.log.last().is_none_or(|l| l != msg)
        {
            s.log.push(msg.to_string());
            s.lines += 1;
            if s.log.len() > SCAN_LOG {
                s.log.remove(0);
            }
        }
    }
    let status = child.wait()?;
    let out = reader.join().unwrap_or_default();
    if !status.success() {
        let msg = other.join("\n");
        anyhow::bail!(
            "{}",
            if msg.trim().is_empty() {
                "The scan failed."
            } else {
                msg.trim().trim_start_matches("keymule: ")
            }
        );
    }
    serde_json::from_str(&out).context("reading the scan's report")
}

fn scan_status() -> Value {
    let s = SCAN.lock().unwrap();
    let elapsed = s.finished.or_else(|| s.started.map(|t| t.elapsed()));
    json!({
        "running": s.running,
        "id": s.id,
        "elapsed_ms": elapsed.map(|d| d.as_millis() as u64),
        "phase": s.phase,
        "log": s.log,
        "lines": s.lines,
        "done": s.done,
        "total": s.total,
        "found": s.found,
        "report": s.report,
        "error": s.error,
    })
}

fn simple_name(s: &str) -> bool {
    s.len() <= 64
        && s.starts_with(|c: char| c.is_ascii_alphanumeric())
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// A host name or an IP address (no spaces, can't be taken for an option).
fn host_like(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 253
        && !s.starts_with('-')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '_' | '[' | ']'))
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

/// Pair a browser for page approvals. The first one needs the code printed
/// by `keymule ui` (or, on a Mac, a yes in a dialog); after that, a browser
/// that's already paired has to allow each new one.
fn pair(ctx: &Ctx, name: &str, key: &str, code: &str) -> Value {
    let vault = crate::store::vault();
    let device = match crate::approvals::new_device(name, key) {
        Ok(d) => d,
        Err(e) => return json!({"ok": false, "stderr": e.to_string()}),
    };
    let known = crate::approvals::devices(vault.as_ref());
    if known.iter().any(|d| d.id == device.id) {
        return json!({"ok": true, "id": device.id});
    }
    let code = code.replace([' ', '-'], "");
    let code_ok = !code.is_empty() && same(code.as_bytes(), ctx.code.replace('-', "").as_bytes());
    let allowed = if known.is_empty() {
        if !code.is_empty() {
            code_ok
        } else {
            first_pair_on_screen(&device.name)
        }
    } else if !code.is_empty() {
        if !ctx.code_repairs {
            return json!({"ok": false, "stderr": "another browser is paired, so the code only \
                works when `keymule ui` runs in your own terminal. Start it there and use the \
                code it prints"});
        }
        code_ok
    } else {
        matches!(
            crate::dialog::confirm(&format!(
                "Let \"{}\" answer keymule's questions?\n\nA new browser wants to pair. It \
                 could then approve anything keymule asks.",
                device.name
            )),
            Ok(true)
        )
    };
    if !allowed {
        return json!({"ok": false, "stderr": if !code.is_empty() {
            "that code doesn't match the one `keymule ui` printed"
        } else if known.is_empty() {
            "type the code `keymule ui` printed in its terminal"
        } else {
            "a paired browser didn't allow it"
        }});
    }
    let id = device.id.clone();
    match crate::approvals::add_device(vault.as_ref(), device) {
        Ok(()) => {
            DEVICES_GEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            crate::http::audit("-", "PAIR", &id, 0);
            json!({"ok": true, "id": id})
        }
        Err(e) => json!({"ok": false, "stderr": e.to_string()}),
    }
}

/// On a Mac, the first browser can be paired with a yes in a dialog.
fn first_pair_on_screen(name: &str) -> bool {
    crate::dialog::confirm_on_screen(&format!(
        "Pair \"{name}\" with keymule?\n\nIt will be able to answer keymule's questions in the page."
    ))
    .unwrap_or(false)
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
        assert!(
            host_like("203.0.113.7") && host_like("vps.example.com") && host_like("[2001:db8::1]")
        );
        assert!(!host_like("-oProxyCommand=x") && !host_like("a b") && !host_like(""));
    }
}
