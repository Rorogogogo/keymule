//! Website logins, filled into a browser the agent drives without the agent
//! ever holding the password.
//!
//! `keymule browser -- <browser MCP server>` starts a Chrome with keymule's
//! own profile (or reuses the one already running), points the browser tool
//! (Playwright MCP, Chrome DevTools MCP) at it, and sits between the agent
//! and the tool, hiding stored website passwords from every reply.
//!
//! The `sign_in` MCP tool then connects to the same Chrome, finds the tab
//! that is on the login's site, asks the user, and types the username and
//! password into the page itself. The site is sealed with the secret, and the
//! tab's origin is checked again right before each field is filled, so a
//! lookalike page (or one the sign-in redirected to) never gets it.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result, anyhow, bail};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio_tungstenite::tungstenite::Message;

use crate::presets;
use crate::store::{self, Auth, Credential, Sealed, Store, Vault};

/// What replaces a stored website password in the browser tool's replies.
pub const HIDDEN: &str = "[website password hidden by keymule]";

/// Where the running keymule Chrome is: `<config>/browser/state.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct State {
    pid: u32,
    port: u16,
    ws: String,
}

fn browser_dir() -> Result<PathBuf> {
    Ok(store::config_dir()?.join("browser"))
}

fn state_path() -> Result<PathBuf> {
    Ok(browser_dir()?.join("state.json"))
}

// ---- the site a login is for ----------------------------------------------

fn is_local(host: &str) -> bool {
    let name = host.rsplit_once(':').map_or(host, |(h, _)| h);
    matches!(name, "localhost" | "127.0.0.1" | "[::1]") || name.ends_with(".localhost")
}

/// The origin (`https://github.com`) a login with this sealed host fills
/// into. Logins are https, except to this machine, where `url`'s scheme
/// (from credentials.toml) decides.
pub fn origin_for(host: &str, url: &str) -> String {
    let scheme = if is_local(host) && url.starts_with("http://") {
        "http"
    } else {
        "https"
    };
    format!("{scheme}://{host}")
}

/// The origin of a page's URL, as `location.origin` gives it.
fn origin_of(url: &str) -> Option<String> {
    let o = reqwest::Url::parse(url).ok()?.origin();
    o.is_tuple().then(|| o.ascii_serialization())
}

/// Check a URL the user gave for a website login and return its sealed host.
pub fn login_host(url: &str) -> Result<String> {
    let host = store::host_of(url)?;
    if !url.starts_with("https://") && !is_local(&host) {
        bail!("a website login is only filled into https pages; use the https:// address");
    }
    Ok(host)
}

// ---- finding and starting Chrome -------------------------------------------

fn chrome_path() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("KEYMULE_CHROME") {
        return Ok(PathBuf::from(p));
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    #[cfg(target_os = "macos")]
    {
        let apps = [
            "Google Chrome.app/Contents/MacOS/Google Chrome",
            "Chromium.app/Contents/MacOS/Chromium",
            "Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "Brave Browser.app/Contents/MacOS/Brave Browser",
        ];
        let home = std::env::var_os("HOME").map(PathBuf::from);
        for app in apps {
            candidates.push(Path::new("/Applications").join(app));
            if let Some(h) = &home {
                candidates.push(h.join("Applications").join(app));
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        for base in ["ProgramFiles", "ProgramFiles(x86)", "LocalAppData"] {
            if let Some(b) = std::env::var_os(base) {
                candidates.push(PathBuf::from(&b).join(r"Google\Chrome\Application\chrome.exe"));
                candidates.push(PathBuf::from(&b).join(r"Microsoft\Edge\Application\msedge.exe"));
            }
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let names = [
            "google-chrome",
            "google-chrome-stable",
            "chromium",
            "chromium-browser",
            "microsoft-edge",
        ];
        if let Some(path) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path) {
                for n in names {
                    candidates.push(dir.join(n));
                }
            }
        }
    }
    candidates.into_iter().find(|p| p.is_file()).ok_or_else(|| {
        anyhow!("keymule couldn't find Chrome (or Chromium, Edge, Brave). Install one, or set KEYMULE_CHROME to its program")
    })
}

/// The DevTools address of a running Chrome, if `state` still answers.
async fn alive(state: &State) -> bool {
    let url = format!("http://127.0.0.1:{}/json/version", state.port);
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    else {
        return false;
    };
    match client.get(&url).send().await {
        Ok(r) => r
            .json::<Value>()
            .await
            .ok()
            .and_then(|v| v["webSocketDebuggerUrl"].as_str().map(|w| w == state.ws))
            .unwrap_or(false),
        Err(_) => false,
    }
}

async fn running() -> Option<State> {
    let text = std::fs::read_to_string(state_path().ok()?).ok()?;
    let state: State = serde_json::from_str(&text).ok()?;
    alive(&state).await.then_some(state)
}

/// keymule's Chrome: the one already running, or a new one. The bool says
/// whether this call started it.
async fn ensure_chrome() -> Result<(State, Option<std::process::Child>)> {
    if let Some(s) = running().await {
        return Ok((s, None));
    }
    let dir = browser_dir()?;
    let profile = dir.join("profile");
    std::fs::create_dir_all(&profile)?;
    let port_file = profile.join("DevToolsActivePort");
    let _ = std::fs::remove_file(&port_file);
    let chrome = chrome_path()?;
    let mut cmd = std::process::Command::new(&chrome);
    cmd.arg(format!("--user-data-dir={}", profile.display()))
        .arg("--remote-debugging-port=0")
        .arg("--remote-debugging-address=127.0.0.1")
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("about:blank")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let no_screen = cfg!(target_os = "linux")
        && std::env::var_os("DISPLAY").is_none()
        && std::env::var_os("WAYLAND_DISPLAY").is_none();
    if no_screen || std::env::var_os("KEYMULE_BROWSER_HEADLESS").is_some_and(|v| v == "1") {
        cmd.arg("--headless=new");
    }
    let child = cmd
        .spawn()
        .with_context(|| format!("starting {}", chrome.display()))?;
    // Chrome writes the port it picked, then the browser's WebSocket path.
    let deadline = Instant::now() + Duration::from_secs(30);
    let (port, path) = loop {
        if let Ok(text) = std::fs::read_to_string(&port_file) {
            let mut lines = text.lines();
            if let (Some(p), Some(path)) = (lines.next(), lines.next())
                && let Ok(port) = p.trim().parse::<u16>()
            {
                break (port, path.trim().to_string());
            }
        }
        if Instant::now() > deadline {
            bail!(
                "Chrome didn't open its DevTools port within 30 seconds. Is another Chrome using {}?",
                profile.display()
            );
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    let state = State {
        pid: child.id(),
        port,
        ws: format!("ws://127.0.0.1:{port}{path}"),
    };
    std::fs::write(state_path()?, serde_json::to_string(&state)?)?;
    Ok((state, Some(child)))
}

// ---- `keymule browser -- <server>` ------------------------------------------

/// The flag that points a browser MCP server at an existing Chrome.
fn endpoint_args(command: &[String], ws: &str) -> Result<Vec<String>> {
    let joined = command.join(" ");
    if joined.contains("playwright") {
        Ok(vec!["--cdp-endpoint".into(), ws.into()])
    } else if joined.contains("chrome-devtools-mcp") {
        Ok(vec![format!("--wsEndpoint={ws}")])
    } else {
        bail!(
            "keymule browser knows Playwright MCP (@playwright/mcp) and Chrome DevTools MCP \
             (chrome-devtools-mcp). Run one of them through it, e.g.\n  keymule browser -- npx @playwright/mcp@latest"
        )
    }
}

/// Every stored website password, in every form it could turn up in.
struct Hidden {
    forms: Vec<String>,
    stamp: Stamp,
}

/// When the files a login lives in last changed: credentials.toml (a new
/// login) and the vault (a new or rotated password).
type Stamp = Vec<Option<SystemTime>>;

impl Hidden {
    fn new() -> Self {
        let mut h = Self {
            forms: vec![],
            stamp: vec![],
        };
        h.reload();
        h
    }

    fn files_stamp() -> Stamp {
        let mut files = vec![];
        if let Ok(dir) = store::config_dir() {
            files.push(dir.join("credentials.toml"));
            files.push(dir.join("vault.enc"));
        }
        if let Some(test) = std::env::var_os("KEYMULE_TEST_VAULT") {
            files.push(PathBuf::from(test));
        }
        files
            .iter()
            .map(|f| std::fs::metadata(f).and_then(|m| m.modified()).ok())
            .collect()
    }

    fn reload(&mut self) {
        self.stamp = Self::files_stamp();
        let Ok(store) = Store::load() else { return };
        let vault = store::vault();
        let mut forms: Vec<String> = store
            .creds
            .iter()
            .filter(|(_, c)| presets::get_or_generic(&c.service).kind == presets::Kind::Login)
            .filter_map(|(n, c)| Sealed::load(vault.as_ref(), n, c).ok())
            .flat_map(|s| {
                let mut f = crate::redact::forms(&s.secret);
                // Inside a JSON string, quotes and backslashes are escaped.
                if let Ok(j) = serde_json::to_string(&s.secret) {
                    f.push(j[1..j.len() - 1].to_string());
                }
                f
            })
            .filter(|f| f.len() >= 4)
            .collect();
        forms.sort_by_key(|f| std::cmp::Reverse(f.len()));
        forms.dedup();
        self.forms = forms;
    }

    /// Hide every stored password in one line of the server's output. A
    /// login added or rotated since is picked up before the line goes out
    /// (checking costs a couple of file stats).
    fn scrub(&mut self, line: &str) -> String {
        if Self::files_stamp() != self.stamp {
            self.reload();
        }
        let mut out = line.to_string();
        for f in &self.forms {
            if out.contains(f.as_str()) {
                out = out.replace(f.as_str(), HIDDEN);
            }
        }
        out
    }
}

pub async fn run(command: &[String]) -> Result<()> {
    let (program, rest) = command.split_first().ok_or_else(|| {
        anyhow!(
            "say which browser MCP server to run: keymule browser -- npx @playwright/mcp@latest"
        )
    })?;
    let (state, mut chrome) = ensure_chrome().await?;
    let mut args: Vec<String> = rest.to_vec();
    args.extend(endpoint_args(command, &state.ws)?);
    let mut child = tokio::process::Command::new(program)
        .args(&args)
        .env("KEYMULE_BROWSER_WS", &state.ws)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("starting {program}"))?;
    let mut child_in = child.stdin.take().expect("piped");
    let child_out = child.stdout.take().expect("piped");

    // Agent -> browser tool, as is.
    let to_child = tokio::spawn(async move {
        let mut stdin = tokio::io::stdin();
        let _ = tokio::io::copy(&mut stdin, &mut child_in).await;
        let _ = child_in.shutdown().await;
    });
    // Browser tool -> agent, one JSON-RPC message per line, passwords hidden.
    let to_agent = tokio::spawn(async move {
        let mut hidden = tokio::task::spawn_blocking(Hidden::new)
            .await
            .unwrap_or_else(|_| Hidden {
                forms: vec![],
                stamp: vec![],
            });
        let mut lines = BufReader::new(child_out).lines();
        let mut out = tokio::io::stdout();
        while let Ok(Some(line)) = lines.next_line().await {
            let line = hidden.scrub(&line);
            if out.write_all(line.as_bytes()).await.is_err()
                || out.write_all(b"\n").await.is_err()
                || out.flush().await.is_err()
            {
                break;
            }
        }
    });
    let status = child.wait().await?;
    to_child.abort();
    let _ = to_agent.await;
    // The Chrome this call started goes with it; one another call started
    // stays for that call.
    if let Some(c) = chrome.as_mut() {
        let _ = c.kill();
        let _ = c.wait();
        let _ = std::fs::remove_file(state_path()?);
    }
    std::process::exit(status.code().unwrap_or(1));
}

// ---- talking to Chrome -------------------------------------------------------

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct Cdp {
    ws: Socket,
    next: u64,
}

impl Cdp {
    async fn connect(url: &str) -> Result<Self> {
        let (ws, _) = tokio_tungstenite::connect_async(url)
            .await
            .context("connecting to the keymule browser")?;
        Ok(Self { ws, next: 0 })
    }

    async fn call(&mut self, method: &str, params: Value, session: Option<&str>) -> Result<Value> {
        self.next += 1;
        let id = self.next;
        let mut msg = json!({ "id": id, "method": method, "params": params });
        if let Some(s) = session {
            msg["sessionId"] = s.into();
        }
        self.ws.send(Message::Text(msg.to_string().into())).await?;
        let wait = async {
            while let Some(m) = self.ws.next().await {
                let Message::Text(t) = m? else { continue };
                let v: Value = serde_json::from_str(&t)?;
                if v["id"].as_u64() != Some(id) {
                    continue; // an event, or another reply
                }
                if let Some(e) = v.get("error") {
                    bail!("{}", e["message"].as_str().unwrap_or("Chrome refused"));
                }
                return Ok(v["result"].clone());
            }
            bail!("the keymule browser closed the connection")
        };
        tokio::time::timeout(Duration::from_secs(20), wait)
            .await
            .map_err(|_| anyhow!("the keymule browser didn't answer"))?
    }

    /// Run `fill.js`'s function in the tab with `arg`, returning its result.
    /// None when the page was navigating (its context went away).
    async fn run(&mut self, session: &str, func: &str, arg: &Value) -> Result<Option<Value>> {
        let expr = format!("({func})({arg})");
        let r = self
            .call(
                "Runtime.evaluate",
                json!({ "expression": expr, "awaitPromise": true, "returnByValue": true }),
                Some(session),
            )
            .await;
        match r {
            Ok(v) if v.get("exceptionDetails").is_some() => Ok(None),
            Ok(v) => Ok(Some(v["result"]["value"].clone())),
            Err(e) if e.to_string().contains("context") => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// Fills the login form in the page. It checks the page's origin first,
/// every time, and returns {status}: "submitted", "username" (filled the
/// username of a two-step form), "wait" (nothing to fill yet), "nofield",
/// or "origin" (with `at`: the page is somewhere else).
const FILL: &str = r#"async ({ origin, username, password, phase }) => {
  if (location.origin !== origin) return { status: 'origin', at: location.origin };
  const visible = (el) => {
    if (!el || el.disabled || el.readOnly) return false;
    const r = el.getBoundingClientRect(), s = getComputedStyle(el);
    return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.display !== 'none';
  };
  const all = (root, sel) => [...root.querySelectorAll(sel)].filter(visible);
  const set = (el, v) => {
    el.focus();
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, v);
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
  };
  const pws = all(document, 'input[type="password"]');
  const pw = pws.find((e) => !/new-password/.test(e.autocomplete)) || pws[0];
  const scope = (pw && pw.form) || document;
  const textish = (e) => ['text', 'email', 'tel'].includes(e.type);
  let user = all(scope, 'input[autocomplete~="username"], input[autocomplete~="email"], input[type="email"], input[name*="user" i], input[name*="email" i], input[name*="login" i], input[id*="user" i], input[id*="email" i], input[id*="login" i]').filter(textish)[0];
  if (!user && pw) {
    user = all(scope, 'input').filter(textish)
      .filter((e) => e.compareDocumentPosition(pw) & Node.DOCUMENT_POSITION_FOLLOWING).pop();
  }
  if (!pw && (phase > 0 || !user)) return { status: phase > 0 ? 'wait' : 'nofield' };
  if (user && user.value !== username) set(user, username);
  if (pw) set(pw, password);
  const field = pw || user;
  const form = field.form;
  const root = form || document;
  const button = all(root, 'button[type="submit"], input[type="submit"]')[0]
    || all(root, 'button, [role="button"]').find((b) => /^\s*(sign in|log in|login|continue|next|submit)\s*$/i.test(b.textContent || b.value || ''));
  await new Promise((r) => setTimeout(r, 200));
  if (location.origin !== origin) return { status: 'origin', at: location.origin };
  // Gone after a page load: AFTER tells a reloaded sign-in page (a refusal)
  // from the same page still waiting.
  if (pw) window.__keymuleSubmitted = true;
  if (button) button.click();
  else if (form && form.requestSubmit) form.requestSubmit();
  else field.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', code: 'Enter', keyCode: 13, bubbles: true }));
  return { status: pw ? 'submitted' : 'username' };
}"#;

/// After submitting: where the tab is, whether a sign-in form is still
/// showing (`form`: a visible password field, filled or not), whether the
/// page loaded anew since (`fresh`), whether it has finished loading
/// (`ready`: mid-redirect a page is blank, which isn't a verdict), and
/// whether a field still holds a value
/// (clearing it with `clear`, so the password doesn't sit in the page).
const AFTER: &str = r#"({ clear }) => {
  const all = [...document.querySelectorAll('input[type="password"]')];
  const held = all.filter((e) => e.value);
  if (clear) held.forEach((e) => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(e, '');
    e.dispatchEvent(new Event('input', { bubbles: true }));
  });
  const form = all.some((e) => {
    const r = e.getBoundingClientRect(), s = getComputedStyle(e);
    return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.display !== 'none';
  });
  return {
    url: location.href, held: held.length > 0, form, fresh: !window.__keymuleSubmitted,
    ready: document.readyState === 'complete',
  };
}"#;

/// What `sign_in` needs from a stored website login.
pub struct Login {
    pub origin: String,
    pub username: String,
    pub password: String,
}

/// The website login `name`, with its site and username as sealed.
pub fn load_login(vault: &dyn Vault, name: &str) -> Result<Login> {
    let store = Store::load()?;
    let cred = store.get(name)?;
    if presets::get_or_generic(&cred.service).kind != presets::Kind::Login {
        bail!(
            "\"{name}\" isn't a website login; sign_in only fills logins added with add_website_login"
        );
    }
    let sealed = Sealed::load(vault, name, cred)?;
    let cred = sealed.apply(cred);
    let [host] = cred.hosts.as_slice() else {
        bail!("\"{name}\" has no site set; add it again with add_website_login");
    };
    let Auth::Basic { username } = &cred.auth else {
        bail!("\"{name}\" has no username; set it with update_credential(username=...)");
    };
    Ok(Login {
        origin: origin_for(host, &cred.url),
        username: username.clone(),
        password: sealed.secret,
    })
}

/// A new website login's metadata (its password goes in a [`Sealed`]).
pub fn login_credential(url: &str, username: &str, description: String) -> Result<Credential> {
    let host = login_host(url)?;
    if username.trim().is_empty() {
        bail!("a website login needs the username (or email) the user signs in with");
    }
    Ok(Credential {
        service: "login".into(),
        url: origin_for(&host, url),
        description,
        auth: Auth::Basic {
            username: username.trim().to_string(),
        },
        hosts: vec![host],
        added_by: "mcp".into(),
        ssh: None,
    })
}

/// The tab to fill: the first one on `origin`.
async fn find_tab(cdp: &mut Cdp, origin: &str) -> Result<(String, Vec<String>)> {
    let targets = cdp.call("Target.getTargets", json!({}), None).await?;
    let pages: Vec<(String, String)> = targets["targetInfos"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|t| t["type"] == "page")
        .filter_map(|t| {
            Some((
                t["targetId"].as_str()?.to_string(),
                t["url"].as_str()?.to_string(),
            ))
        })
        .collect();
    let open: Vec<String> = pages.iter().map(|(_, u)| u.clone()).collect();
    match pages
        .into_iter()
        .find(|(_, u)| origin_of(u).as_deref() == Some(origin))
    {
        Some((id, _)) => Ok((id, open)),
        None => Ok((String::new(), open)),
    }
}

pub const NOT_RUNNING: &str = "The keymule browser isn't running. sign_in fills logins into the \
Chrome that `keymule browser` starts, so run your browser tool through it, e.g. for Claude Code:\n  \
claude mcp add playwright -- keymule browser -- npx @playwright/mcp@latest\n\
then reconnect it (/mcp) and open the site's sign-in page with it.";

/// Fill `login` into the keymule browser's tab on its site, after `ask`
/// says yes. Returns what happened, for the agent.
pub async fn sign_in(
    name: &str,
    login: Login,
    ask: impl FnOnce(&str) -> Result<bool>,
) -> Result<String> {
    let state = running().await.ok_or_else(|| anyhow!(NOT_RUNNING))?;
    let mut cdp = Cdp::connect(&state.ws).await?;
    let (target, open) = find_tab(&mut cdp, &login.origin).await?;
    if target.is_empty() {
        let open = if open.is_empty() {
            "no tabs".to_string()
        } else {
            open.join(", ")
        };
        bail!(
            "No tab in the keymule browser is on {} (open: {open}). Open the site's sign-in page \
             with your browser tool first, then call sign_in again.",
            login.origin
        );
    }
    let host = login.origin.split("://").nth(1).unwrap_or(&login.origin);
    let question = format!(
        "Sign in to {host} as {}?\n\nYour agent asked to use \"{name}\". keymule types the \
         password into the page on {}; the agent never sees it.",
        login.username, login.origin
    );
    if !ask(&question)? {
        bail!("The user didn't allow signing in to {host}. Nothing was filled.");
    }
    let session = cdp
        .call(
            "Target.attachToTarget",
            json!({ "targetId": target, "flatten": true }),
            None,
        )
        .await?["sessionId"]
        .as_str()
        .ok_or_else(|| anyhow!("couldn't attach to the tab"))?
        .to_string();
    let arg = |phase: u8| json!({ "origin": login.origin, "username": login.username, "password": login.password, "phase": phase });
    let mut phase = 0;
    let deadline = Instant::now() + Duration::from_secs(20);
    let result = loop {
        let r = cdp.run(&session, FILL, &arg(phase)).await?;
        let status = r
            .as_ref()
            .and_then(|v| v["status"].as_str())
            .unwrap_or("wait");
        match status {
            "submitted" => break Ok(()),
            "username" => phase = 1,
            "origin" => {
                let at = r
                    .as_ref()
                    .and_then(|v| v["at"].as_str())
                    .unwrap_or("another site");
                break Err(anyhow!(
                    "The tab moved to {at}, so keymule stopped: \"{name}\" is only filled on {}. \
                     If the site signs in somewhere else (single sign-on), that needs its own login.",
                    login.origin
                ));
            }
            "nofield" => {
                break Err(anyhow!(
                    "keymule found no sign-in form on the tab ({}). Open the page with the \
                     username and password fields, then call sign_in again.",
                    login.origin
                ));
            }
            _ => {}
        }
        if Instant::now() > deadline {
            break Err(anyhow!(
                "The password field didn't appear within 20 seconds of entering the username. \
                 Take a snapshot to see what the site is asking for."
            ));
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    };
    // Wait for the outcome: the sign-in form gone (signed in), or a new page
    // that shows it again (refused). A form still up on the same page after
    // 8 seconds means the site wants something else (a code, a CAPTCHA) or
    // showed an error in place. Then make sure the password doesn't stay.
    let mut after = None;
    if result.is_ok() {
        for _ in 0..16 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            if let Some(v) = cdp.run(&session, AFTER, &json!({ "clear": false })).await? {
                let form = v["form"].as_bool().unwrap_or(false);
                let fresh = v["fresh"].as_bool().unwrap_or(false);
                let ready = v["ready"].as_bool().unwrap_or(false);
                after = Some(v);
                if ready && (!form || fresh) {
                    break;
                }
            }
        }
    }
    let _ = cdp.run(&session, AFTER, &json!({ "clear": true })).await;
    let _ = cdp
        .call(
            "Target.detachFromTarget",
            json!({ "sessionId": session }),
            None,
        )
        .await;
    result?;
    let url = after
        .as_ref()
        .and_then(|v| v["url"].as_str())
        .unwrap_or(&login.origin)
        .to_string();
    let still = after
        .as_ref()
        .is_none_or(|v| v["form"].as_bool().unwrap_or(false));
    // In the activity log: 200 when the form went away, 401 when it's still there.
    crate::http::audit(
        name,
        "SIGN-IN",
        &login.origin,
        if still { 401 } else { 200 },
    );
    Ok(if still {
        format!(
            "keymule filled \"{name}\" on {} and submitted, but the sign-in form is still there \
             (now {url}): the password may be wrong, or the site wants more (a code, a CAPTCHA). \
             Take a snapshot to see. If the password changed, offer rotate_credential.",
            login.origin
        )
    } else {
        format!(
            "Signed in to {host} as {} (the tab is now on {url}). Take a snapshot to carry on.",
            login.username
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logins_are_for_one_exact_origin() {
        assert_eq!(
            origin_for("github.com", "https://github.com/login"),
            "https://github.com"
        );
        // An http URL in the file can't downgrade a real site.
        assert_eq!(
            origin_for("github.com", "http://github.com"),
            "https://github.com"
        );
        assert_eq!(
            origin_for("localhost:3000", "http://localhost:3000/login"),
            "http://localhost:3000"
        );
        assert_eq!(
            origin_of("https://github.com/login?x=1").as_deref(),
            Some("https://github.com")
        );
        assert_ne!(
            origin_of("https://github.com.evil.io/login").as_deref(),
            Some("https://github.com")
        );
        assert_ne!(
            origin_of("http://github.com/login").as_deref(),
            Some("https://github.com")
        );
        assert!(login_host("http://github.com/login").is_err());
        assert_eq!(
            login_host("https://GitHub.com/login").unwrap(),
            "github.com"
        );
        assert_eq!(
            login_host("http://localhost:3000").unwrap(),
            "localhost:3000"
        );
    }

    #[test]
    fn the_browser_tool_is_pointed_at_keymule_chrome() {
        let ws = "ws://127.0.0.1:9/devtools/browser/x";
        let pw = ["npx".to_string(), "@playwright/mcp@latest".into()];
        assert_eq!(endpoint_args(&pw, ws).unwrap(), ["--cdp-endpoint", ws]);
        let cd = [
            "npx".to_string(),
            "-y".into(),
            "chrome-devtools-mcp@latest".into(),
        ];
        assert_eq!(
            endpoint_args(&cd, ws).unwrap(),
            [format!("--wsEndpoint={ws}")]
        );
        assert!(endpoint_args(&["some-other-mcp".to_string()], ws).is_err());
    }

    #[test]
    fn stored_passwords_are_hidden_from_replies() {
        let mut h = Hidden {
            forms: vec![
                "hunter2-secret".into(),
                "hunter2-secret".replace('-', "%2D"),
            ],
            stamp: Hidden::files_stamp(),
        };
        let line = r#"{"result":{"content":[{"text":"value: hunter2-secret"}]}}"#;
        let out = h.scrub(line);
        assert!(!out.contains("hunter2-secret"));
        assert!(out.contains(HIDDEN));
    }
}
