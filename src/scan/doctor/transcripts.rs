//! Agent transcripts: the bulk of the scan, read in parallel chunks.

use super::*;

/// Cheap filter so the full detector only runs on lines that could match.
pub(super) static PREFILTER: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(
        r"(?i)gh[pousr]_|github_pat_|glpat-|sk-|sk_live|sk_test|rk_live|xox[abprs]-|AKIA|AIza|ntn_|npm_|token|secret|passw|api_?key|access_?key|private_?key|bearer|://[^/\s:@]+:[^@\s/]+@",
    )
    .unwrap()
});

/// Big transcripts are split into chunks of this size so all cores help.
pub(super) const CHUNK: u64 = 8 << 20;

pub(super) fn scan_transcripts(
    home: &Path,
    collector: &Mutex<Collector>,
    stats: &mut Stats,
    progress: &(dyn Fn(Progress) + Sync),
) {
    progress(Progress {
        phase: "transcripts",
        msg: Some("Looking for agent transcripts".into()),
        ..Progress::default()
    });
    let mut files: Vec<(PathBuf, &'static str, u64)> = Vec::new();
    for (dir, agent) in [
        (home.join(".claude").join("projects"), "Claude Code"),
        (home.join(".codex").join("sessions"), "Codex"),
    ] {
        walk(&dir, 6, &mut |p| {
            if p.extension().is_some_and(|e| e == "jsonl") {
                let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                files.push((p.to_path_buf(), agent, size));
            }
        });
    }
    stats.transcripts = files.len();
    stats.transcript_bytes = files.iter().map(|f| f.2).sum();
    let total = stats.transcript_bytes;
    progress(Progress {
        phase: "transcripts",
        msg: Some(format!(
            "Reading {} {} ({})",
            files.len(),
            plural(files.len(), "transcript", "transcripts"),
            size_text(total)
        )),
        total,
        ..Progress::default()
    });

    // (file index, start, end), biggest pieces first.
    let mut work: Vec<(usize, u64, u64)> = Vec::new();
    for (i, (_, _, size)) in files.iter().enumerate() {
        let mut start = 0;
        while start < *size {
            work.push((i, start, (start + CHUNK).min(*size)));
            start += CHUNK;
        }
    }
    work.sort_by_key(|(_, s, e)| std::cmp::Reverse(e - s));
    let meta: Vec<(String, String, Option<String>)> = files
        .iter()
        .map(|(path, agent, _)| {
            (
                path.to_string_lossy().to_string(),
                transcript_place(home, path, agent),
                modified_date(path),
            )
        })
        .collect();

    let next = AtomicUsize::new(0);
    let read = AtomicU64::new(0);
    // A line every so often, not one per piece: there can be thousands.
    let last = Mutex::new(Instant::now());
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(12);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&(file, start, end)) = work.get(i) else {
                        break;
                    };
                    let hits = scan_range(&files[file].0, start, end);
                    let done = read.fetch_add(end - start, Ordering::Relaxed) + (end - start);
                    if let Ok(mut t) = last.try_lock()
                        && t.elapsed() >= Duration::from_millis(150)
                    {
                        *t = Instant::now();
                        drop(t);
                        progress(Progress {
                            phase: "transcripts",
                            msg: Some(format!("Reading {}", meta[file].1)),
                            done,
                            total,
                            found: collector.lock().map(|c| c.by_secret.len()).unwrap_or(0),
                        });
                    }
                    if hits.is_empty() {
                        continue;
                    }
                    let (source, place, date) = &meta[file];
                    let mut c = collector.lock().unwrap();
                    for found in hits {
                        c.add(
                            found,
                            Location {
                                source: source.clone(),
                                severity: Severity::Leaked,
                                place: place.clone(),
                                date: date.clone(),
                            },
                        );
                    }
                }
            });
        }
    });
}

/// Every distinct secret on lines that *start* in `[start, end)` of a file.
pub(super) fn scan_range(path: &Path, start: u64, end: u64) -> Vec<Found> {
    use std::io::{Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut pos = start;
    let mut buf = Vec::new();
    if start > 0 {
        // Skip the rest of the line that began in the previous chunk.
        if file.seek(SeekFrom::Start(start - 1)).is_err() {
            return Vec::new();
        }
        pos = start - 1;
    }
    let mut reader = BufReader::with_capacity(1 << 20, file);
    if start > 0 {
        match reader.read_until(b'\n', &mut buf) {
            Ok(n) => pos += n as u64,
            Err(_) => return Vec::new(),
        }
    }
    let mut out: Vec<Found> = Vec::new();
    while pos < end {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => pos += n as u64,
        }
        let line = String::from_utf8_lossy(&buf);
        if !PREFILTER.is_match(&line) {
            continue;
        }
        // Transcripts are JSON; un-escape quotes so `\"KEY\": \"v\"` reads as `"KEY": "v"`.
        let line = line.replace("\\\"", "\"").replace("\\n", "\n");
        for f in detect::scan_deep(&line) {
            if let Some(existing) = out.iter_mut().find(|e| e.value == f.value) {
                existing.confident |= f.confident;
            } else {
                out.push(f);
            }
        }
    }
    out
}

pub(super) fn scan_lines(path: &Path) -> Vec<Found> {
    scan_range(path, 0, u64::MAX)
}

pub(super) fn transcript_place(home: &Path, path: &Path, agent: &str) -> String {
    if agent == "Claude Code" {
        // ~/.claude/projects/-Users-roro-Developer-x/<session>.jsonl
        let project = path
            .strip_prefix(home.join(".claude").join("projects"))
            .ok()
            .and_then(|p| p.components().next())
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .unwrap_or_default();
        let home_prefix = home.to_string_lossy().replace('/', "-");
        let short = project.strip_prefix(&home_prefix).unwrap_or(&project);
        let short = short.trim_start_matches('-').replace('-', "/");
        format!("Claude Code session in ~/{short}")
    } else {
        // Codex records the working directory in the first line.
        static CWD: std::sync::LazyLock<Regex> =
            std::sync::LazyLock::new(|| Regex::new(r#""cwd"\s*:\s*"([^"]+)""#).unwrap());
        let mut first = Vec::new();
        if let Ok(f) = std::fs::File::open(path) {
            let _ = BufReader::new(f)
                .take(256 * 1024)
                .read_until(b'\n', &mut first);
        }
        match CWD.captures(&String::from_utf8_lossy(&first)) {
            Some(c) => format!("{agent} session in {}", display(home, Path::new(&c[1]))),
            None => format!("{agent} session"),
        }
    }
}
