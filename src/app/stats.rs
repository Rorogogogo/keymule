//! Usage numbers for the page, added up from the whole audit log (which
//! keymule only ever appends to, so it is the history).
//!
//! Every use of a key counts, split by how it was used: an API call through
//! `http_request`, a command (`keymule exec`, ssh, gh-secret, a git copy), an
//! MCP server started with its secret, or a value shown to you (reveal).
//! Commands and server starts are logged when they start, so only API calls
//! and gh-secret uploads can be counted as failed.

use serde_json::{Value, json};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Api,
    Command,
    Mcp,
    Reveal,
}

const KINDS: [Kind; 4] = [Kind::Api, Kind::Command, Kind::Mcp, Kind::Reveal];

impl Kind {
    fn key(self) -> &'static str {
        match self {
            Kind::Api => "api",
            Kind::Command => "command",
            Kind::Mcp => "mcp",
            Kind::Reveal => "reveal",
        }
    }
    fn index(self) -> usize {
        self as usize
    }
}

/// What kind of use an audit line is, if it is one (pairing, renames, allow
/// all and so on aren't).
pub fn kind(method: &str) -> Option<Kind> {
    match method {
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS" => Some(Kind::Api),
        "EXEC" | "SSH" | "GH-SECRET" | "INSTALL" | "SIGN-IN" => Some(Kind::Command),
        "START" => Some(Kind::Mcp),
        "REVEAL" => Some(Kind::Reveal),
        _ => None,
    }
}

/// Whether a use failed. Only API calls, gh-secret uploads and website
/// sign-ins record how they went (status 0 there means the request never
/// got an answer).
fn failed(method: &str, status: u64) -> bool {
    let has_outcome = kind(method) == Some(Kind::Api) || matches!(method, "GH-SECRET" | "SIGN-IN");
    has_outcome && !(200..300).contains(&status)
}

#[derive(Default, Clone)]
struct Counts {
    by_kind: [u64; 4],
    failed: u64,
}

impl Counts {
    fn add(&mut self, k: Kind, fail: bool) {
        self.by_kind[k.index()] += 1;
        self.failed += u64::from(fail);
    }
    fn total(&self) -> u64 {
        self.by_kind.iter().sum()
    }
    fn json(&self) -> serde_json::Map<String, Value> {
        let mut m = serde_json::Map::new();
        for k in KINDS {
            m.insert(k.key().into(), self.by_kind[k.index()].into());
        }
        m.insert("failed".into(), self.failed.into());
        m.insert("total".into(), self.total().into());
        m
    }
}

#[derive(Default)]
struct Key {
    range: Counts,
    all_time: u64,
    last: u64,
    spark: Vec<u64>,
}

/// The day number of a timestamp in the viewer's time zone (`tz_min`
/// minutes east of UTC).
fn day(ts: u64, tz_min: i64) -> i64 {
    (ts as i64 + tz_min * 60).div_euclid(86_400)
}

/// Add up `log` (audit.log's text) for the `days` days ending today.
pub fn summarize(log: &str, now: u64, tz_min: i64, days: u32) -> Value {
    let days = days.clamp(1, 366) as i64;
    let today = day(now, tz_min);
    let first_day = today - days + 1;
    let mut daily = vec![Counts::default(); days as usize];
    let mut all = Counts::default();
    let mut keys: std::collections::BTreeMap<String, Key> = Default::default();
    let mut since = 0u64;

    for line in log.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 5 {
            continue;
        }
        let Ok(ts) = f[0].parse::<u64>() else {
            continue;
        };
        let Some(k) = kind(f[2]) else { continue };
        let status = f[4].parse::<u64>().unwrap_or(0);
        let fail = failed(f[2], status);
        if since == 0 || ts < since {
            since = ts;
        }
        all.add(k, fail);
        let d = day(ts, tz_min);
        let in_range = (first_day..=today).contains(&d);
        if in_range {
            daily[(d - first_day) as usize].add(k, fail);
        }
        let name = f[1];
        if name.is_empty() || name == "-" {
            continue;
        }
        let key = keys.entry(name.to_string()).or_insert_with(|| Key {
            spark: vec![0; days as usize],
            ..Key::default()
        });
        key.all_time += 1;
        key.last = key.last.max(ts);
        if in_range {
            key.range.add(k, fail);
            key.spark[(d - first_day) as usize] += 1;
        }
    }

    let mut keys: Vec<(String, Key)> = keys.into_iter().collect();
    keys.sort_by(|a, b| {
        b.1.range
            .total()
            .cmp(&a.1.range.total())
            .then(b.1.last.cmp(&a.1.last))
    });
    json!({
        "today": today,
        "days": days,
        "since": since,
        "all_time": all.json(),
        "daily": daily.iter().enumerate().map(|(i, c)| {
            let mut m = c.json();
            m.insert("d".into(), (first_day + i as i64).into());
            Value::Object(m)
        }).collect::<Vec<_>>(),
        "keys": keys.into_iter().map(|(name, k)| {
            let mut m = k.range.json();
            m.insert("name".into(), name.into());
            m.insert("all_time".into(), k.all_time.into());
            m.insert("last".into(), k.last.into());
            m.insert("spark".into(), k.spark.into());
            Value::Object(m)
        }).collect::<Vec<_>>(),
    })
}

/// [`summarize`] over the real audit log.
pub fn load(tz_min: i64, days: u32) -> Value {
    let text = crate::store::config_dir()
        .ok()
        .and_then(|d| std::fs::read_to_string(d.join("audit.log")).ok())
        .unwrap_or_default();
    summarize(&text, crate::trust::now(), tz_min, days)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400;
    // 2026-11-19 12:00 UTC
    const NOW: u64 = 1_795_089_600;

    fn log(lines: &[(u64, &str, &str, u64)]) -> String {
        lines
            .iter()
            .map(|(ts, name, method, status)| format!("{ts}\t{name}\t{method}\tx\t{status}\n"))
            .collect()
    }

    #[test]
    fn every_use_counts_by_kind() {
        let text = log(&[
            (NOW - 60, "github", "GET", 200),
            (NOW - 50, "github", "POST", 401),
            (NOW - 40, "github", "EXEC", 0),
            (NOW - 30, "vps", "SSH", 0),
            (NOW - 20, "notion", "START", 0),
            (NOW - 10, "github", "REVEAL", 0),
            (NOW - 5, "-", "PAIR", 0),
            (NOW - 4, "stripe", "GH-SECRET", 422),
        ]);
        let s = summarize(&text, NOW, 0, 7);
        let today = &s["daily"][6];
        assert_eq!(today["api"], 2);
        assert_eq!(today["command"], 3, "exec, ssh, gh-secret");
        assert_eq!(today["mcp"], 1);
        assert_eq!(today["reveal"], 1);
        assert_eq!(today["total"], 7, "pairing isn't a use");
        assert_eq!(
            today["failed"], 2,
            "the 401 and the 422; exec has no outcome"
        );
        assert_eq!(s["keys"][0]["name"], "github");
        assert_eq!(s["keys"][0]["total"], 4);
    }

    #[test]
    fn days_follow_the_viewers_time_zone_and_range() {
        // 23:30 UTC yesterday is already today 10 hours east of UTC.
        let late = NOW - 12 * 3600 - 1800;
        let old = NOW - 40 * DAY;
        let text = log(&[(late, "a", "GET", 200), (old, "a", "GET", 200)]);
        let utc = summarize(&text, NOW, 0, 30);
        assert_eq!(utc["daily"][28]["total"], 1, "yesterday in UTC");
        let sydney = summarize(&text, NOW, 600, 30);
        assert_eq!(sydney["daily"][29]["total"], 1, "today in Sydney");
        assert_eq!(
            sydney["keys"][0]["total"], 1,
            "the 40-day-old use is out of range"
        );
        assert_eq!(sydney["keys"][0]["all_time"], 2);
        assert_eq!(sydney["keys"][0]["last"], late);
        assert_eq!(sydney["since"], old);
    }

    #[test]
    fn unused_keys_still_report_when_they_were_last_used() {
        let text = log(&[(NOW - 60 * DAY, "old", "EXEC", 0)]);
        let s = summarize(&text, NOW, 0, 30);
        assert_eq!(s["keys"][0]["name"], "old");
        assert_eq!(s["keys"][0]["total"], 0);
        assert_eq!(s["keys"][0]["last"], NOW - 60 * DAY);
    }
}
