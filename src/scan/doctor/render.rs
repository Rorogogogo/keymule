//! Showing the report.

use super::*;

pub(super) fn summarize_places(locs: &[&Location], limit: usize) -> Vec<String> {
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
