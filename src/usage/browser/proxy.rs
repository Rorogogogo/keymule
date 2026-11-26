//! keymule browser: runs the browser MCP server on keymule's Chrome and hides stored website passwords from its replies.

use super::*;

/// The flag that points a browser MCP server at an existing Chrome.
pub(super) fn endpoint_args(command: &[String], ws: &str) -> Result<Vec<String>> {
    let joined = command.join(" ");
    if joined.contains("playwright") {
        Ok(vec!["--cdp-endpoint".into(), ws.into()])
    } else if joined.contains("chrome-devtools-mcp") {
        Ok(vec![format!("--wsEndpoint={ws}")])
    } else {
        bail!(
            "keymule browser knows Playwright MCP (@playwright/mcp) and Chrome DevTools MCP \
             (chrome-devtools-mcp). Run one of them through it, e.g.\n  keymule browser -- npx @playwright/mcp@latest"
        )
    }
}

/// Every stored website password, in every form it could turn up in.
pub(super) struct Hidden {
    pub(super) forms: Vec<String>,
    pub(super) stamp: Stamp,
}

/// When the files a login lives in last changed: credentials.toml (a new
/// login) and the vault (a new or rotated password).
pub(super) type Stamp = Vec<Option<SystemTime>>;

impl Hidden {
    pub(super) fn new() -> Self {
        let mut h = Self {
            forms: vec![],
            stamp: vec![],
        };
        h.reload();
        h
    }

    pub(super) fn files_stamp() -> Stamp {
        let mut files = vec![];
        if let Ok(dir) = store::config_dir() {
            files.push(dir.join("credentials.toml"));
            files.push(dir.join("vault.enc"));
        }
        if let Some(test) = std::env::var_os("KEYMULE_TEST_VAULT") {
            files.push(PathBuf::from(test));
        }
        files
            .iter()
            .map(|f| std::fs::metadata(f).and_then(|m| m.modified()).ok())
            .collect()
    }

    pub(super) fn reload(&mut self) {
        self.stamp = Self::files_stamp();
        let Ok(store) = Store::load() else { return };
        let vault = store::vault();
        let mut forms: Vec<String> = store
            .creds
            .iter()
            .filter(|(_, c)| presets::get_or_generic(&c.service).kind == presets::Kind::Login)
            .filter_map(|(n, c)| Sealed::load(vault.as_ref(), n, c).ok())
            .flat_map(|s| {
                let mut f = crate::redact::forms(&s.secret);
                // Inside a JSON string, quotes and backslashes are escaped.
                if let Ok(j) = serde_json::to_string(&s.secret) {
                    f.push(j[1..j.len() - 1].to_string());
                }
                f
            })
            .filter(|f| f.len() >= 4)
            .collect();
        forms.sort_by_key(|f| std::cmp::Reverse(f.len()));
        forms.dedup();
        self.forms = forms;
    }

    /// Hide every stored password in one line of the server's output. A
    /// login added or rotated since is picked up before the line goes out
    /// (checking costs a couple of file stats).
    pub(super) fn scrub(&mut self, line: &str) -> String {
        if Self::files_stamp() != self.stamp {
            self.reload();
        }
        let mut out = line.to_string();
        for f in &self.forms {
            if out.contains(f.as_str()) {
                out = out.replace(f.as_str(), HIDDEN);
            }
        }
        out
    }
}

pub async fn run(command: &[String]) -> Result<()> {
    let (program, rest) = command.split_first().ok_or_else(|| {
        anyhow!(
            "say which browser MCP server to run: keymule browser -- npx @playwright/mcp@latest"
        )
    })?;
    let (state, mut chrome) = ensure_chrome().await?;
    let mut args: Vec<String> = rest.to_vec();
    args.extend(endpoint_args(command, &state.ws)?);
    let mut child = tokio::process::Command::new(program)
        .args(&args)
        .env("KEYMULE_BROWSER_WS", &state.ws)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("starting {program}"))?;
    let mut child_in = child.stdin.take().expect("piped");
    let child_out = child.stdout.take().expect("piped");

    // Agent -> browser tool, as is.
    let to_child = tokio::spawn(async move {
        let mut stdin = tokio::io::stdin();
        let _ = tokio::io::copy(&mut stdin, &mut child_in).await;
        let _ = child_in.shutdown().await;
    });
    // Browser tool -> agent, one JSON-RPC message per line, passwords hidden.
    let to_agent = tokio::spawn(async move {
        let mut hidden = tokio::task::spawn_blocking(Hidden::new)
            .await
            .unwrap_or_else(|_| Hidden {
                forms: vec![],
                stamp: vec![],
            });
        let mut lines = BufReader::new(child_out).lines();
        let mut out = tokio::io::stdout();
        while let Ok(Some(line)) = lines.next_line().await {
            let line = hidden.scrub(&line);
            if out.write_all(line.as_bytes()).await.is_err()
                || out.write_all(b"\n").await.is_err()
                || out.flush().await.is_err()
            {
                break;
            }
        }
    });
    let status = child.wait().await?;
    to_child.abort();
    let _ = to_agent.await;
    // The Chrome this call started goes with it; one another call started
    // stays for that call.
    if let Some(c) = chrome.as_mut() {
        let _ = c.kill();
        let _ = c.wait();
        let _ = std::fs::remove_file(state_path()?);
    }
    std::process::exit(status.code().unwrap_or(1));
}
