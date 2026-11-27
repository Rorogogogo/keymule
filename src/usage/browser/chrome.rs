//! Finding Chrome, and starting keymule's own (or finding the one already running).

use std::path::Path;

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

pub(super) fn profile_dir() -> Result<PathBuf> {
    Ok(browser_dir()?.join("profile"))
}

/// How to start keymule's Chrome when it isn't running.
pub(super) struct Start<'a> {
    /// The first tab's address.
    pub(super) url: &'a str,
    /// A window of its own that outlives this keymule (`keymule browser
    /// --open`), rather than one that goes with the browser tool.
    pub(super) window: bool,
}

/// keymule's Chrome: the one already running, or a new one. The child is
/// Some when this call started it.
pub(super) async fn ensure_chrome(
    start: Start<'_>,
) -> Result<(State, Option<std::process::Child>)> {
    if let Some(s) = running().await {
        return Ok((s, None));
    }
    let profile = profile_dir()?;
    std::fs::create_dir_all(&profile)?;
    prepare_profile(&profile);
    let port_file = profile.join("DevToolsActivePort");
    let _ = std::fs::remove_file(&port_file);
    let chrome = chrome_path()?;
    let mut cmd = std::process::Command::new(&chrome);
    cmd.arg(format!("--user-data-dir={}", profile.display()))
        .arg("--remote-debugging-port=0")
        .arg("--remote-debugging-address=127.0.0.1")
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg(start.url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let no_screen = cfg!(target_os = "linux")
        && std::env::var_os("DISPLAY").is_none()
        && std::env::var_os("WAYLAND_DISPLAY").is_none();
    if start.window {
        if no_screen {
            bail!("there's no screen here to open a browser window on");
        }
        // Its own process group, so ending this command (or Ctrl-C in its
        // terminal) leaves the window open.
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    }
    if (no_screen && !start.window)
        || std::env::var_os("KEYMULE_BROWSER_HEADLESS").is_some_and(|v| v == "1")
    {
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

// ---- the profile ------------------------------------------------------------

/// Before Chrome starts on keymule's profile: let the Claude in Chrome
/// extension reach Claude Code from it, and keep Chrome from offering to save
/// the passwords sign_in types (the profile would keep a copy).
pub(super) fn prepare_profile(profile: &Path) {
    copy_native_hosts(&native_host_dirs(), profile);
    no_saved_passwords(profile);
}

/// Where Chrome-family browsers keep the user's native messaging hosts: the
/// small manifests that tell an extension which program to talk to.
fn native_host_dirs() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return vec![];
    };
    let browsers: &[&str] = if cfg!(target_os = "macos") {
        &[
            "Library/Application Support/Google/Chrome",
            "Library/Application Support/Chromium",
            "Library/Application Support/Microsoft Edge",
            "Library/Application Support/BraveSoftware/Brave-Browser",
        ]
    } else {
        &[
            ".config/google-chrome",
            ".config/chromium",
            ".config/microsoft-edge",
            ".config/BraveSoftware/Brave-Browser",
        ]
    };
    browsers
        .iter()
        .map(|b| home.join(b).join("NativeMessagingHosts"))
        .collect()
}

/// Chrome looks for the user's native messaging hosts inside the profile it
/// runs on, so keymule's own profile can't see the one Claude Code installed.
/// Copy Anthropic's (Claude in Chrome's) manifests in, and no others; on
/// every start, so a reinstalled Claude Code's new path is picked up.
/// (On Windows the hosts are in the registry, which every profile sees.)
pub(super) fn copy_native_hosts(from: &[PathBuf], profile: &Path) {
    let to = profile.join("NativeMessagingHosts");
    let mut done = std::collections::HashSet::new();
    for dir in from {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !(name.starts_with("com.anthropic.") && name.ends_with(".json"))
                || !done.insert(name.clone())
            {
                continue;
            }
            let Ok(text) = std::fs::read(e.path()) else {
                continue;
            };
            let dest = to.join(&name);
            if std::fs::read(&dest).ok().as_ref() != Some(&text) {
                let _ = std::fs::create_dir_all(&to);
                let _ = std::fs::write(&dest, &text);
            }
        }
    }
}

/// Turn off Chrome's "Save password?" in keymule's profile.
pub(super) fn no_saved_passwords(profile: &Path) {
    let path = profile.join("Default").join("Preferences");
    let mut prefs: Value = match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(v @ Value::Object(_)) => v,
            _ => return, // not ours to fix
        },
        Err(_) => json!({}),
    };
    let before = prefs.clone();
    prefs["credentials_enable_service"] = false.into();
    if !prefs["profile"].is_object() {
        prefs["profile"] = json!({});
    }
    prefs["profile"]["password_manager_enabled"] = false.into();
    if prefs != before {
        let _ = std::fs::create_dir_all(path.parent().expect("has a parent"));
        let _ = std::fs::write(&path, prefs.to_string());
    }
}
