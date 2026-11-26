//! .env files and shell history.

use super::*;

pub(super) const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "dist",
    "build",
    ".venv",
    "venv",
    "Library",
    ".cache",
    ".npm",
    ".cargo",
    ".rustup",
    "Pictures",
    "Music",
    "Movies",
    ".Trash",
];

pub(super) fn is_env_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let env = lower == ".env" || lower.starts_with(".env.") || lower.ends_with(".env");
    env && !["example", "sample", "template", "dist", "defaults"]
        .iter()
        .any(|s| lower.contains(s))
}

pub(super) fn scan_env_files(opts: &Options, collector: &Mutex<Collector>, stats: &mut Stats) {
    let mut files = Vec::new();
    let in_home = opts.dir == opts.home;
    walk(&opts.dir, if in_home { 1 } else { 4 }, &mut |p| {
        if p.file_name()
            .is_some_and(|n| is_env_file(&n.to_string_lossy()))
        {
            files.push(p.to_path_buf());
        }
    });
    if !in_home {
        walk(&opts.home, 1, &mut |p| {
            if p.file_name()
                .is_some_and(|n| is_env_file(&n.to_string_lossy()))
            {
                files.push(p.to_path_buf());
            }
        });
    }
    files.sort();
    files.dedup();
    stats.env_files = files.len();
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let place = display(&opts.home, &path);
        let mut c = collector.lock().unwrap();
        for line in text.lines() {
            let line = line.trim().trim_start_matches("export ");
            let mut hits = detect::scan_deep(line);
            if let Some((key, value)) = line.split_once('=') {
                let value = value.trim().trim_matches(['"', '\'']);
                if is_secret_name(key.trim())
                    && detect::plausible_value(value)
                    && !hits.iter().any(|h| h.value == value)
                {
                    hits.push(Found {
                        value: value.into(),
                        service: None,
                        label: Some(key.trim().into()),
                        confident: true,
                    });
                }
            }
            for h in hits {
                c.add(
                    h,
                    Location {
                        source: place.clone(),
                        severity: Severity::Readable,
                        place: place.clone(),
                        date: None,
                    },
                );
            }
        }
    }
}

pub(super) fn scan_histories(home: &Path, collector: &Mutex<Collector>, stats: &mut Stats) {
    for name in [
        ".zsh_history",
        ".bash_history",
        ".local/share/fish/fish_history",
    ] {
        let path = home.join(name);
        if !path.is_file() {
            continue;
        }
        stats.histories += 1;
        let place = display(home, &path);
        let mut c = collector.lock().unwrap();
        for h in scan_lines(&path) {
            c.add(
                h,
                Location {
                    source: place.clone(),
                    severity: Severity::Readable,
                    place: place.clone(),
                    date: None,
                },
            );
        }
    }
}
