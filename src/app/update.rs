//! Whether a newer keymule has been released, and whether the copy that's
//! running is older than the one installed.
//!
//! **A notice, not an updater.** keymule never replaces itself. A vault that
//! updates itself turns one bad release into everyone's problem, and on macOS
//! a swapped binary also loses the Keychain's "Always Allow". The problem
//! people actually have is not knowing, so this says so, with the command that
//! upgrades, and leaves the upgrade to them.
//!
//! **Once a day, and failure is silence.** The answer is cached for a day, so
//! the page and the menu bar never ask GitHub more than that. An offline
//! machine hasn't learned it's out of date, so a failed check says nothing.
//! This is the only network request keymule makes on its own;
//! `KEYMULE_NO_UPDATE_CHECK=1` turns it off.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const RELEASES_API: &str = "https://api.github.com/repos/Rorogogogo/keymule/releases/latest";
const RELEASES_PAGE: &str = "https://github.com/Rorogogogo/keymule/releases/latest";
const INSTALLER: &str =
    "curl -fsSL https://raw.githubusercontent.com/Rorogogogo/keymule/main/install.sh | sh";
const TTL_SECS: u64 = 24 * 60 * 60;

pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Serialize, Deserialize, Default)]
struct Cache {
    /// The newest release GitHub reported, without the "v".
    #[serde(default)]
    latest: Option<String>,
    /// When we last asked (whether or not it worked).
    #[serde(default)]
    checked: u64,
}

fn cache_path() -> Option<PathBuf> {
    crate::store::config_dir()
        .ok()
        .map(|d| d.join("update-check.json"))
}

fn read_cache() -> Cache {
    cache_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn write_cache(c: &Cache) {
    if let (Some(p), Ok(text)) = (cache_path(), serde_json::to_string(c)) {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, text);
    }
}

pub fn disabled() -> bool {
    std::env::var("KEYMULE_NO_UPDATE_CHECK").is_ok_and(|v| !v.is_empty() && v != "0")
}

/// `latest` is strictly newer than `current` (numeric parts compared, so
/// 0.10 is newer than 0.9; anything unparseable never counts as newer).
pub fn is_newer(latest: &str, current: &str) -> bool {
    fn parts(v: &str) -> Option<Vec<u64>> {
        let v = v.trim().trim_start_matches('v');
        let core = v.split(['-', '+']).next()?;
        core.split('.').map(|p| p.parse().ok()).collect()
    }
    let (Some(mut l), Some(mut c)) = (parts(latest), parts(current)) else {
        return false;
    };
    let n = l.len().max(c.len());
    l.resize(n, 0);
    c.resize(n, 0);
    l > c
}

/// How this copy was installed decides how to upgrade it.
pub fn upgrade_command(exe: &Path) -> &'static str {
    let p = exe.to_string_lossy();
    let brew = [
        "/Cellar/",
        "/opt/homebrew/",
        "/home/linuxbrew/",
        "/.linuxbrew/",
    ];
    if brew.iter().any(|b| p.contains(b)) {
        "brew upgrade keymule"
    } else {
        INSTALLER
    }
}

/// Ask GitHub for the latest release, at most once a day. Blocks for up to
/// ten seconds; call it off the main thread.
pub fn refresh() {
    if disabled() {
        return;
    }
    let mut cache = read_cache();
    let now = crate::trust::now();
    if now.saturating_sub(cache.checked) < TTL_SECS {
        return;
    }
    if let Some(v) = fetch_latest() {
        cache.latest = Some(v);
    }
    cache.checked = now;
    write_cache(&cache);
}

fn fetch_latest() -> Option<String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    rt.block_on(async {
        let r = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent(format!("keymule/{CURRENT}"))
            .build()
            .ok()?
            .get(RELEASES_API)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .ok()?;
        if !r.status().is_success() {
            return None;
        }
        let v: Value = r.json().await.ok()?;
        let tag = v["tag_name"].as_str()?.trim_start_matches('v').to_string();
        (!tag.is_empty()).then_some(tag)
    })
}

/// Keep checking in the background for as long as this process runs.
pub fn watch() {
    std::thread::spawn(|| {
        loop {
            refresh();
            std::thread::sleep(Duration::from_secs(6 * 60 * 60));
        }
    });
}

/// The newer release, if one is known (from the cache; never asks GitHub).
pub fn available() -> Option<String> {
    read_cache().latest.filter(|l| is_newer(l, CURRENT))
}

// ---- the running copy vs the installed one ------------------------------------

#[derive(PartialEq, Eq, Clone, Copy)]
struct Stamp {
    dev: u64,
    ino: u64,
    modified: Option<SystemTime>,
}

fn stamp() -> Option<Stamp> {
    use std::os::unix::fs::MetadataExt;
    let exe = std::env::current_exe().ok()?;
    let m = std::fs::metadata(exe).ok()?;
    Some(Stamp {
        dev: m.dev(),
        ino: m.ino(),
        modified: m.modified().ok(),
    })
}

static STARTED_AS: OnceLock<Option<Stamp>> = OnceLock::new();

/// Note which binary this process started from. Call it early.
pub fn remember_binary() {
    STARTED_AS.get_or_init(stamp);
}

/// The keymule binary on disk changed since this process started (an
/// upgrade or a rebuild), so this process is running an old copy.
pub fn replaced() -> bool {
    match STARTED_AS.get() {
        Some(Some(then)) => stamp() != Some(*then),
        _ => false,
    }
}

static IN_MENUBAR: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// This process is the menu bar app (so restarting means restarting it).
pub fn set_in_menubar() {
    IN_MENUBAR.store(true, std::sync::atomic::Ordering::Relaxed);
}

pub fn in_menubar() -> bool {
    IN_MENUBAR.load(std::sync::atomic::Ordering::Relaxed)
}

/// What the page shows.
pub fn status() -> Value {
    let exe = std::env::current_exe().unwrap_or_default();
    json!({
        "current": CURRENT,
        "latest": available(),
        "command": upgrade_command(&exe),
        "notes": RELEASES_PAGE,
        "replaced": replaced(),
        "disabled": disabled(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number_not_text() {
        assert!(is_newer("0.19.0", "0.18.0"));
        assert!(is_newer("v0.10.0", "0.9.3"), "0.10 is newer than 0.9");
        assert!(is_newer("1.0", "0.99.9"));
        assert!(is_newer("0.18.1", "0.18"), "a missing patch is 0");
        assert!(!is_newer("0.18.0", "0.18.0"));
        assert!(!is_newer("0.17.2", "0.18.0"));
        assert!(
            !is_newer("nightly", "0.18.0"),
            "unparseable never claims newer"
        );
        assert!(
            is_newer("0.19.0-rc.1", "0.18.0"),
            "suffixes compare on their numbers"
        );
    }

    #[test]
    fn the_upgrade_command_follows_how_it_was_installed() {
        let brew = Path::new("/opt/homebrew/Cellar/keymule/0.18.0/bin/keymule");
        assert_eq!(upgrade_command(brew), "brew upgrade keymule");
        let linuxbrew = Path::new("/home/linuxbrew/.linuxbrew/bin/keymule");
        assert_eq!(upgrade_command(linuxbrew), "brew upgrade keymule");
        let installer = Path::new("/Users/me/.cargo/bin/keymule");
        assert_eq!(upgrade_command(installer), INSTALLER);
    }

    #[test]
    fn nothing_is_replaced_before_it_is_remembered_or_while_unchanged() {
        // Without remember_binary() nothing is known, so nothing is claimed.
        assert!(STARTED_AS.get().is_none() || !replaced());
        remember_binary();
        assert!(!replaced(), "the test binary hasn't changed under us");
    }
}
