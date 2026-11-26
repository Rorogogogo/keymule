//! `keymule doctor`: find secrets that already leaked to AI models (agent
//! transcripts), sit in plain text in agent configs, or are readable by
//! agents (.env files, shell history). Secrets are only ever shown masked.

mod configs;
mod files;
mod helpers;
mod render;
#[cfg(test)]
mod tests;
mod transcripts;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant, UNIX_EPOCH};

use regex::Regex;
use serde::Serialize;
use serde_json::Value;

use crate::detect::{self, Found};
use crate::store::{self, Store};

pub use configs::*;
use files::*;
pub use helpers::*;
pub use render::*;
use transcripts::*;

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

/// How far a scan has got, for a progress display. Never holds a secret:
/// only counts, file places and what the scan is doing.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Progress {
    /// transcripts, configs, env, history, stored or done.
    pub phase: &'static str,
    /// A line for the log, if there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub msg: Option<String>,
    /// Bytes of transcripts read so far, and in all.
    pub done: u64,
    pub total: u64,
    /// Distinct secrets found so far.
    pub found: usize,
}

pub fn run(opts: &Options) -> Report {
    run_with(opts, &|_| {})
}

/// `run`, telling `progress` what it is doing as it goes.
pub fn run_with(opts: &Options, progress: &(dyn Fn(Progress) + Sync)) -> Report {
    let mut stats = Stats::default();
    let collector = Mutex::new(Collector::default());
    let found = |c: &Mutex<Collector>| c.lock().map(|c| c.by_secret.len()).unwrap_or(0);
    let say = |phase, msg: String, stats: &Stats| {
        progress(Progress {
            phase,
            msg: Some(msg),
            done: stats.transcript_bytes,
            total: stats.transcript_bytes,
            found: found(&collector),
        })
    };

    scan_transcripts(&opts.home, &collector, &mut stats, progress);
    say("configs", "Checking agent configs".into(), &stats);
    scan_configs(opts, &collector, &mut stats);
    for c in &stats.configs {
        say("configs", format!("Checked {c}"), &stats);
    }
    say("env", "Looking for .env files".into(), &stats);
    scan_env_files(opts, &collector, &mut stats);
    say(
        "env",
        format!(
            "Checked {} .env {}",
            stats.env_files,
            plural(stats.env_files, "file", "files")
        ),
        &stats,
    );
    scan_histories(&opts.home, &collector, &mut stats);
    say(
        "history",
        format!(
            "Checked {} shell {}",
            stats.histories,
            plural(stats.histories, "history", "histories")
        ),
        &stats,
    );

    let stored = if opts.check_stored {
        say(
            "stored",
            "Comparing what was found with the keys in keymule".into(),
            &stats,
        );
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
    let n = entries.len();
    progress(Progress {
        phase: "done",
        msg: Some(format!(
            "Done: {n} possible {} found",
            plural(n, "key", "keys")
        )),
        done: stats.transcript_bytes,
        total: stats.transcript_bytes,
        found: n,
    });
    Report { stats, entries }
}

fn plural<'a>(n: usize, one: &'a str, many: &'a str) -> &'a str {
    if n == 1 { one } else { many }
}

fn size_text(bytes: u64) -> String {
    let mb = bytes as f64 / 1_048_576.0;
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else {
        format!("{mb:.0} MB")
    }
}
