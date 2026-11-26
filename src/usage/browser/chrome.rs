//! Finding Chrome, and starting keymule's own (or finding the one already running).

use super::*;

pub(super) fn chrome_path() -> Result<PathBuf> {
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
            candidates.push(std::path::Path::new("/Applications").join(app));
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
pub(super) async fn alive(state: &State) -> bool {
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

pub(super) async fn running() -> Option<State> {
    let text = std::fs::read_to_string(state_path().ok()?).ok()?;
    let state: State = serde_json::from_str(&text).ok()?;
    alive(&state).await.then_some(state)
}

/// keymule's Chrome: the one already running, or a new one. The bool says
/// whether this call started it.
pub(super) async fn ensure_chrome() -> Result<(State, Option<std::process::Child>)> {
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
