mod adopt;
mod cliexec;
mod detect;
mod dialog;
mod doctor;
mod ghsecret;
mod guard;
mod hook;
mod http;
mod init;
mod install;
mod mcp;
mod presets;
mod redact;
mod remote;
mod servers;
mod store;
mod tty;
mod ui;
mod vault;

use std::io::Read;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

use presets::{AuthKind, PRESETS};
use store::{Auth, Credential, Sealed, Store, host_of, validate_name};

/// Let AI agents use your tokens and passwords without ever seeing them.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Store a new credential (the secret is typed into a hidden prompt).
    Add {
        /// Service preset: bamboo, github, gitlab, jenkins, generic.
        service: Option<String>,
        #[arg(long)]
        name: Option<String>,
        /// Base URL of the API.
        #[arg(long)]
        url: Option<String>,
        /// Username, for services that use basic auth.
        #[arg(long)]
        username: Option<String>,
        /// Send the secret in this header instead of `Authorization: Bearer`.
        #[arg(long)]
        header: Option<String>,
        #[arg(long)]
        description: Option<String>,
        /// Skip checking the token against the service.
        #[arg(long)]
        no_verify: bool,
        /// Ask for the secret in a macOS dialog instead of the terminal.
        #[arg(long)]
        dialog: bool,
    },
    /// List stored credentials (never shows secrets).
    List {
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// Change a credential's URL, username, service or description.
    Edit {
        name: String,
        #[arg(long)]
        url: Option<String>,
        #[arg(long)]
        username: Option<String>,
        #[arg(long)]
        service: Option<String>,
        #[arg(long)]
        description: Option<String>,
    },
    /// Replace a credential's secret, e.g. after rotating it.
    Rotate {
        name: String,
        #[arg(long)]
        no_verify: bool,
        /// Ask for the new secret in a macOS dialog instead of the terminal.
        #[arg(long)]
        dialog: bool,
    },
    /// Show a stored secret in a macOS window. It is never printed, so an
    /// agent that runs this can't read it.
    Reveal { name: String },
    /// Check that a stored credential still works.
    Test { name: String },
    /// Delete a credential and its secret.
    Rm {
        name: String,
        /// Confirm in a macOS dialog instead of the terminal.
        #[arg(long)]
        dialog: bool,
    },
    /// Show the built-in service presets.
    Presets,
    /// Move secrets an older keymule kept as separate keychain items into
    /// keymule's encrypted vault (it also happens by itself as they're used).
    Migrate,
    /// Find secrets that leaked to AI models or sit in plain text where agents can read them.
    Doctor {
        /// Also list unsure matches (secrets without a known format).
        #[arg(long)]
        all: bool,
        /// Machine-readable output (secrets are still masked).
        #[arg(long)]
        json: bool,
        /// Project directory to check for .env and .mcp.json files (default: current directory).
        #[arg(long)]
        dir: Option<std::path::PathBuf>,
    },
    /// Move secrets that MCP server configs keep in plain text into the keychain.
    Adopt {
        /// Show what would change without changing anything.
        #[arg(long)]
        dry_run: bool,
        /// Don't ask for confirmation.
        #[arg(long)]
        yes: bool,
        /// Also adopt this MCP server even if its secrets look like defaults,
        /// e.g. a local database password (repeatable).
        #[arg(long = "server", value_name = "NAME")]
        servers: Vec<String>,
    },
    /// Run a command with a secret: an adopted MCP server (used in agent
    /// configs), or a CLI tool with a stored credential in an env var
    /// (`keymule exec github --env GH_TOKEN -- gh pr create …`).
    Exec {
        name: String,
        /// The env var to put the secret in (a credential: defaults to the
        /// service's, e.g. GH_TOKEN; an MCP server: renames its one value).
        #[arg(long = "env", value_name = "VAR")]
        rename: Vec<String>,
        /// The command to run.
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
    /// Copy a credential into git's credential store so git uses it for
    /// https://<host> as <username> (asks you first).
    Install {
        name: String,
        #[arg(long)]
        host: String,
        #[arg(long)]
        username: String,
        /// The repo folder: the service's CLI (gh) may use it there too,
        /// approved in the same dialog (default: this folder).
        #[arg(long)]
        dir: Option<std::path::PathBuf>,
    },
    /// Copy a credential into a GitHub Actions secret, encrypted with the
    /// repo's key (asks you first). Uses your stored GitHub credential.
    GhSecret {
        name: String,
        /// owner/repo
        #[arg(long)]
        repo: String,
        /// The Actions secret to create or replace, e.g. STRIPE_SECRET_KEY.
        #[arg(long = "as", value_name = "SECRET")]
        secret: String,
        /// The GitHub credential to upload with (default: your only one).
        #[arg(long)]
        via: Option<String>,
    },
    /// Remove a credential's copies from git's credential store (keeps it
    /// in keymule).
    Uninstall {
        name: String,
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        username: Option<String>,
    },
    /// Connect to a remote (HTTP) MCP server, adding the headers keymule
    /// holds (used in agent configs, as a stdio server).
    Remote {
        name: String,
        /// The server's URL, exactly as approved.
        url: String,
        /// An extra header that isn't secret, "Name: value" (repeatable).
        #[arg(long = "header", value_name = "HEADER")]
        headers: Vec<String>,
    },
    /// Open keymule's local web page: credentials, MCP servers, activity, leaks.
    Ui {
        /// Print the address instead of opening a browser.
        #[arg(long)]
        no_open: bool,
        /// Port on 127.0.0.1 (default: any free one).
        #[arg(long, default_value_t = 0)]
        port: u16,
    },
    /// Set keymule up in every agent found: Claude Code, Claude Desktop,
    /// Codex, Cursor, Antigravity (MCP server, paste hook, instructions).
    Init {
        #[arg(long)]
        dry_run: bool,
    },
    /// Run the MCP server on stdio (started by your agent).
    Mcp,
    /// Agent hooks.
    Hook {
        #[command(subcommand)]
        kind: HookKind,
    },
}

#[derive(Subcommand)]
enum HookKind {
    /// The prompt hook (Claude Code, Codex, Cursor): hold a prompt with a
    /// secret and ask.
    Prompt {
        /// Which agent runs it.
        #[arg(long, value_enum, default_value_t)]
        agent: hook::Agent,
    },
    /// Before an agent runs a shell command: block reading tokens back out
    /// of git's, gh's or the keychain's stores.
    Bash {
        /// Which agent runs it.
        #[arg(long, value_enum, default_value_t)]
        agent: hook::Agent,
    },
    /// For the Claude Code mod: find secrets in the prompt on stdin (output
    /// is masked).
    Scan,
    /// For the Claude Code mod: save the secrets in the prompt on stdin and
    /// print it with each swapped for its keymule name.
    Save {
        #[arg(long)]
        name: Option<String>,
    },
}

fn home() -> Result<std::path::PathBuf> {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("HOME is not set"))
}

#[tokio::main]
async fn main() {
    if let Err(e) = run(Cli::parse()).await {
        eprintln!("keymule: {e:#}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<()> {
    match cli.cmd {
        Cmd::Add {
            service,
            name,
            url,
            username,
            header,
            description,
            no_verify,
            dialog: true,
        } => {
            let msg = mcp::add_with_dialog(mcp::AddArgs {
                service: service.unwrap_or_else(|| "generic".into()),
                url,
                name,
                username,
                header,
                description,
                verify: Some(!no_verify),
            })
            .await?;
            println!("{msg}");
            Ok(())
        }
        Cmd::Add {
            service,
            name,
            url,
            username,
            header,
            description,
            no_verify,
            dialog: false,
        } => add(service, name, url, username, header, description, no_verify).await,
        Cmd::List { json: false } => list(),
        Cmd::List { json: true } => list_json(),
        Cmd::Edit {
            name,
            url,
            username,
            service,
            description,
        } => edit(&name, url, username, service, description),
        Cmd::Rotate {
            name,
            no_verify,
            dialog,
        } => rotate(&name, no_verify, dialog).await,
        Cmd::Reveal { name } => reveal(&name),
        Cmd::Test { name } => test(&name).await,
        Cmd::Rm { name, dialog } => rm(&name, dialog),
        Cmd::Migrate => migrate(),
        Cmd::Presets => {
            for p in PRESETS {
                println!("{:<8} {}  ({})", p.id, p.title, p.url_example);
            }
            Ok(())
        }
        Cmd::Doctor { all, json, dir } => {
            let opts = doctor::Options {
                home: home()?,
                dir: match dir {
                    Some(d) => d,
                    None => std::env::current_dir()?,
                },
                check_stored: true,
            };
            let report = doctor::run(&opts);
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", doctor::render(&report, all));
            }
            Ok(())
        }
        Cmd::Adopt {
            dry_run,
            yes,
            servers,
        } => adopt(dry_run, yes, servers),
        Cmd::Exec {
            name,
            rename,
            command,
        } => {
            if Store::load()?.creds.contains_key(&name) {
                cliexec::run(store::vault().as_ref(), &name, &rename, &command)
            } else {
                servers::exec(store::vault().as_ref(), &name, &rename, &command)
            }
        }
        Cmd::Install {
            name,
            host,
            username,
            dir,
        } => {
            let store = Store::load()?;
            let dir = match dir {
                Some(d) => d,
                None => std::env::current_dir()?,
            };
            let cli = cliexec::cli_grant(&store.get(&name)?.service, &dir);
            let msg = install::install(
                store::vault().as_ref(),
                &store,
                &name,
                &host,
                &username,
                cli,
                &|m| dialog::confirm(m),
                &install::RealGit,
            )?;
            println!("{msg}");
            Ok(())
        }
        Cmd::GhSecret {
            name,
            repo,
            secret,
            via,
        } => {
            let store = Store::load()?;
            let msg = ghsecret::push(
                store::vault().as_ref(),
                &store,
                &name,
                &repo,
                &secret,
                via.as_deref(),
                &|m| dialog::confirm(m),
            )
            .await?;
            println!("{msg}");
            Ok(())
        }
        Cmd::Uninstall {
            name,
            host,
            username,
        } => {
            let store = Store::load()?;
            let gone = install::uninstall(
                store::vault().as_ref(),
                &store,
                &name,
                host.as_deref(),
                username.as_deref(),
            )?;
            if gone.is_empty() {
                println!("\"{name}\" had no matching copy in git's credential store.");
            }
            for i in gone {
                println!("Removed {} (still in keymule as \"{name}\").", i.label());
            }
            Ok(())
        }
        Cmd::Ui { no_open, port } => ui::run(!no_open, port),
        Cmd::Remote { name, url, headers } => {
            remote::run(store::vault().as_ref(), &name, &headers, &url).await
        }
        Cmd::Init { dry_run } => init::run(dry_run),
        Cmd::Mcp => mcp::serve().await,
        Cmd::Hook {
            kind: HookKind::Prompt { agent },
        } => {
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input)?;
            let mut store = Store::load()?;
            if let Some(out) = hook::run_as(agent, &input, &mut store, store::vault().as_ref()) {
                println!("{out}");
            }
            Ok(())
        }
        Cmd::Hook {
            kind: HookKind::Bash { agent },
        } => {
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input)?;
            if let Some(out) = guard::run(agent, &input) {
                println!("{out}");
            }
            Ok(())
        }
        Cmd::Hook {
            kind: HookKind::Scan,
        } => {
            let cwd = std::env::current_dir()
                .ok()
                .and_then(|d| d.to_str().map(str::to_string));
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input)?;
            let store = Store::load()?;
            println!(
                "{}",
                hook::scan_json(&input, cwd.as_deref(), &store, store::vault().as_ref())
            );
            Ok(())
        }
        Cmd::Hook {
            kind: HookKind::Save { name },
        } => {
            let cwd = std::env::current_dir()
                .ok()
                .and_then(|d| d.to_str().map(str::to_string));
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input)?;
            let mut store = Store::load()?;
            println!(
                "{}",
                hook::save_json(
                    &input,
                    cwd.as_deref(),
                    name.as_deref(),
                    &mut store,
                    store::vault().as_ref()
                )
            );
            Ok(())
        }
    }
}

async fn add(
    service: Option<String>,
    name: Option<String>,
    url: Option<String>,
    username: Option<String>,
    header: Option<String>,
    description: Option<String>,
    no_verify: bool,
) -> Result<()> {
    let mut store = Store::load()?;
    let service = match service {
        Some(s) => s,
        None => {
            let ids: Vec<_> = PRESETS.iter().map(|p| p.id).collect();
            tty::ask_default(&format!("Service ({})", ids.join(", ")), "generic")?
        }
    };
    let Some(preset) = presets::find(&service) else {
        bail!("unknown service \"{service}\" (see `keymule presets`, or use generic)");
    };

    let name = match name {
        Some(n) => n,
        None => tty::ask_default("Name", &store.free_name(preset.id))?,
    };
    validate_name(&name)?;
    if store.creds.contains_key(&name) {
        bail!("\"{name}\" already exists; use `keymule rotate {name}` to replace its secret");
    }

    let url = match url.or(preset.default_url.map(String::from)) {
        Some(u) => u,
        None => tty::ask(&format!("Base URL (e.g. {}): ", preset.url_example))?,
    };
    let url = url.trim_end_matches('/').to_string();
    let host = host_of(&url)?;

    let username = match (username, preset.auth, &header) {
        (None, AuthKind::Basic, None) => Some(tty::ask("Username: ")?),
        (u, _, _) => u,
    };
    let auth = presets::auth_for(preset, header, username)?;

    let secret = tty::secret(&format!("{} token/password (hidden): ", preset.title))?;
    let hosts = presets::hosts_for(preset, &url)?;
    let cred = Credential {
        service: preset.id.into(),
        url,
        description: description.unwrap_or_default(),
        auth,
        hosts,
        added_by: "cli".into(),
    };

    if !no_verify {
        match http::verify(&name, &cred, &secret).await {
            Ok(Some(who)) => println!("✓ verified with {host} ({who})"),
            Ok(None) => println!(
                "· no verify endpoint for {}; saved without checking",
                preset.title
            ),
            Err(e) => {
                eprintln!("✗ {}", http::redact(&format!("{e:#}"), &secret));
                tty::confirm("Save it anyway?")?;
            }
        }
    }

    Sealed::new(secret, &cred).save(store::vault().as_ref(), &name)?;
    store.creds.insert(name.clone(), cred);
    store.save()?;
    println!("Saved \"{name}\". Your agent can now use it by name; it will never see the secret.");
    Ok(())
}

fn list() -> Result<()> {
    let store = Store::load()?;
    if store.creds.is_empty() && store.servers.is_empty() {
        println!("No credentials yet. Try: keymule add bamboo");
        return Ok(());
    }
    for (name, s) in &store.servers {
        let mut what = s
            .env
            .iter()
            .cloned()
            .chain(s.headers.iter().map(|h| format!("{h} header")))
            .collect::<Vec<_>>()
            .join(", ");
        if s.arg_secrets > 0 {
            what = format!("{what} (+{} in arguments)", s.arg_secrets);
        }
        println!(
            "{name:<16} {:<8} {:<40} used by {}",
            "mcp",
            what.trim(),
            s.used_by.len()
        );
    }
    let vault = store::vault();
    for (name, c) in &store.creds {
        let url = if c.url.is_empty() {
            "(no URL: keymule edit --url)"
        } else {
            &c.url
        };
        println!("{name:<16} {:<8} {:<40} {}", c.service, url, c.auth.label());
        if let Ok(sealed) = Sealed::load(vault.as_ref(), name, c)
            && sealed.differs_from(c)
        {
            println!(
                "  ⚠ credentials.toml was edited outside keymule; still only sent to {} ({})",
                sealed.hosts.join(", "),
                sealed.auth.label()
            );
        }
    }
    Ok(())
}

/// `keymule list --json`, for the Claude Code panel and scripts. No secrets.
fn list_json() -> Result<()> {
    let store = Store::load()?;
    let vault = store::vault();
    let creds: Vec<_> = store
        .creds
        .iter()
        .map(|(name, c)| {
            let sealed = Sealed::load(vault.as_ref(), name, c).ok();
            let used = sealed
                .as_ref()
                .map(|s| s.apply(c))
                .unwrap_or_else(|| c.clone());
            serde_json::json!({
                "name": name,
                "service": presets::get_or_generic(&c.service).title,
                "url": c.url,
                "hosts": used.hosts,
                "auth": used.auth.label(),
                "description": c.description,
                "added_by": c.added_by,
                "file_edited": sealed.as_ref().is_some_and(|s| s.differs_from(c)),
                "installed": sealed.as_ref().map(|s| s.installs.iter().map(|i| i.label()).collect::<Vec<_>>()).unwrap_or_default(),
                "exec_allowed": sealed.as_ref().map(|s| s.exec_grants.iter().map(|g| format!("{} in {}", g.program, g.dir)).collect::<Vec<_>>()).unwrap_or_default(),
                "missing_secret": sealed.is_none(),
            })
        })
        .collect();
    let servers: Vec<_> = store
        .servers
        .iter()
        .map(|(name, s)| {
            serde_json::json!({
                "name": name,
                "env": s.env,
                "headers": s.headers,
                "arg_secrets": s.arg_secrets,
                "used_by": s.used_by,
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "credentials": creds,
            "servers": servers,
            "audit_log": store::config_dir()?.join("audit.log"),
        }))?
    );
    Ok(())
}

fn edit(
    name: &str,
    url: Option<String>,
    username: Option<String>,
    service: Option<String>,
    description: Option<String>,
) -> Result<()> {
    let mut store = Store::load()?;
    let vault = store::vault();
    let mut sealed = Sealed::load(vault.as_ref(), name, store.get(name)?)?;
    // Start from the sealed rules, not whatever the file says now.
    let mut cred = sealed.apply(store.get(name)?);

    if let Some(s) = service {
        let Some(p) = presets::find(&s) else {
            bail!("unknown service \"{s}\"");
        };
        cred.service = p.id.into();
        if let AuthKind::Header(h) = p.auth {
            cred.auth = Auth::Header { name: h.into() };
        }
    }
    if let Some(u) = url {
        let u = u.trim_end_matches('/').to_string();
        let host = host_of(&u)?;
        if !cred.hosts.contains(&host) {
            // Pointing a secret at a new host is exactly what a prompt-injected
            // agent would try, so a human has to approve it on the terminal.
            tty::confirm(&format!("Allow \"{name}\" to be sent to {host}?"))?;
        }
        cred.hosts = presets::hosts_for(presets::get_or_generic(&cred.service), &u)?;
        cred.url = u;
    }
    if let Some(u) = username {
        cred.auth = Auth::Basic { username: u };
    }
    if let Some(d) = description {
        cred.description = d;
    }
    sealed.hosts = cred.hosts.clone();
    sealed.auth = cred.auth.clone();
    sealed.save(vault.as_ref(), name)?;
    store.creds.insert(name.to_string(), cred);
    store.save()?;
    println!("Updated \"{name}\".");
    Ok(())
}

async fn rotate(name: &str, no_verify: bool, dialog: bool) -> Result<()> {
    let store = Store::load()?;
    if store.servers.contains_key(name) && !store.creds.contains_key(name) {
        return rotate_server(name, dialog);
    }
    if dialog {
        let msg = mcp::rotate_with_dialog(mcp::RotateArgs {
            name: name.into(),
            verify: Some(!no_verify),
        })
        .await?;
        println!("{msg}");
        return Ok(());
    }
    let vault = store::vault();
    let mut sealed = Sealed::load(vault.as_ref(), name, store.get(name)?)?;
    let cred = sealed.apply(store.get(name)?);
    let secret = tty::secret(&format!("New secret for {name} (hidden): "))?;
    if !no_verify && !cred.url.is_empty() {
        match http::verify(name, &cred, &secret).await {
            Ok(Some(who)) => println!("✓ verified ({who})"),
            Ok(None) => {}
            Err(e) => {
                eprintln!("✗ {}", http::redact(&format!("{e:#}"), &secret));
                tty::confirm("Save it anyway?")?;
            }
        }
    }
    sealed.secret = secret;
    sealed.save(vault.as_ref(), name)?;
    for note in install::resync(&sealed) {
        eprintln!("! {note}");
    }
    if !sealed.installs.is_empty() {
        println!("Updated its copy in git's credential store too.");
    }
    println!("Rotated \"{name}\". Revoke the old token at the service if you haven't.");
    Ok(())
}

/// Show a credential's or an MCP server's secret in a keymule window. The
/// value never goes to stdout, where an agent running this would see it.
fn reveal(name: &str) -> Result<()> {
    let store = Store::load()?;
    let vault = store::vault();
    let (items, copy): (Vec<(String, String)>, Option<String>) =
        if let Some(c) = store.creds.get(name) {
            let sealed = Sealed::load(vault.as_ref(), name, c)?;
            let mut items = Vec::new();
            if let Auth::Basic { username } = &sealed.auth {
                items.push(("Username".to_string(), username.clone()));
            }
            items.push(("Secret".to_string(), sealed.secret.clone()));
            (items, Some(sealed.secret))
        } else if store.servers.contains_key(name) {
            let b = servers::Binding::load(vault.as_ref(), name)?;
            let items: Vec<(String, String)> = b
                .env
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .chain(
                    b.headers
                        .iter()
                        .map(|(k, v)| (format!("{k} header"), v.clone())),
                )
                .chain(
                    b.args
                        .values()
                        .enumerate()
                        .map(|(i, v)| (format!("Argument secret #{}", i + 1), v.clone())),
                )
                .collect();
            let copy = (items.len() == 1).then(|| items[0].1.clone());
            (items, copy)
        } else {
            bail!("keymule has no credential or MCP server named \"{name}\"");
        };
    if items.is_empty() {
        bail!("\"{name}\" holds no secret");
    }
    if dialog::reveal(name, &items, copy.as_deref())? {
        http::audit(name, "REVEAL", "", 0);
        println!("Showed \"{name}\" in a keymule window. The value isn't printed here.");
    } else {
        println!("Not shown.");
    }
    Ok(())
}

async fn test(name: &str) -> Result<()> {
    let store = Store::load()?;
    let sealed = Sealed::load(store::vault().as_ref(), name, store.get(name)?)?;
    let cred = sealed.apply(store.get(name)?);
    let secret = sealed.secret;
    match http::verify(name, &cred, &secret).await {
        Ok(Some(who)) => println!("✓ {name} works ({who})"),
        Ok(None) => println!("· {name}: no verify endpoint for this service"),
        Err(e) => bail!("{}", http::redact(&format!("{e:#}"), &secret)),
    }
    Ok(())
}

/// A new value for one of an adopted server's secrets, or None to keep it.
fn ask_new(dialog: bool, server: &str, what: &str) -> Result<Option<String>> {
    if !dialog {
        return tty::secret_or_keep(&format!(
            "New {what} for {server} (hidden, Enter keeps it): "
        ));
    }
    match dialog::secret(&format!(
        "Type the new {what} for the \"{server}\" MCP server.\n\nClick Cancel to keep the current one."
    ))? {
        dialog::Answer::Text(s) if !s.trim().is_empty() => Ok(Some(s.trim().to_string())),
        dialog::Answer::Text(_) | dialog::Answer::Cancelled => Ok(None),
        dialog::Answer::TimedOut => bail!("the dialog timed out; nothing changed"),
    }
}

fn rotate_server(name: &str, dialog: bool) -> Result<()> {
    let vault = store::vault();
    let mut binding = servers::Binding::load(vault.as_ref(), name)?;
    let mut changed = 0;
    for (var, value) in binding.env.iter_mut() {
        if let Some(new) = ask_new(dialog, name, var)? {
            *value = new;
            changed += 1;
        }
    }
    for (header, value) in binding.headers.iter_mut() {
        if let Some(new) = ask_new(dialog, name, &format!("the {header} header"))? {
            // Keep "Bearer " if the old value had it and the new one doesn't.
            *value = match value.split_once(' ') {
                Some((scheme, _))
                    if !new.contains(' ') && scheme.eq_ignore_ascii_case("bearer") =>
                {
                    format!("{scheme} {new}")
                }
                _ => new,
            };
            changed += 1;
        }
    }
    for (i, value) in binding.args.values_mut().enumerate() {
        if let Some(new) = ask_new(dialog, name, &format!("argument secret #{}", i + 1))? {
            *value = new;
            changed += 1;
        }
    }
    if changed == 0 {
        println!("Nothing changed.");
        return Ok(());
    }
    binding.save(vault.as_ref(), name)?;
    println!(
        "Rotated {changed} secret(s) for \"{name}\". Restart your agent so the server picks them up."
    );
    Ok(())
}

fn migrate() -> Result<()> {
    let store = Store::load()?;
    let mut names: Vec<String> = store.creds.keys().cloned().collect();
    names.extend(store.servers.keys().map(|n| servers::item(n)));
    let dir = store::config_dir()?;
    let v = vault::EncVault::open_default(dir.clone());
    let moved = v.migrate(&names)?;
    println!(
        "Moved {moved} secret(s) into {}. {} secret(s) in all; only the vault's key stays in the keychain.",
        dir.join("vault.enc").display(),
        names.len()
    );
    Ok(())
}

fn adopt(dry_run: bool, yes: bool, force: Vec<String>) -> Result<()> {
    let opts = adopt::Options {
        force,
        home: home()?,
        dir: std::env::current_dir()?,
        exe: std::env::current_exe()?.to_string_lossy().into_owned(),
    };
    let vault = store::vault();
    let mut store = Store::load()?;
    let plan = adopt::plan(&opts, &store, vault.as_ref())?;
    print!("{}", plan.render());
    if plan.targets.is_empty() || dry_run {
        if dry_run && !plan.targets.is_empty() {
            println!("\nDry run: nothing changed. Run `keymule adopt` to do it.");
        }
        return Ok(());
    }
    println!();
    if !yes {
        tty::confirm(&format!(
            "Move these secrets into the keychain and rewrite {}?",
            plan.files().join(", ")
        ))?;
    }
    // Configs may have changed while you were reading: plan again and make
    // sure it is still the plan you approved.
    let fresh = adopt::plan(&opts, &store, vault.as_ref())?;
    if fresh.fingerprint() != plan.fingerprint() {
        bail!("the configs changed while waiting for you; run `keymule adopt` again");
    }
    let done = adopt::apply(&fresh, &mut store, vault.as_ref(), &opts.home)?;
    println!(
        "✓ Moved {} secret(s) into the keychain as {}.",
        done.secrets,
        done.bindings.join(", ")
    );
    println!("✓ Rewrote {}.", fresh.files().join(", "));
    println!(
        "  Backups: {}. They still hold the old secrets: delete them once the servers work.",
        done.backups.join(", ")
    );
    println!("\nRestart Claude Code, Codex, Cursor or Claude Desktop so they pick up the change.");
    println!(
        "These secrets sat in plain text, so rotate them when you can: `keymule rotate <name>`."
    );
    Ok(())
}

/// A human has to say yes: on the terminal, or in keymule's own dialog.
fn confirm_rm(question: &str, dialog: bool) -> Result<()> {
    if !dialog {
        return tty::confirm(question);
    }
    if dialog::confirm_delete(question)? {
        Ok(())
    } else {
        bail!("cancelled; nothing was deleted")
    }
}

fn rm(name: &str, dialog: bool) -> Result<()> {
    let mut store = Store::load()?;
    if store.servers.contains_key(name) && !store.creds.contains_key(name) {
        confirm_rm(
            &format!(
                "Delete MCP server \"{name}\" and its secrets? Configs that run `keymule exec {name}` will stop working."
            ),
            dialog,
        )?;
        servers::Binding::delete(store::vault().as_ref(), name)?;
        store.servers.remove(name);
        store.save()?;
        println!("Deleted \"{name}\".");
        return Ok(());
    }
    let cred = store.get(name)?.clone();
    let vault = store::vault();
    let installs = Sealed::load(vault.as_ref(), name, &cred)
        .map(|s| s.installs.clone())
        .unwrap_or_default();
    let also = if installs.is_empty() {
        String::new()
    } else {
        let list: Vec<String> = installs.iter().map(|i| i.label()).collect();
        format!(
            " Its copy in git's credential store ({}) goes too.",
            list.join(", ")
        )
    };
    confirm_rm(
        &format!("Delete \"{name}\" and its secret? Agents using it will stop working.{also}"),
        dialog,
    )?;
    if let Ok(sealed) = Sealed::load(vault.as_ref(), name, &cred) {
        for note in install::erase_all(&sealed) {
            eprintln!("! {note}");
        }
    }
    vault.delete(name)?;
    store.creds.remove(name);
    store.save()?;
    println!("Deleted \"{name}\".");
    Ok(())
}
