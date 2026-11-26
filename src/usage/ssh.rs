//! SSH logins: `keymule ssh <name> -- <command>` logs in to the one server
//! a stored login is for and runs a command there. The agent gets the
//! command's output, never the password or key:
//!
//! - A password goes to ssh through `SSH_ASKPASS`: ssh runs keymule itself
//!   as its helper, which reads the password from a file only you can read
//!   (in a private folder), deletes it, and answers only a password prompt.
//! - A private key goes to ssh as `-i <file>`, the same kind of file, which
//!   is removed when ssh ends.
//! - The server, port and account are sealed with the secret, so editing
//!   credentials.toml can't send the password to another machine. The agent
//!   can't add ssh options either (no ProxyCommand, no -o).
//! - The user approves ssh once per folder, in a dialog that names the
//!   server; the secret is scrubbed from the output.
//!
//! `keymule convert` turns an MCP server secret that's really an SSH login
//! (or a plain password) into a credential, moving the value inside the
//! vault: nobody sees it or types it again.

use std::path::Path;
use std::process::Stdio;

use anyhow::{Context, Result, anyhow, bail};

use crate::cliexec::{KeyFiles, Prepared, tilde};
use crate::presets;
use crate::servers::Binding;
use crate::store::{Auth, Credential, ExecGrant, Sealed, SshAuth, SshTarget, Store, Vault};

/// Set for ssh's askpass helper (keymule itself): the file holding the
/// password.
pub const ASKPASS_ENV: &str = "KEYMULE_ASKPASS";

/// The askpass file's name inside its private folder.
const ASKPASS_FILE: &str = "askpass";

/// When ssh runs keymule as its `SSH_ASKPASS` helper: answer a password
/// prompt from the file `KEYMULE_ASKPASS` names, once. Returns the exit
/// code, or None when this isn't an askpass call.
pub fn askpass_main() -> Option<i32> {
    let file = std::env::var_os(ASKPASS_ENV)?;
    let prompt = std::env::args().nth(1).unwrap_or_default();
    Some(match askpass_answer(Path::new(&file), &prompt) {
        Some(password) => {
            println!("{password}");
            0
        }
        None => 1,
    })
}

/// The password for `prompt`, read from `file` and then deleted. Only for
/// a password prompt (not a host key question or a passphrase), and only
/// from a file keymule made: `<temp>/keymule-…/askpass`.
fn askpass_answer(file: &Path, prompt: &str) -> Option<String> {
    let p = prompt.to_ascii_lowercase();
    if !p.contains("password") || p.contains("passphrase") {
        return None;
    }
    let file = file.canonicalize().ok()?;
    let folder = file.parent()?;
    let temp = std::env::temp_dir().canonicalize().ok()?;
    let ours = file.file_name()? == ASKPASS_FILE
        && folder.parent()? == temp
        && folder
            .file_name()?
            .to_string_lossy()
            .starts_with("keymule-");
    if !ours {
        return None;
    }
    let text = std::fs::read_to_string(&file).ok()?;
    let _ = std::fs::remove_file(&file); // one answer, then it's gone
    Some(text.strip_suffix('\n').unwrap_or(&text).to_string())
}

/// The login's server, from the sealed item (not the editable file).
fn target_of(store: &Store, sealed: &Sealed, name: &str) -> Result<SshTarget> {
    let cred = store.get(name)?;
    if presets::get_or_generic(&cred.service).kind != presets::Kind::Ssh {
        bail!(
            "\"{name}\" isn't an SSH login. Add one with add_ssh_credential (or `keymule add ssh`), \
             or turn an MCP server secret into one with `keymule convert {name} --to ssh`"
        );
    }
    sealed.ssh.clone().ok_or_else(|| {
        anyhow!("\"{name}\" has no server set: keymule edit {name} --host … --username …")
    })
}

/// Everything up to starting ssh: the checks, the approval, and the
/// command with the password or key handed over.
#[allow(clippy::too_many_arguments)]
pub fn prepare(
    vault: &dyn Vault,
    store: &Store,
    name: &str,
    command: &[String],
    dir: &Path,
    confirm: &dyn Fn(&str) -> Result<bool>,
    allow_all: bool,
    askpass: &Path,
) -> Result<Prepared> {
    let mut sealed = Sealed::load(vault, name, store.get(name)?)?;
    let target = target_of(store, &sealed, name)?;
    let ssh = crate::cliexec::resolve_in("ssh", allow_all)?;
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let program = ssh.to_string_lossy().into_owned();
    let granted = sealed.exec_grants.iter().any(|g| g.covers(&program, &dir));
    if !granted && allow_all {
        crate::http::audit(
            name,
            "AUTO-ALLOW",
            &format!("ssh {} in {} (allow all)", target.label(), dir.display()),
            0,
        );
    } else if !granted {
        let what = if command.is_empty() {
            "an interactive session".to_string()
        } else {
            command.join(" ")
        };
        let unusual = if ["/usr/bin/ssh", "/bin/ssh"].contains(&program.as_str()) {
            String::new()
        } else {
            format!("\n\nssh here is {}.", tilde(&ssh))
        };
        let allowed = confirm(&format!(
            "Let ssh log in to {} as \"{name}\"?\n\n        {what}\n\n\
             In {}. keymule types the {} itself; the agent only sees the output.{unusual}",
            target.label(),
            tilde(&dir),
            target.auth_label(),
        ))
        .context("keymule needs to ask you first, but can't show a dialog here")?;
        if !allowed {
            bail!(
                "you didn't allow ssh to log in to {} as \"{name}\" in {}; nothing ran",
                target.label(),
                dir.display()
            );
        }
        sealed.exec_grants.push(ExecGrant {
            program: program.clone(),
            dir: dir.to_string_lossy().into_owned(),
        });
        sealed.save(vault, name)?;
    }

    let files = KeyFiles::new()?;
    let mut cmd = std::process::Command::new(&ssh);
    cmd.current_dir(&dir);
    for var in crate::servers::STRIP_ENV {
        cmd.env_remove(var);
    }
    let port = target.port.to_string();
    let mut args: Vec<&str> = vec![
        "-p",
        &port,
        "-l",
        &target.username,
        // Batch use: one try, no question about a new server's key (a
        // changed key is still refused), and the user's BatchMode can't
        // switch the password prompt off.
        "-o",
        "NumberOfPasswordPrompts=1",
        "-o",
        "StrictHostKeyChecking=accept-new",
        "-o",
        "BatchMode=no",
    ];
    let key_file;
    match target.auth {
        SshAuth::Password => {
            let file = files.write(ASKPASS_FILE, &sealed.secret)?;
            cmd.env("SSH_ASKPASS", askpass)
                .env("SSH_ASKPASS_REQUIRE", "force")
                .env(ASKPASS_ENV, &file);
            args.extend([
                "-o",
                "PubkeyAuthentication=no",
                "-o",
                "PreferredAuthentications=keyboard-interactive,password",
            ]);
        }
        SshAuth::Key | SshAuth::KeyFile => {
            key_file = if target.auth == SshAuth::KeyFile {
                // Used where it is; checked again, it may have moved.
                let path = target.key_file.as_deref().ok_or_else(|| {
                    anyhow!(
                        "\"{name}\" has no key file set: keymule edit {name} --key-file ~/.ssh/…"
                    )
                })?;
                crate::store::check_key_file(path)?
            } else {
                files
                    .write("key", &sealed.secret)?
                    .to_string_lossy()
                    .into_owned()
            };
            cmd.env_remove(ASKPASS_ENV);
            args.extend([
                "-i",
                &key_file,
                "-o",
                "IdentitiesOnly=yes",
                "-o",
                "PreferredAuthentications=publickey",
            ]);
        }
    }
    // Debug builds only: end-to-end tests keep their servers' keys out of
    // the real ~/.ssh/known_hosts.
    #[cfg(debug_assertions)]
    let test_hosts = std::env::var("KEYMULE_TEST_KNOWN_HOSTS")
        .ok()
        .map(|f| format!("UserKnownHostsFile={f}"));
    #[cfg(debug_assertions)]
    if let Some(opt) = &test_hosts {
        args.extend(["-o", opt]);
    }
    cmd.args(&args).arg("--").arg(&target.host).args(command);
    Ok(Prepared {
        cmd,
        scrub: sealed.secret,
        files: Some(files),
    })
}

/// `keymule ssh <name> [-- command…]`: run it, output scrubbed, and exit
/// with ssh's exit code.
pub fn run(vault: &dyn Vault, name: &str, command: &[String]) -> Result<()> {
    use std::io::IsTerminal;
    let store = Store::load()?;
    let dir = std::env::current_dir()?;
    let all = crate::trust::covering(vault, &dir, crate::trust::now()).is_some();
    let exe = std::env::current_exe()?;
    let Prepared {
        mut cmd,
        scrub,
        files,
    } = prepare(
        vault,
        &store,
        name,
        command,
        &dir,
        &|m| crate::dialog::confirm(m),
        all,
        &exe,
    )?;
    let target = Sealed::load(vault, name, store.get(name)?)?
        .ssh
        .map(|t| t.label())
        .unwrap_or_default();
    crate::http::audit(
        name,
        "SSH",
        format!("{target} {}", command.join(" ")).trim_end(),
        0,
    );
    // A person at a terminal with no command gets a real session; anything
    // else has its output scrubbed on the way back.
    let interactive =
        command.is_empty() && std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let status = if interactive {
        cmd.status().context("starting ssh")?
    } else {
        let mut child = cmd
            .stdin(Stdio::inherit())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("starting ssh")?;
        let out = child.stdout.take().expect("piped");
        let err = child.stderr.take().expect("piped");
        let s1 = scrub.clone();
        let t1 = std::thread::spawn(move || crate::cliexec::pump(out, std::io::stdout(), &s1));
        let t2 = std::thread::spawn(move || crate::cliexec::pump(err, std::io::stderr(), &scrub));
        let status = child.wait()?;
        let _ = t1.join();
        let _ = t2.join();
        status
    };
    drop(files);
    std::process::exit(status.code().unwrap_or(255));
}

/// What `keymule convert` turns an MCP server secret into.
pub enum ConvertTo<'a> {
    Ssh {
        host: &'a str,
        port: Option<u16>,
        username: &'a str,
    },
    Password,
}

impl<'a> ConvertTo<'a> {
    pub fn parse(
        to: &str,
        host: Option<&'a str>,
        port: Option<u16>,
        username: Option<&'a str>,
    ) -> Result<Self> {
        match to {
            "ssh" => Ok(ConvertTo::Ssh {
                host: host
                    .ok_or_else(|| anyhow!("an SSH login needs the server's host or IP (host)"))?,
                port,
                username: username.ok_or_else(|| {
                    anyhow!("an SSH login needs the account to log in as (username)")
                })?,
            }),
            "password" if host.is_none() && username.is_none() => Ok(ConvertTo::Password),
            "password" => bail!("host and username are for --to ssh"),
            _ => {
                bail!("convert to \"ssh\" (an SSH login) or \"password\" (a password for programs)")
            }
        }
    }
}

fn looks_like_private_key(s: &str) -> bool {
    let s = s.trim_start();
    s.starts_with("-----BEGIN") && s.contains("PRIVATE KEY-----")
}

/// Turn MCP server secret `name` into a credential under the same name,
/// after the user says yes. The value moves inside the vault.
pub fn convert(
    vault: &dyn Vault,
    store: &mut Store,
    name: &str,
    to: ConvertTo,
    description: Option<String>,
    confirm: &dyn Fn(&str) -> Result<bool>,
) -> Result<String> {
    if store.creds.contains_key(name) {
        bail!("\"{name}\" is already a credential, not an MCP server secret");
    }
    let meta = store
        .servers
        .get(name)
        .cloned()
        .ok_or_else(|| anyhow!("keymule has no MCP server secret named \"{name}\""))?;
    if !meta.used_by.is_empty() {
        bail!(
            "\"{name}\" is still run by {}; an MCP server uses it, so it stays an MCP server secret",
            meta.used_by.join(", ")
        );
    }
    let binding = Binding::load(vault, name)?;
    let values: Vec<&String> = binding.secrets().collect();
    let [value] = values[..] else {
        bail!(
            "\"{name}\" holds {} values; only one with a single value can become a credential",
            values.len()
        );
    };
    let value = value.clone();
    let (service, ssh, question) = match to {
        ConvertTo::Ssh {
            host,
            port,
            username,
        } => {
            let auth = if looks_like_private_key(&value) {
                SshAuth::Key
            } else {
                SshAuth::Password
            };
            let t = SshTarget::new(host, port, username, auth)?;
            let q = format!(
                "Make \"{name}\" an SSH login for {}?\n\nIts {} will only be used to log in \
                 there, with keymule ssh. It stops being an MCP server secret.",
                t.label(),
                t.auth_label()
            );
            ("ssh", Some(t), q)
        }
        ConvertTo::Password => (
            "password",
            None,
            format!(
                "Make \"{name}\" a password for programs?\n\nThey get it with keymule exec \
                 {name} --env VAR, once you approve each one. It stops being an MCP server secret."
            ),
        ),
    };
    if !confirm(&question)? {
        bail!("the user didn't allow it; \"{name}\" is still an MCP server secret");
    }
    let cred = Credential {
        service: service.into(),
        url: String::new(),
        description: description
            .filter(|d| !d.trim().is_empty())
            .unwrap_or(meta.description.clone()),
        auth: Auth::Bearer,
        hosts: vec![],
        added_by: "convert".into(),
        ssh,
    };
    Sealed::new(value, &cred).save(vault, name)?;
    store.servers.remove(name);
    store.creds.insert(name.to_string(), cred.clone());
    if let Err(e) = store.save() {
        let _ = vault.delete(name);
        store.creds.remove(name);
        store.servers.insert(name.to_string(), meta);
        return Err(e);
    }
    Binding::delete(vault, name)?;
    crate::http::audit(
        name,
        "CONVERT",
        &format!("MCP server secret → {service}"),
        0,
    );
    Ok(match &cred.ssh {
        Some(t) => format!(
            "\"{name}\" is now an SSH login for {} ({}), under Credentials. Run commands there \
             with: keymule ssh {name} -- <command>",
            t.label(),
            t.auth_label()
        ),
        None => format!(
            "\"{name}\" is now a password, under Credentials. Hand it to a program with: \
             keymule exec {name} --env VAR -- <command>"
        ),
    })
}

#[cfg(test)]
mod tests;
