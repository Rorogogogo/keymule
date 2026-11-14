//! Make an HTTP request on a credential's behalf. The secret is attached
//! here, only for hosts the credential is locked to, and scrubbed from
//! whatever comes back.

use std::collections::BTreeMap;
use std::io::Write;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use reqwest::header::{ACCEPT, HeaderName, HeaderValue, LOCATION};
use reqwest::{Method, StatusCode, Url};

use crate::presets;
use crate::store::{Auth, Credential, config_dir, host_of};

const MAX_REDIRECTS: usize = 5;
const HARD_BODY_CAP: usize = 20 * 1024 * 1024;

#[derive(Debug, Default, Clone)]
pub struct Request {
    pub method: String,
    /// "/rest/api/..." relative to the credential URL, or a full URL on an allowed host.
    pub path: String,
    pub query: BTreeMap<String, String>,
    pub headers: BTreeMap<String, String>,
    pub body: Option<serde_json::Value>,
    pub max_bytes: usize,
    /// Keep the end of a long body instead of the start (useful for logs).
    pub keep_tail: bool,
}

#[derive(Debug)]
pub struct Response {
    pub status: StatusCode,
    pub url: String,
    pub content_type: String,
    pub body: String,
    pub total_bytes: usize,
    pub truncated: bool,
}

impl Response {
    pub fn render(&self) -> String {
        let mut out = format!(
            "HTTP {} · {} · {}\n\n{}",
            self.status.as_u16(),
            self.url,
            if self.content_type.is_empty() {
                "-"
            } else {
                &self.content_type
            },
            self.body
        );
        if self.truncated {
            out.push_str(&format!(
                "\n\n[keymule: body is {} bytes, showing {} of it; pass a larger max_bytes, or keep_tail=true for logs]",
                self.total_bytes,
                if self.body.is_empty() { "none" } else { "part" }
            ));
        }
        out
    }
}

/// Resolve `path` against the credential and refuse anything off its hosts.
pub fn resolve_url(name: &str, cred: &Credential, path: &str) -> Result<Url> {
    let raw = if path.starts_with("http://") || path.starts_with("https://") {
        path.to_string()
    } else {
        if cred.url.is_empty() {
            bail!(
                "credential \"{name}\" has no base URL yet. Ask the user for it, then call \
                 update_credential (they approve the host in a dialog), or have them run \
                 `keymule edit {name} --url <base url>`."
            );
        }
        format!(
            "{}/{}",
            cred.url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    };
    let url = Url::parse(&raw).with_context(|| format!("invalid URL: {raw}"))?;
    let host = host_of(url.as_str())?;
    if !cred.hosts.iter().any(|h| h == &host) {
        bail!(
            "credential \"{name}\" may only be used with {}; refusing to send it to {host}",
            cred.hosts.join(", ")
        );
    }
    if !cred.url.is_empty() && url.scheme() == "http" && cred.url.starts_with("https://") {
        bail!("refusing to downgrade \"{name}\" from https to http");
    }
    Ok(url)
}

fn is_reserved_header(name: &str, cred: &Credential) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "authorization" | "proxy-authorization" | "cookie" | "host"
    ) || matches!(&cred.auth, Auth::Header { name: h } if h.eq_ignore_ascii_case(&lower))
}

fn same_origin_allowed(cred: &Credential, from: &Url, to: &Url) -> bool {
    to.scheme() == from.scheme()
        && host_of(to.as_str()).is_ok_and(|h| cred.hosts.iter().any(|a| a == &h))
}

pub async fn send(name: &str, cred: &Credential, secret: &str, req: &Request) -> Result<Response> {
    let method = Method::from_bytes(req.method.to_ascii_uppercase().as_bytes())
        .with_context(|| format!("invalid HTTP method: {}", req.method))?;
    if matches!(&cred.auth, Auth::Basic { username } if username.is_empty()) {
        bail!(
            "credential \"{name}\" needs a username. Ask the user for it, then call \
             update_credential, or have them run `keymule edit {name} --username <user>`."
        );
    }
    let mut url = resolve_url(name, cred, &req.path)?;
    if !req.query.is_empty() {
        url.query_pairs_mut().extend_pairs(req.query.iter());
    }
    for key in req.headers.keys() {
        if is_reserved_header(key, cred) {
            bail!("header {key} is set by keymule and cannot be overridden");
        }
    }

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(60))
        .user_agent(concat!("keymule/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let accept = presets::get_or_generic(&cred.service).accept;

    let mut method = method;
    let mut body = req.body.clone();
    let mut with_secret = true;
    let mut hops = 0;
    let resp = loop {
        let mut rb = client
            .request(method.clone(), url.clone())
            .header(ACCEPT, accept);
        for (k, v) in &req.headers {
            rb = rb.header(
                HeaderName::from_bytes(k.as_bytes())?,
                HeaderValue::from_str(v)?,
            );
        }
        if with_secret {
            rb = match &cred.auth {
                Auth::Bearer => rb.bearer_auth(secret),
                Auth::Basic { username } => rb.basic_auth(username, Some(secret)),
                Auth::Header { name } => {
                    rb.header(HeaderName::from_bytes(name.as_bytes())?, secret)
                }
            };
        }
        rb = match &body {
            Some(serde_json::Value::String(s)) => rb.body(s.clone()),
            Some(v) => rb.json(v),
            None => rb,
        };
        let resp = rb
            .send()
            .await
            .map_err(|e| anyhow::anyhow!(redact(&e.to_string(), secret)))?;

        let status = resp.status();
        let location = resp.headers().get(LOCATION).and_then(|v| v.to_str().ok());
        match (status.is_redirection(), location) {
            (true, Some(loc)) if hops < MAX_REDIRECTS => {
                let next = url.join(loc)?;
                // Never carry the secret to a host it is not locked to
                // (e.g. GitHub log downloads redirect to blob storage).
                if !same_origin_allowed(cred, &url, &next) {
                    with_secret = false;
                }
                if status == StatusCode::SEE_OTHER
                    || (matches!(status, StatusCode::MOVED_PERMANENTLY | StatusCode::FOUND)
                        && method == Method::POST)
                {
                    method = Method::GET;
                    body = None;
                }
                url = next;
                hops += 1;
            }
            _ => break resp,
        }
    };

    let status = resp.status();
    let final_url = resp.url().to_string();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let mut bytes: Vec<u8> = Vec::new();
    let mut resp = resp;
    while let Some(chunk) = resp.chunk().await? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() >= HARD_BODY_CAP {
            break;
        }
    }
    let total_bytes = bytes.len();
    let text = redact(&String::from_utf8_lossy(&bytes), secret);
    let (body, truncated) = clip(&text, req.max_bytes, req.keep_tail);

    audit(name, method.as_str(), &final_url, status.as_u16());
    Ok(Response {
        status,
        url: final_url,
        content_type,
        body,
        total_bytes,
        truncated,
    })
}

/// Check a token against the preset's verify endpoint. Returns who it
/// authenticated as, when the service says.
pub async fn verify(name: &str, cred: &Credential, secret: &str) -> Result<Option<String>> {
    let Some(path) = presets::get_or_generic(&cred.service).verify_path else {
        return Ok(None);
    };
    let resp = send(
        name,
        cred,
        secret,
        &Request {
            method: "GET".into(),
            path: path.into(),
            max_bytes: 64 * 1024,
            ..Default::default()
        },
    )
    .await?;
    if !resp.status.is_success() {
        bail!(
            "{} answered HTTP {} for {path}",
            cred.hosts
                .first()
                .map(String::as_str)
                .unwrap_or("the service"),
            resp.status.as_u16()
        );
    }
    let who = serde_json::from_str::<serde_json::Value>(&resp.body)
        .ok()
        .and_then(|v| {
            ["login", "username", "name", "fullName", "id"]
                .iter()
                .find_map(|k| v.get(*k).and_then(|x| x.as_str().map(String::from)))
        });
    Ok(Some(who.unwrap_or_else(|| "ok".into())))
}

/// `text` with `secret` taken out, also URL-encoded or base64.
pub fn redact(text: &str, secret: &str) -> String {
    crate::redact::scrub(text, secret)
}

fn clip(text: &str, max_bytes: usize, keep_tail: bool) -> (String, bool) {
    if max_bytes == 0 || text.len() <= max_bytes {
        return (text.to_string(), false);
    }
    let cut = if keep_tail {
        let mut start = text.len() - max_bytes;
        while !text.is_char_boundary(start) {
            start += 1;
        }
        &text[start..]
    } else {
        let mut end = max_bytes;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        &text[..end]
    };
    (cut.to_string(), true)
}

/// One line per request in ~/.config/keymule/audit.log. Best effort.
pub fn audit(name: &str, method: &str, url: &str, status: u16) {
    let Ok(dir) = config_dir() else { return };
    let _ = std::fs::create_dir_all(&dir);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let url = url.split('?').next().unwrap_or(url);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("audit.log"))
    {
        let _ = writeln!(f, "{ts}\t{name}\t{method}\t{url}\t{status}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redact::REDACTED;

    fn cred(url: &str) -> Credential {
        Credential {
            service: "bamboo".into(),
            url: url.into(),
            description: String::new(),
            auth: Auth::Bearer,
            hosts: vec![host_of(url).unwrap()],
            added_by: "cli".into(),
        }
    }

    #[test]
    fn resolves_relative_paths_under_base_path() {
        let c = cred("https://ci.corp.com/bamboo/");
        let u = resolve_url("b", &c, "/rest/api/latest/plan").unwrap();
        assert_eq!(
            u.as_str(),
            "https://ci.corp.com/bamboo/rest/api/latest/plan"
        );
    }

    #[test]
    fn refuses_other_hosts_and_downgrades() {
        let c = cred("https://ci.corp.com");
        assert!(resolve_url("b", &c, "https://evil.example/steal").is_err());
        assert!(resolve_url("b", &c, "https://ci.corp.com.evil.example/").is_err());
        assert!(resolve_url("b", &c, "http://ci.corp.com/rest").is_err());
        assert!(resolve_url("b", &c, "https://ci.corp.com/rest").is_ok());
    }

    #[test]
    fn missing_url_tells_user_what_to_run() {
        let mut c = cred("https://ci.corp.com");
        c.url.clear();
        let err = resolve_url("bamboo", &c, "/rest").unwrap_err().to_string();
        assert!(err.contains("keymule edit bamboo --url"));
    }

    #[test]
    fn auth_headers_cannot_be_overridden() {
        let mut c = cred("https://ci.corp.com");
        assert!(is_reserved_header("Authorization", &c));
        c.auth = Auth::Header {
            name: "PRIVATE-TOKEN".into(),
        };
        assert!(is_reserved_header("private-token", &c));
        assert!(!is_reserved_header("X-Request-Id", &c));
    }

    #[test]
    fn redacts_and_clips() {
        assert_eq!(
            redact("tok=abcd1234!", "abcd1234"),
            format!("tok={REDACTED}!")
        );
        let (head, t) = clip("héllo world", 3, false);
        assert!(t && head == "h\u{e9}");
        let (tail, _) = clip("error at the end", 7, true);
        assert_eq!(tail, "the end");
    }
}
