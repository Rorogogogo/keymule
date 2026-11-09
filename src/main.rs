mod adopt;
mod detect;
mod dialog;
mod doctor;
mod hook;
mod http;
mod init;
mod mcp;
mod presets;
mod servers;
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
    },
    /// Start an adopted MCP server with its secrets (used in agent configs).
    Exec {
        name: String,
        /// The server's command, exactly as adopted.
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
    /// Register keymule with Claude Code (MCP server, paste hook, CLAUDE.md).
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
        Cmd::Adopt { dry_run, yes } => adopt(dry_run, yes),
        Cmd::Exec { name, command } => servers::exec(store::vault().as_ref(), &name, &command),
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
    if store.creds.is_empty() && store.servers.is_empty() {
        println!("No credentials yet. Try: keymule add bamboo");
        return Ok(());
    }
    for (name, s) in &store.servers {
        let mut what = s.env.join(", ");
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
    for (name, c) in &store.creds {
        let url = if c.url.is_empty() {
            "(no URL: keymule edit --url)"
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
    if store.servers.contains_key(name) && !store.creds.contains_key(name) {
        return rotate_server(name);
    }
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

fn rotate_server(name: &str) -> Result<()> {
    let vault = store::vault();
    let mut binding = servers::Binding::load(vault.as_ref(), name)?;
    let mut changed = 0;
    for (var, value) in binding.env.iter_mut() {
        if let Some(new) =
            tty::secret_or_keep(&format!("New {var} for {name} (hidden, Enter keeps it): "))?
        {
            *value = new;
            changed += 1;
        }
    }
    for (i, value) in binding.args.values_mut().enumerate() {
        if let Some(new) = tty::secret_or_keep(&format!(
            "New argument secret #{} for {name} (hidden, Enter keeps it): ",
            i + 1
        ))? {
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

fn adopt(dry_run: bool, yes: bool) -> Result<()> {
    let opts = adopt::Options {
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

fn rm(name: &str) -> Result<()> {
    let mut store = Store::load()?;
    if store.servers.contains_key(name) && !store.creds.contains_key(name) {
        tty::confirm(&format!(
            "Delete MCP server \"{name}\" and its secrets? Configs that run `keymule exec {name}` will stop working."
        ))?;
        servers::Binding::delete(store::vault().as_ref(), name)?;
        store.servers.remove(name);
        store.save()?;
        println!("Deleted \"{name}\".");
        return Ok(());
    }
    store.get(name)?;
    tty::confirm(&format!("Delete \"{name}\" and its secret?"))?;
    store::vault().delete(name)?;
    store.creds.remove(name);
    store.save()?;
    println!("Deleted \"{name}\".");
    Ok(())
}
