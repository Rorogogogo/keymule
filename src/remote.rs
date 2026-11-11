//! `keymule remote <name> <url>`: a stdio MCP server that forwards every
//! message to a remote (Streamable HTTP) MCP server, adding the headers
//! keymule holds. The agent config then has no API key in it, only the URL,
//! and the headers go to the URLs the user approved and nowhere else.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use reqwest::StatusCode;
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

use crate::servers::{Binding, approve_url};
use crate::store::Vault;

const SESSION: &str = "mcp-session-id";
const PROTOCOL: &str = "mcp-protocol-version";
const HIDDEN: &str = "[hidden by keymule]";

/// Headers keymule sets itself, so `--header` can't.
pub const RESERVED: &[&str] = &[
    "host",
    "content-length",
    "content-type",
    "accept",
    "connection",
    "transfer-encoding",
    SESSION,
    PROTOCOL,
];

/// Whether keymule will send secrets to `url`: https, or http to this machine.
pub fn url_ok(url: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(url) else {
        return false;
    };
    match u.scheme() {
        "https" => true,
        "http" => matches!(
            u.host_str(),
            Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
        ),
        _ => false,
    }
}

pub async fn run(vault: &dyn Vault, name: &str, extra: &[String], url: &str) -> Result<()> {
    if !url_ok(url) {
        bail!(
            "{url} isn't an https URL; keymule only sends secrets over https (or http to localhost)"
        );
    }
    let mut binding = Binding::load(vault, name)
        .with_context(|| format!("keymule has no MCP server secret named \"{name}\""))?;
    if binding.headers.is_empty() {
        bail!(
            "\"{name}\" holds no HTTP headers; for a local server use `keymule exec {name} -- <command>`"
        );
    }
    if !binding.urls.iter().any(|u| u == url) {
        approve_url(name, &binding, url)?;
        binding.allow_url(url);
        binding.save(vault, name)?;
    }

    let mut headers = HeaderMap::new();
    for (k, v) in &binding.headers {
        let mut value = HeaderValue::from_str(v)
            .with_context(|| format!("the {k} header keymule holds isn't a valid header value"))?;
        value.set_sensitive(true);
        headers.insert(HeaderName::from_bytes(k.as_bytes())?, value);
    }
    for h in extra {
        let (k, v) = h
            .split_once(':')
            .ok_or_else(|| anyhow!("--header wants \"Name: value\", got {h:?}"))?;
        let k = k.trim();
        if RESERVED.contains(&k.to_ascii_lowercase().as_str())
            || binding.headers.keys().any(|s| s.eq_ignore_ascii_case(k))
        {
            bail!("--header can't set {k}");
        }
        headers.insert(
            HeaderName::from_bytes(k.as_bytes())?,
            HeaderValue::from_str(v.trim())?,
        );
    }

    let mut redact: Vec<String> = Vec::new();
    for v in binding.headers.values() {
        let token = v.split_once(' ').map(|(_, t)| t).unwrap_or(v).trim();
        for s in [v.as_str(), token] {
            if s.len() >= 8 && !redact.iter().any(|r| r == s) {
                redact.push(s.to_string());
            }
        }
    }
    redact.sort_by_key(|s| std::cmp::Reverse(s.len()));

    let client = reqwest::Client::builder()
        // A redirect could take the headers to another host.
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(30))
        .user_agent(concat!("keymule/", env!("CARGO_PKG_VERSION")))
        .build()?;

    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let writer = tokio::spawn(async move {
        let mut out = tokio::io::stdout();
        while let Some(line) = rx.recv().await {
            if out.write_all(line.as_bytes()).await.is_err()
                || out.write_all(b"\n").await.is_err()
                || out.flush().await.is_err()
            {
                break;
            }
        }
    });

    let bridge = Arc::new(Bridge {
        client,
        url: url.to_string(),
        headers,
        session: Mutex::new(None),
        protocol: Mutex::new(None),
        out: tx,
        redact,
    });
    crate::http::audit(name, "START", url, 0);

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            eprintln!("keymule remote: ignoring a line that isn't JSON");
            continue;
        };
        tokio::spawn(bridge.clone().post(msg));
    }
    bridge.close().await;
    drop(bridge);
    let _ = tokio::time::timeout(Duration::from_secs(2), writer).await;
    Ok(())
}

struct Bridge {
    client: reqwest::Client,
    url: String,
    headers: HeaderMap,
    session: Mutex<Option<String>>,
    protocol: Mutex<Option<String>>,
    out: mpsc::UnboundedSender<String>,
    /// Secret values to blank out of anything the server sends back.
    redact: Vec<String>,
}

impl Bridge {
    fn request(&self, method: reqwest::Method) -> reqwest::RequestBuilder {
        let mut req = self
            .client
            .request(method, &self.url)
            .headers(self.headers.clone());
        if let Some(s) = self.session.lock().unwrap().clone() {
            req = req.header(SESSION, s);
        }
        if let Some(p) = self.protocol.lock().unwrap().clone() {
            req = req.header(PROTOCOL, p);
        }
        req
    }

    async fn post(self: Arc<Self>, msg: Value) {
        let id = msg.get("method").and(msg.get("id")).cloned();
        let initialized =
            msg.get("method").and_then(Value::as_str) == Some("notifications/initialized");
        match self.send(&msg).await {
            Ok(()) if initialized => {
                tokio::spawn(self.clone().listen());
            }
            Ok(()) => {}
            Err(e) => {
                let e = self.clean(&format!("{e:#}"));
                eprintln!("keymule remote: {e}");
                if let Some(id) = id {
                    self.emit(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {"code": -32000, "message": format!("keymule: {e}")},
                    }));
                }
            }
        }
    }

    async fn send(&self, msg: &Value) -> Result<()> {
        let is_init = msg.get("method").and_then(Value::as_str) == Some("initialize");
        let resp = self
            .request(reqwest::Method::POST)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json, text/event-stream")
            .body(serde_json::to_vec(msg)?)
            .send()
            .await
            .map_err(|e| anyhow!("can't reach {}: {}", self.url, e.without_url()))?;
        let status = resp.status();
        if let Some(s) = resp.headers().get(SESSION).and_then(|v| v.to_str().ok()) {
            *self.session.lock().unwrap() = Some(s.to_string());
        }
        if status == StatusCode::NOT_FOUND
            && !is_init
            && self.session.lock().unwrap().take().is_some()
        {
            bail!("the server ended the session; restart this MCP server");
        }
        if status.is_redirection() {
            bail!(
                "{} redirects elsewhere, and keymule doesn't follow redirects with your secret; put the new URL in the config",
                self.url
            );
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            let body: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
            let body: String = body.chars().take(200).collect();
            let sep = if body.is_empty() { "" } else { ": " };
            bail!("{} answered HTTP {}{sep}{body}", self.url, status.as_u16());
        }
        if status == StatusCode::ACCEPTED || status == StatusCode::NO_CONTENT {
            return Ok(());
        }
        if is_sse(&resp) {
            self.read_sse(resp).await
        } else {
            let text = resp.text().await?;
            if text.trim().is_empty() {
                return Ok(());
            }
            let v: Value = serde_json::from_str(&text)
                .with_context(|| format!("{} answered something that isn't JSON", self.url))?;
            self.emit(v);
            Ok(())
        }
    }

    /// The optional stream of messages the server sends on its own.
    async fn listen(self: Arc<Self>) {
        let mut failures = 0;
        while failures < 5 {
            let started = Instant::now();
            let resp = self
                .request(reqwest::Method::GET)
                .header(ACCEPT, "text/event-stream")
                .send()
                .await;
            match resp {
                Ok(r) if r.status().is_success() && is_sse(&r) => {
                    let _ = self.read_sse(r).await;
                    if started.elapsed() < Duration::from_secs(5) {
                        failures += 1;
                    } else {
                        failures = 0;
                    }
                }
                // 405 and the like: the server doesn't offer one.
                Ok(_) => return,
                Err(_) => failures += 1,
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    async fn read_sse(&self, mut resp: reqwest::Response) -> Result<()> {
        let mut buf: Vec<u8> = Vec::new();
        let mut data = String::new();
        while let Some(chunk) = resp.chunk().await? {
            buf.extend_from_slice(&chunk);
            while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                let raw: Vec<u8> = buf.drain(..=pos).collect();
                let line = String::from_utf8_lossy(&raw);
                let line = line.trim_end_matches(['\n', '\r']);
                if line.is_empty() {
                    self.dispatch(&mut data);
                } else if let Some(rest) = line.strip_prefix("data:") {
                    if !data.is_empty() {
                        data.push('\n');
                    }
                    data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
                }
            }
        }
        self.dispatch(&mut data);
        Ok(())
    }

    fn dispatch(&self, data: &mut String) {
        if data.is_empty() {
            return;
        }
        match serde_json::from_str::<Value>(data) {
            Ok(v) => self.emit(v),
            Err(_) => eprintln!("keymule remote: skipped an event that isn't JSON"),
        }
        data.clear();
    }

    fn emit(&self, v: Value) {
        if let Some(p) = v
            .pointer("/result/protocolVersion")
            .and_then(Value::as_str)
            .filter(|_| v.pointer("/result/serverInfo").is_some())
        {
            *self.protocol.lock().unwrap() = Some(p.to_string());
        }
        let line = serde_json::to_string(&v).unwrap_or_default();
        let _ = self.out.send(self.clean(&line));
    }

    fn clean(&self, s: &str) -> String {
        let mut s = s.to_string();
        for secret in &self.redact {
            if s.contains(secret.as_str()) {
                s = s.replace(secret.as_str(), HIDDEN);
            }
        }
        s
    }

    async fn close(&self) {
        if self.session.lock().unwrap().is_none() {
            return;
        }
        let _ = self
            .request(reqwest::Method::DELETE)
            .timeout(Duration::from_secs(3))
            .send()
            .await;
    }
}

fn is_sse(resp: &reqwest::Response) -> bool {
    resp.headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|t| t.starts_with("text/event-stream"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_or_localhost() {
        assert!(url_ok("https://mcp.example.com/mcp"));
        assert!(url_ok("http://127.0.0.1:8080/mcp"));
        assert!(url_ok("http://localhost/mcp"));
        assert!(!url_ok("http://mcp.example.com/mcp"));
        assert!(!url_ok("ftp://example.com"));
        assert!(!url_ok("not a url"));
    }
}
