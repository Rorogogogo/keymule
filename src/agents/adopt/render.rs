//! Showing the plan to the user.

use super::*;

impl Plan {
    pub(super) fn new_args(&self, t: &Target) -> Vec<String> {
        if let Some(url) = &t.url {
            return ["remote", &t.binding, url]
                .into_iter()
                .map(String::from)
                .chain(t.keep_headers.iter().map(|h| format!("--header={h}")))
                .collect();
        }
        ["exec", &t.binding, "--"]
            .into_iter()
            .map(String::from)
            .chain(t.argv.iter().cloned())
            .collect()
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        if self.targets.is_empty() {
            out.push_str("✓ No MCP server keeps a secret in plain text.\n");
        } else {
            let files: std::collections::BTreeSet<usize> =
                self.targets.iter().map(|t| t.config).collect();
            out.push_str(&format!(
                "{} MCP server entr{} in {} config file{} {} secrets in plain text:\n",
                self.targets.len(),
                if self.targets.len() == 1 { "y" } else { "ies" },
                files.len(),
                if files.len() == 1 { "" } else { "s" },
                if self.targets.len() == 1 {
                    "keeps"
                } else {
                    "keep"
                },
            ));
            let mut by_binding: BTreeMap<&str, Vec<&Target>> = BTreeMap::new();
            for t in &self.targets {
                by_binding.entry(&t.binding).or_default().push(t);
            }
            for (name, ts) in &by_binding {
                let t = ts[0];
                let mut what: Vec<String> = t
                    .secret_env
                    .iter()
                    .map(|(k, v)| format!("{k}={}", mask(v)))
                    .chain(
                        t.secret_headers
                            .iter()
                            .map(|(k, v)| format!("{k}: {}", mask_header(v))),
                    )
                    .collect();
                if !t.arg_secrets.is_empty() {
                    let n = t.arg_secrets.len();
                    what.push(format!("{n} in arguments"));
                }
                out.push_str(&format!("\n  {name}   {}\n", what.join(", ")));
                for t in ts {
                    out.push_str(&format!("      {}\n", t.place));
                }
            }
            if let Some(t) = self.targets.iter().find(|t| t.url.is_none()) {
                out.push_str(&format!(
                    "\nEach entry will start the same command through keymule, which adds the secrets. For example:\n  before: {}\n  after:  {} {}\n",
                    t.argv.join(" "),
                    self.exe,
                    self.new_args(t).join(" ")
                ));
            }
            if let Some(t) = self.targets.iter().find(|t| t.url.is_some()) {
                let names: Vec<&str> = t.secret_headers.keys().map(String::as_str).collect();
                out.push_str(&format!(
                    "\nRemote servers will connect through keymule, which adds the {} header{}. For example:\n  before: {} (headers in the config)\n  after:  {} {}\n",
                    names.join(", "),
                    if names.len() == 1 { "" } else { "s" },
                    t.url.as_deref().unwrap_or_default(),
                    self.exe,
                    self.new_args(t).join(" ")
                ));
            }
        }
        if !self.skipped.is_empty() {
            out.push_str("\nNot adopted:\n");
            for s in &self.skipped {
                out.push_str(&format!("  {}\n      {}\n", s.place, s.reason));
            }
        }
        out
    }

    /// A stable summary, to check nothing changed between showing the plan
    /// and applying it.
    pub fn fingerprint(&self) -> Vec<(String, String)> {
        self.targets
            .iter()
            .map(|t| (t.place.clone(), t.binding.clone()))
            .collect()
    }

    pub fn files(&self) -> Vec<String> {
        let mut seen = std::collections::BTreeSet::new();
        self.targets
            .iter()
            .filter(|t| seen.insert(t.config))
            .map(|t| self.configs[t.config].display.clone())
            .collect()
    }
}
