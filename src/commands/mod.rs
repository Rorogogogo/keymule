//! What each command does. `run` dispatches; the credential commands live in
//! [`credentials`], everything about setting keymule up in [`setup`].

mod credentials;
mod setup;

use std::io::Read;

use anyhow::{Result, bail};

use crate::cli::{Cli, Cmd, HookKind};
use crate::presets::PRESETS;
use crate::store::{self, Store};
use crate::*;
use credentials::*;
use setup::*;

pub async fn run(cli: Cli) -> Result<()> {
    match cli.cmd {
        Cmd::Add {
            service: Some(service),
            name,
            username,
            description,
            host,
            port,
            key,
            key_file,
            dialog,
            ..
        } if service == "ssh" => {
            add_ssh(
                name,
                host,
                port,
                username,
                key,
                key_file,
                description,
                dialog,
            )
            .await
        }
        Cmd::Add { host: Some(_), .. }
        | Cmd::Add { port: Some(_), .. }
        | Cmd::Add { key: true, .. }
        | Cmd::Add {
            key_file: Some(_), ..
        } => bail!("--host, --port, --key and --key-file are for `keymule add ssh`"),
        Cmd::Add {
            service,
            name,
            url,
            username,
            header,
            description,
            no_verify,
            dialog: true,
            ..
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
            ..
        } => add(service, name, url, username, header, description, no_verify).await,
        Cmd::List { json: false } => list(),
        Cmd::List { json: true } => list_json(),
        Cmd::Rename {
            name,
            new_name,
            description,
        } => {
            let mut store = Store::load()?;
            store.rename(store::vault().as_ref(), &name, &new_name, description)?;
            http::audit(&new_name, "RENAME", &format!("from {name}"), 0);
            println!("Renamed \"{name}\" to \"{new_name}\".");
            Ok(())
        }
        Cmd::Edit {
            name,
            url,
            username,
            service,
            host,
            port,
            key_file,
            description,
        } => edit(
            &name,
            url,
            username,
            service,
            host,
            port,
            key_file,
            description,
        ),
        Cmd::Rotate {
            name,
            no_verify,
            dialog,
        } => rotate(&name, no_verify, dialog).await,
        Cmd::Reveal { name } => reveal(&name),
        Cmd::Test { name } => credentials::test(&name).await,
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
            files,
            command,
        } => {
            if Store::load()?.creds.contains_key(&name) {
                cliexec::run(store::vault().as_ref(), &name, &rename, &files, &command)
            } else if !files.is_empty() {
                bail!("--file is for credentials; MCP server secrets go in env vars")
            } else {
                servers::exec(store::vault().as_ref(), &name, &rename, &command)
            }
        }
        Cmd::Ssh { name, command } => ssh::run(store::vault().as_ref(), &name, &command),
        Cmd::Convert {
            name,
            to,
            host,
            port,
            username,
            description,
        } => {
            let to = ssh::ConvertTo::parse(&to, host.as_deref(), port, username.as_deref())?;
            let mut store = Store::load()?;
            let msg = ssh::convert(
                store::vault().as_ref(),
                &mut store,
                &name,
                to,
                description,
                &|m| trust::confirm(m),
            )?;
            println!("{msg}");
            Ok(())
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
                &|m| trust::confirm(m),
                &install::RealGit,
            )?;
            println!("{msg}");
            Ok(())
        }
        Cmd::GhSecret {
            items,
            repo,
            secret,
            via,
            dry_run,
        } => {
            let store = Store::load()?;
            let transfers = ghsecret::parse_transfers(&items, secret.as_deref())?;
            let via = via.as_deref().map(ghsecret::Via::parse);
            let msg = ghsecret::push(
                store::vault().as_ref(),
                &store,
                &transfers,
                &repo,
                via.as_ref(),
                dry_run,
                &|m| trust::confirm(m),
            )
            .await?;
            println!("{msg}");
            Ok(())
        }
        Cmd::Allow {
            dir,
            off,
            everywhere,
            status,
        } => {
            let vault = store::vault();
            let here = match dir {
                Some(d) => d,
                None => std::env::current_dir()?,
            };
            let project = trust::project_of(&here);
            if status {
                let on = trust::list(vault.as_ref(), trust::now());
                if on.is_empty() {
                    println!("Allow all is off everywhere: keymule asks before each use.");
                }
                for a in on {
                    println!(
                        "Allow all is on in {}, {}.",
                        cliexec::tilde(std::path::Path::new(&a.dir)),
                        trust::until_text(&a)
                    );
                }
            } else if off {
                let gone = trust::revoke(vault.as_ref(), (!everywhere).then_some(here.as_path()))?;
                if gone.is_empty() {
                    println!("Allow all wasn't on there; nothing changed.");
                }
                for a in gone {
                    http::audit("-", "ALLOW-ALL-OFF", &a.dir, 0);
                    println!(
                        "Allow all is off in {}: keymule asks again.",
                        cliexec::tilde(std::path::Path::new(&a.dir))
                    );
                }
            } else if let Some(a) = trust::covering(vault.as_ref(), &project, trust::now()) {
                println!(
                    "Allow all is already on in {}, {}.",
                    cliexec::tilde(std::path::Path::new(&a.dir)),
                    trust::until_text(&a)
                );
            } else {
                match trust::ask(vault.as_ref(), &project)? {
                    Some(a) => {
                        http::audit("-", "ALLOW-ALL-ON", &a.dir, 0);
                        println!(
                            "Allow all is on in {}, {}. keymule won't ask there again, \
                             except before deleting. Turn it off: keymule allow --off",
                            cliexec::tilde(&project),
                            trust::until_text(&a)
                        );
                    }
                    None => bail!("the user didn't turn on allow all; keymule keeps asking"),
                }
            }
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
        Cmd::Menubar { background, login } => menubar_cmd(background, login),
        Cmd::Init { dry_run } => init::run(dry_run),
        Cmd::Mcp => mcp::serve().await,
        Cmd::Hook {
            kind: HookKind::Prompt { agent },
        } => {
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input)?;
            let mut store = Store::load()?;
            if let Some(out) =
                hook::run_as(agent, &input, &mut store, store::vault().as_ref(), &|m| {
                    dialog::paste_choice(m)
                })
            {
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
            let input = hook::mod_input(&read_stdin()?);
            let store = Store::load()?;
            println!(
                "{}",
                hook::scan_json(&input, cwd.as_deref(), &store, store::vault().as_ref())
            );
            Ok(())
        }
        Cmd::Hook {
            kind: HookKind::Hold,
        } => {
            let cwd = std::env::current_dir()
                .ok()
                .and_then(|d| d.to_str().map(str::to_string));
            let input = hook::mod_input(&read_stdin()?);
            let store = Store::load()?;
            println!(
                "{}",
                hook::hold_json(&input, cwd.as_deref(), &store, store::vault().as_ref())
            );
            Ok(())
        }
        Cmd::Hook {
            kind: HookKind::Save { name },
        } => {
            let cwd = std::env::current_dir()
                .ok()
                .and_then(|d| d.to_str().map(str::to_string));
            let input = hook::mod_input(&read_stdin()?);
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
