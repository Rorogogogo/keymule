//! Shared by the scanners: walking folders, stored secrets, small text helpers.

use super::*;

pub(super) fn stored_secrets() -> HashMap<String, String> {
    let Ok(store) = Store::load() else {
        return HashMap::new();
    };
    let vault = store::vault();
    let mut out: HashMap<String, String> = store
        .creds
        .keys()
        .filter_map(|name| {
            store::Sealed::peek_secret(vault.as_ref(), name).map(|s| (s, name.clone()))
        })
        .collect();
    for name in store.servers.keys() {
        if let Ok(b) = crate::servers::Binding::load(vault.as_ref(), name) {
            for s in b.secrets() {
                out.insert(s.clone(), name.clone());
            }
        }
    }
    out
}

pub(super) fn walk(dir: &Path, depth: usize, f: &mut dyn FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if depth > 1 && !SKIP_DIRS.contains(&name.as_str()) {
                walk(&path, depth - 1, f);
            }
        } else if kind.is_file() {
            f(&path);
        }
    }
}

pub(super) fn display(home: &Path, path: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rel) => format!("~/{}", rel.display()),
        Err(_) => path.display().to_string(),
    }
}

pub(super) fn tilde(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(h) if path.starts_with(&h) => format!("~{}", &path[h.len()..]),
        _ => path.to_string(),
    }
}

pub(super) fn modified_date(path: &Path) -> Option<String> {
    let secs = std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs();
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

/// Days since 1970-01-01 → (year, month, day). Howard Hinnant's algorithm.
pub(super) fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = yoe as i64 + era * 400;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `ghp_…9f2` — enough to recognise, not enough to use.
pub fn mask(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    match chars.len() {
        n if n >= 16 => {
            let head: String = chars[..4].iter().collect();
            let tail: String = chars[n - 3..].iter().collect();
            format!("{head}…{tail}")
        }
        n if n >= 8 => format!("{}…", chars[..2].iter().collect::<String>()),
        _ => "…".into(),
    }
}
