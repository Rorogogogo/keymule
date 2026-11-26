//! The leak check, run in the background while the page shows its progress.

use super::*;

#[derive(Default)]
pub(super) struct Scan {
    running: bool,
    /// Counts scans, so the page can tell a new one from the last.
    id: u64,
    started: Option<Instant>,
    finished: Option<Duration>,
    phase: String,
    /// The latest log lines, oldest first.
    log: Vec<String>,
    /// How many log lines there have been in all.
    lines: usize,
    done: u64,
    total: u64,
    found: u64,
    report: Value,
    error: Option<String>,
}

pub(super) static SCAN: LazyLock<Mutex<Scan>> = LazyLock::new(|| Mutex::new(Scan::default()));

pub(super) const SCAN_LOG: usize = 50;

pub(super) fn start_scan(ctx: &Ctx) {
    let mut s = SCAN.lock().unwrap();
    if s.running {
        return;
    }
    *s = Scan {
        running: true,
        id: s.id + 1,
        started: Some(Instant::now()),
        phase: "transcripts".into(),
        ..Scan::default()
    };
    drop(s);
    let exe = ctx.exe.clone();
    std::thread::spawn(move || {
        let (report, error) = match run_scan(&exe) {
            Ok(report) => (report, None),
            Err(e) => (Value::Null, Some(e.to_string())),
        };
        let mut s = SCAN.lock().unwrap();
        s.running = false;
        s.finished = s.started.map(|t| t.elapsed());
        s.report = report;
        s.error = error;
    });
}

pub(super) fn run_scan(exe: &std::path::Path) -> Result<Value> {
    let mut child = std::process::Command::new(exe)
        .args(["doctor", "--json", "--progress"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("starting keymule doctor")?;
    // Drain stdout alongside: the report can be bigger than a pipe holds.
    let mut stdout = child.stdout.take().context("no stdout")?;
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        let _ = stdout.read_to_string(&mut out);
        out
    });
    let mut other = Vec::new();
    for line in BufReader::new(child.stderr.take().context("no stderr")?).lines() {
        let Ok(line) = line else { break };
        let Ok(p) = serde_json::from_str::<Value>(&line) else {
            other.push(line);
            continue;
        };
        let mut s = SCAN.lock().unwrap();
        if let Some(phase) = p["phase"].as_str() {
            s.phase = phase.into();
        }
        s.done = p["done"].as_u64().unwrap_or(s.done);
        s.total = p["total"].as_u64().unwrap_or(s.total);
        s.found = p["found"].as_u64().unwrap_or(s.found);
        // Big transcripts are read in pieces: say each place once in a row.
        if let Some(msg) = p["msg"].as_str()
            && s.log.last().is_none_or(|l| l != msg)
        {
            s.log.push(msg.to_string());
            s.lines += 1;
            if s.log.len() > SCAN_LOG {
                s.log.remove(0);
            }
        }
    }
    let status = child.wait()?;
    let out = reader.join().unwrap_or_default();
    if !status.success() {
        let msg = other.join("\n");
        anyhow::bail!(
            "{}",
            if msg.trim().is_empty() {
                "The scan failed."
            } else {
                msg.trim().trim_start_matches("keymule: ")
            }
        );
    }
    serde_json::from_str(&out).context("reading the scan's report")
}

pub(super) fn scan_status() -> Value {
    let s = SCAN.lock().unwrap();
    let elapsed = s.finished.or_else(|| s.started.map(|t| t.elapsed()));
    json!({
        "running": s.running,
        "id": s.id,
        "elapsed_ms": elapsed.map(|d| d.as_millis() as u64),
        "phase": s.phase,
        "log": s.log,
        "lines": s.lines,
        "done": s.done,
        "total": s.total,
        "found": s.found,
        "report": s.report,
        "error": s.error,
    })
}
