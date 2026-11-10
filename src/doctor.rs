//! `keymule doctor`: find secrets that already leaked to AI models (agent
//! transcripts), sit in plain text in agent configs, or are readable by
//! agents (.env files, shell history). Secrets are only ever shown masked.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::UNIX_EPOCH;

use regex::Regex;
use serde::Serialize;
use serde_json::Value;

use crate::detect::{self, Found};
use crate::store::{self, Store};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// In an agent transcript: it was sent to a model provider.
    Leaked,
    /// In plain text in an agent config file.
    Plaintext,
    /// In a .env file or shell history an agent can read.
    Readable,
}

#[derive(Debug, Clone, Serialize)]
pub struct Location {
    /// File it came from; one location per file, however often it appears.
    #[serde(skip)]
    pub source: String,
    pub severity: Severity,
    pub place: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Entry {
    pub masked: String,
    pub label: Option<String>,
    pub service: Option<&'static str>,
    pub confident: bool,
    pub stored_as: Option<String>,
    pub locations: Vec<Location>,
}

impl Entry {
    pub fn severity(&self) -> Severity {
        self.locations
            .iter()
            .map(|l| l.severity)
            .min()
            .unwrap_or(Severity::Readable)
    }
}

#[derive(Debug, Default, Serialize)]
pub struct Stats {
    pub transcripts: usize,
    pub transcript_bytes: u64,
    pub configs: Vec<String>,
    pub env_files: usize,
    pub histories: usize,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub stats: Stats,
    pub entries: Vec<Entry>,
}

pub struct Options {
    pub home: PathBuf,
    /// Project directory to look for .env and .mcp.json files in.
    pub dir: PathBuf,
    /// Note which findings are already stored in keymule (reads the keychain).
    pub check_stored: bool,
}

#[derive(Default)]
struct Collector {
    by_secret: HashMap<String, (Found, Vec<Location>)>,
}

impl Collector {
    fn add(&mut self, found: Found, loc: Location) {
        let entry = self
            .by_secret
            .entry(found.value.clone())
            .or_insert_with(|| (found.clone(), Vec::new()));
        entry.0.confident |= found.confident;
        if entry.0.label.is_none() {
            entry.0.label = found.label;
        }
        if entry.0.service.is_none() {
            entry.0.service = found.service;
        }
        if !entry
            .1
            .iter()
            .any(|l| l.source == loc.source && l.severity == loc.severity)
        {
            entry.1.push(loc);
        }
    }
}

pub fn run(opts: &Options) -> Report {
    let mut stats = Stats::default();
    let collector = Mutex::new(Collector::default());

    scan_transcripts(&opts.home, &collector, &mut stats);
    scan_configs(opts, &collector, &mut stats);
    scan_env_files(opts, &collector, &mut stats);
    scan_histories(&opts.home, &collector, &mut stats);

    let stored = if opts.check_stored {
        stored_secrets()
    } else {
        HashMap::new()
    };
    let mut entries: Vec<Entry> = collector
        .into_inner()
        .unwrap()
        .by_secret
        .into_iter()
        .map(|(secret, (found, mut locations))| {
            locations.sort_by(|a, b| a.severity.cmp(&b.severity).then(b.date.cmp(&a.date)));
            Entry {
                masked: mask(&secret),
                label: found.label,
                service: found.service,
                confident: found.confident,
                stored_as: stored.get(&secret).cloned(),
                locations,
            }
        })
        .collect();
    entries.sort_by(|a, b| {
        a.severity()
            .cmp(&b.severity())
            .then(b.confident.cmp(&a.confident))
            .then(b.locations.len().cmp(&a.locations.len()))
            .then(a.masked.cmp(&b.masked))
    });
    Report { stats, entries }
}

// ---- transcripts -----------------------------------------------------------

/// Cheap filter so the full detector only runs on lines that could match.
static PREFILTER: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(
        r"(?i)gh[pousr]_|github_pat_|glpat-|sk-|sk_live|sk_test|rk_live|xox[abprs]-|AKIA|AIza|ntn_|npm_|token|secret|passw|api_?key|access_?key|private_?key|bearer|://[^/\s:@]+:[^@\s/]+@",
    )
    .unwrap()
});

/// Big transcripts are split into chunks of this size so all cores help.
const CHUNK: u64 = 8 << 20;

fn scan_transcripts(home: &Path, collector: &Mutex<Collector>, stats: &mut Stats) {
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
fn scan_range(path: &Path, start: u64, end: u64) -> Vec<Found> {
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

fn scan_lines(path: &Path) -> Vec<Found> {
    scan_range(path, 0, u64::MAX)
}

fn transcript_place(home: &Path, path: &Path, agent: &str) -> String {
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

// ---- configs ---------------------------------------------------------------

fn scan_configs(opts: &Options, collector: &Mutex<Collector>, stats: &mut Stats) {
    let home = &opts.home;
    let json_files = [
        home.join(".claude.json"),
        home.join(".claude").join("settings.json"),
        home.join(".cursor").join("mcp.json"),
        home.join("Library/Application Support/Claude/claude_desktop_config.json"),
        opts.dir.join(".mcp.json"),
        opts.dir.join(".cursor").join("mcp.json"),
        opts.dir.join(".claude").join("settings.json"),
        opts.dir.join(".claude").join("settings.local.json"),
    ];
    let mut seen = Vec::new();
    for path in json_files {
        let Ok(canon) = path.canonicalize() else {
            continue;
        };
        if seen.contains(&canon) {
            continue;
        }
        seen.push(canon);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let name = display(home, &path);
        scan_value(&value, &mut Vec::new(), &name, collector);
        stats.configs.push(name);
    }
    let codex = home.join(".codex").join("config.toml");
    if let Ok(text) = std::fs::read_to_string(&codex)
        && let Ok(value) = toml::from_str::<toml::Value>(&text)
        && let Ok(json) = serde_json::to_value(value)
    {
        let name = display(home, &codex);
        scan_value(&json, &mut Vec::new(), &name, collector);
        stats.configs.push(name);
    }
}

pub fn is_secret_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    [
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "API_KEY",
        "APIKEY",
        "ACCESS_KEY",
        "PRIVATE_KEY",
        "AUTHORIZATION",
        "CREDENTIAL",
    ]
    .iter()
    .any(|k| upper.contains(k))
        && !upper.contains("AUTHOR_")
        // maxTokens, tokenCount, FirstTokenDate, GITHUB_API_URL: settings, not secrets.
        && ![
            "DATE", "COUNT", "LIMIT", "USAGE", "TIME", "MAX", "SIZE", "URL", "PATH", "FILE",
            "ENDPOINT",
        ]
        .iter()
        .any(|k| upper.contains(k))
}

/// Walk a parsed config; report secret-named values and anything the
/// detector recognises in other strings.
fn scan_value(value: &Value, path: &mut Vec<String>, file: &str, collector: &Mutex<Collector>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                path.push(k.clone());
                scan_value(v, path, file, collector);
                path.pop();
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                path.push(i.to_string());
                scan_value(v, path, file, collector);
                path.pop();
            }
        }
        Value::String(s) => {
            let key = path.last().map(String::as_str).unwrap_or("");
            let mut hits = detect::scan_deep(s);
            let value = s.trim().trim_start_matches("Bearer ").trim();
            if is_secret_name(key)
                && detect::plausible_value(value)
                && !hits.iter().any(|h| h.value == value)
            {
                hits.push(Found {
                    value: value.to_string(),
                    service: None,
                    label: Some(key.to_string()),
                    confident: true,
                });
            }
            if hits.is_empty() {
                return;
            }
            let place = format!("{file} · {}", config_path(path));
            let mut c = collector.lock().unwrap();
            for mut h in hits {
                if h.label.is_none() {
                    h.label = Some(key.to_string());
                }
                c.add(
                    h,
                    Location {
                        source: place.clone(),
                        severity: Severity::Plaintext,
                        place: place.clone(),
                        date: None,
                    },
                );
            }
        }
        _ => {}
    }
}

/// `projects./Users/x/app.mcpServers.github.env.TOKEN` → `project ~/app: github.env.TOKEN`
fn config_path(path: &[String]) -> String {
    let mut parts: Vec<&str> = path.iter().map(String::as_str).collect();
    let mut prefix = String::new();
    if parts.first() == Some(&"projects") && parts.len() > 2 {
        prefix = format!("project {}: ", tilde(parts[1]));
        parts.drain(..2);
    }
    if matches!(parts.first(), Some(&"mcpServers") | Some(&"mcp_servers")) && parts.len() > 1 {
        parts.remove(0);
    }
    format!("{prefix}{}", parts.join("."))
}

// ---- .env files & shell history -------------------------------------------

const SKIP_DIRS: &[&str] = &[
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

fn is_env_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let env = lower == ".env" || lower.starts_with(".env.") || lower.ends_with(".env");
    env && !["example", "sample", "template", "dist", "defaults"]
        .iter()
        .any(|s| lower.contains(s))
}

fn scan_env_files(opts: &Options, collector: &Mutex<Collector>, stats: &mut Stats) {
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

fn scan_histories(home: &Path, collector: &Mutex<Collector>, stats: &mut Stats) {
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

// ---- helpers ---------------------------------------------------------------

fn stored_secrets() -> HashMap<String, String> {
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

fn walk(dir: &Path, depth: usize, f: &mut dyn FnMut(&Path)) {
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

fn display(home: &Path, path: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rel) => format!("~/{}", rel.display()),
        Err(_) => path.display().to_string(),
    }
}

fn tilde(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(h) if path.starts_with(&h) => format!("~{}", &path[h.len()..]),
        _ => path.to_string(),
    }
}

fn modified_date(path: &Path) -> Option<String> {
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
fn civil_from_days(z: i64) -> (i64, u32, u32) {
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

// ---- rendering -------------------------------------------------------------

fn summarize_places(locs: &[&Location], limit: usize) -> Vec<String> {
    // Many sessions in the same project become one line with a count.
    let mut groups: Vec<(String, usize, Option<String>)> = Vec::new();
    for l in locs {
        if let Some(g) = groups.iter_mut().find(|g| g.0 == l.place) {
            g.1 += 1;
            if l.date > g.2 {
                g.2 = l.date.clone();
            }
        } else {
            groups.push((l.place.clone(), 1, l.date.clone()));
        }
    }
    groups.sort_by(|a, b| b.2.cmp(&a.2));
    let mut lines: Vec<String> = groups
        .iter()
        .take(limit)
        .map(|(place, n, date)| match (n, date) {
            (1, Some(d)) => format!("{place} ({d})"),
            (n, Some(d)) => format!("{place} ×{n} (latest {d})"),
            (_, None) => place.clone(),
        })
        .collect();
    if groups.len() > limit {
        lines.push(format!("…and {} more", groups.len() - limit));
    }
    lines
}

pub fn render(report: &Report, all: bool) -> String {
    let s = &report.stats;
    let mut out = format!(
        "keymule doctor: scanned {} agent transcripts ({} MB), {} agent configs, {} .env files, {} shell histories\n",
        s.transcripts,
        s.transcript_bytes / 1_000_000,
        s.configs.len(),
        s.env_files,
        s.histories
    );
    let sections = [
        (Severity::Leaked, "🔴 Sent to an AI model: rotate these"),
        (
            Severity::Plaintext,
            "🟠 Plain text in agent configs: move these into keymule",
        ),
        (
            Severity::Readable,
            "🟡 Readable by agents (.env files, shell history)",
        ),
    ];
    let mut hidden = 0;
    for (sev, title) in sections {
        let entries: Vec<&Entry> = report
            .entries
            .iter()
            .filter(|e| e.severity() == sev)
            .collect();
        let shown: Vec<&&Entry> = entries.iter().filter(|e| all || e.confident).collect();
        hidden += entries.len() - shown.len();
        if shown.is_empty() {
            continue;
        }
        out.push_str(&format!("\n{title} ({})\n", shown.len()));
        for e in shown {
            let what = e.label.clone().unwrap_or_else(|| "possible secret".into());
            let maybe = if e.confident { "" } else { "  (unsure)" };
            out.push_str(&format!("  {:<12} {what}{maybe}\n", e.masked));
            if let Some(name) = &e.stored_as {
                out.push_str(&format!("      stored in keymule as `{name}`\n"));
            }
            for sev2 in [Severity::Leaked, Severity::Plaintext, Severity::Readable] {
                let locs: Vec<&Location> =
                    e.locations.iter().filter(|l| l.severity == sev2).collect();
                if locs.is_empty() {
                    continue;
                }
                let verb = match sev2 {
                    Severity::Leaked => "seen in",
                    Severity::Plaintext => "plain text in",
                    Severity::Readable => "readable in",
                };
                for (i, line) in summarize_places(&locs, 3).iter().enumerate() {
                    let lead = if i == 0 { verb } else { "" };
                    out.push_str(&format!("      {lead:<13} {line}\n"));
                }
            }
        }
    }
    if report.entries.is_empty() {
        out.push_str("\n✓ No secrets found.\n");
    }
    if hidden > 0 {
        out.push_str(&format!(
            "\n⚪ {hidden} more possible secret{} without a known format (keymule doctor --all to list)\n",
            if hidden == 1 { "" } else { "s" }
        ));
    }
    if report
        .entries
        .iter()
        .any(|e| e.confident && e.severity() == Severity::Leaked)
    {
        out.push_str(
            "\nNext: revoke each 🔴 token at its service, create a new one, and store it with\n\
             `keymule add <service>` (or `keymule rotate <name>` if it is already stored).\n",
        );
    }
    if report.entries.iter().any(|e| {
        e.confident
            && e.locations
                .iter()
                .any(|l| l.severity == Severity::Plaintext)
    }) {
        out.push_str(
            "\nNext: `keymule adopt --dry-run` shows how to move MCP server secrets out of\n\
             agent configs and into the keychain.\n",
        );
    }
    out.push_str("\nNothing left your machine. Secrets are shown masked.\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_401), (2025, 11, 9));
    }

    #[test]
    fn chunked_scan_matches_whole_file_scan() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        let mut text = String::new();
        let tokens: Vec<String> = (0..40)
            .map(|i| format!("ghp_Q8mZ2vK7xR4tW9pN3bY6cJ1hL5dF0gS2a{i:03}"))
            .collect();
        for (i, t) in tokens.iter().enumerate() {
            text.push_str(&format!(
                "{{\"n\":{i},\"pad\":\"{}\",\"t\":\"{t}\"}}\n",
                "x".repeat(i * 7)
            ));
        }
        std::fs::write(&path, &text).unwrap();
        let size = text.len() as u64;
        for chunk in [1, 7, 64, 500, size] {
            let mut got: Vec<String> = Vec::new();
            let mut start = 0;
            while start < size {
                got.extend(
                    scan_range(&path, start, (start + chunk).min(size))
                        .into_iter()
                        .map(|f| f.value),
                );
                start += chunk;
            }
            got.sort();
            let mut want = tokens.clone();
            want.sort();
            assert_eq!(got, want, "chunk size {chunk}");
        }
    }

    #[test]
    fn date_and_counter_names_are_not_secrets() {
        assert!(!is_secret_name("claudeCodeFirstTokenDate"));
        assert!(!is_secret_name("maxTokens"));
        assert!(!is_secret_name("GITHUB_API_URL"));
        assert!(is_secret_name("GITHUB_PERSONAL_ACCESS_TOKEN"));
        assert!(!detect::plausible_value("2025-01-01T00:00:00Z"));
        assert!(!detect::plausible_value("YourSecretKeyForSecurity"));
    }

    #[test]
    fn masks() {
        assert_eq!(mask("ghp_1234567890abcdefXYZ"), "ghp_…XYZ");
        assert_eq!(mask("short123"), "sh…");
    }

    #[test]
    fn env_file_names() {
        assert!(
            is_env_file(".env") && is_env_file(".env.local") && is_env_file("jj-stripe-test.env")
        );
        assert!(!is_env_file(".env.example") && !is_env_file("environment.ts"));
    }

    #[test]
    fn config_paths_read_well() {
        let p: Vec<String> = ["projects", "/x/app", "mcpServers", "github", "env", "TOKEN"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(config_path(&p), "project /x/app: github.env.TOKEN");
    }

    #[test]
    fn end_to_end_on_a_fake_home() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        let leaked = "ghp_Q8mZ2vK7xR4tW9pN3bY6cJ1hL5dF0gS2aE7u";
        let plain = "Zq8vL2mK9xR4tW7pN3bY6cJ1";
        std::fs::create_dir_all(h.join(".claude/projects/-Users-me-app")).unwrap();
        std::fs::write(
            h.join(".claude/projects/-Users-me-app/s1.jsonl"),
            format!("{{\"message\":\"use {leaked} please\"}}\n"),
        )
        .unwrap();
        std::fs::write(
            h.join(".claude.json"),
            format!(r#"{{"mcpServers":{{"jj":{{"env":{{"JJ_API_KEY":"{plain}","URL":"https://x"}}}}}}}}"#),
        )
        .unwrap();
        std::fs::write(
            h.join(".zsh_history"),
            format!("curl -H 'Authorization: Bearer {plain}'\n"),
        )
        .unwrap();
        let report = run(&Options {
            home: h.to_path_buf(),
            dir: h.to_path_buf(),
            check_stored: false,
        });
        assert_eq!(report.entries.len(), 2, "{report:#?}");
        assert_eq!(report.entries[0].severity(), Severity::Leaked);
        assert_eq!(report.entries[0].label.as_deref(), Some("GitHub token"));
        assert_eq!(report.entries[1].severity(), Severity::Plaintext);
        assert_eq!(report.entries[1].locations.len(), 2, "config + history");
        let text = render(&report, false);
        assert!(
            !text.contains(leaked) && !text.contains(plain),
            "secrets must be masked:\n{text}"
        );
        assert!(text.contains("jj.env.JJ_API_KEY"), "{text}");
        assert!(
            text.contains("Claude Code session in ~/app")
                || text.contains("Claude Code session in"),
            "{text}"
        );
    }
}
