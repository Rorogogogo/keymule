mod detect;
mod dialog;
mod hook;
mod http;
mod init;
mod mcp;
mod presets;
mod store;
mod tty;

use std::io::Read;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

use presets::{AuthKind, PRESETS};
use store::{Auth, Credential, Store, host_of, validate_name};

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
    },
    /// List stored credentials (never shows secrets).
    List,
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
    },
    /// Check that a stored credential still works.
    Test { name: String },
    /// Delete a credential and its secret.
    Rm { name: String },
    /// Show the built-in service presets.
    Presets,
    /// Register agentkey with Claude Code (MCP server, paste hook, CLAUDE.md).
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
    /// Claude Code UserPromptSubmit: save pasted secrets and block the prompt.
    Prompt,
}

#[tokio::main]
async fn main() {
    if let Err(e) = run(Cli::parse()).await {
        eprintln!("agentkey: {e:#}");
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
        } => add(service, name, url, username, header, description, no_verify).await,
        Cmd::List => list(),
        Cmd::Edit {
            name,
            url,
            username,
            service,
            description,
        } => edit(&name, url, username, service, description),
        Cmd::Rotate { name, no_verify } => rotate(&name, no_verify).await,
        Cmd::Test { name } => test(&name).await,
        Cmd::Rm { name } => rm(&name),
        Cmd::Presets => {
            for p in PRESETS {
                println!("{:<8} {}  ({})", p.id, p.title, p.url_example);
            }
            Ok(())
        }
        Cmd::Init { dry_run } => init::run(dry_run),
        Cmd::Mcp => mcp::serve().await,
        Cmd::Hook {
            kind: HookKind::Prompt,
        } => {
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input)?;
            let mut store = Store::load()?;
            if let Some(out) = hook::run(&input, &mut store, store::vault().as_ref()) {
                println!("{out}");
            }
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
        bail!("unknown service \"{service}\" (see `agentkey presets`, or use generic)");
    };

    let name = match name {
        Some(n) => n,
        None => tty::ask_default("Name", &store.free_name(preset.id))?,
    };
    validate_name(&name)?;
    if store.creds.contains_key(&name) {
        bail!("\"{name}\" already exists; use `agentkey rotate {name}` to replace its secret");
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
    let cred = Credential {
        service: preset.id.into(),
        url,
        description: description.unwrap_or_default(),
        auth,
        hosts: vec![host.clone()],
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

    store::vault().set(&name, &secret)?;
    store.creds.insert(name.clone(), cred);
    store.save()?;
    println!("Saved \"{name}\". Your agent can now use it by name; it will never see the secret.");
    Ok(())
}

fn list() -> Result<()> {
    let store = Store::load()?;
    if store.creds.is_empty() {
        println!("No credentials yet. Try: agentkey add bamboo");
        return Ok(());
    }
    for (name, c) in &store.creds {
        let url = if c.url.is_empty() {
            "(no URL: agentkey edit --url)"
        } else {
            &c.url
        };
        println!("{name:<16} {:<8} {:<40} {}", c.service, url, c.auth.label());
    }
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
    let mut cred = store.get(name)?.clone();

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
        cred.hosts = vec![host];
        cred.url = u;
    }
    if let Some(u) = username {
        cred.auth = Auth::Basic { username: u };
    }
    if let Some(d) = description {
        cred.description = d;
    }
    store.creds.insert(name.to_string(), cred);
    store.save()?;
    println!("Updated \"{name}\".");
    Ok(())
}

async fn rotate(name: &str, no_verify: bool) -> Result<()> {
    let store = Store::load()?;
    let cred = store.get(name)?.clone();
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
    store::vault().set(name, &secret)?;
    println!("Rotated \"{name}\". Revoke the old token at the service if you haven't.");
    Ok(())
}

async fn test(name: &str) -> Result<()> {
    let store = Store::load()?;
    let cred = store.get(name)?;
    let secret = store::vault().get(name)?;
    match http::verify(name, cred, &secret).await {
        Ok(Some(who)) => println!("✓ {name} works ({who})"),
        Ok(None) => println!("· {name}: no verify endpoint for this service"),
        Err(e) => bail!("{}", http::redact(&format!("{e:#}"), &secret)),
    }
    Ok(())
}

fn rm(name: &str) -> Result<()> {
    let mut store = Store::load()?;
    store.get(name)?;
    tty::confirm(&format!("Delete \"{name}\" and its secret?"))?;
    store::vault().delete(name)?;
    store.creds.remove(name);
    store.save()?;
    println!("Deleted \"{name}\".");
    Ok(())
}
