//! `keymule exec <credential> --env VAR -- <command…>`: hand a stored API
//! credential to a command-line tool (gh, glab…) in an env var, for the jobs
//! `http_request` can't do. The agent still never sees the secret:
//!
//! - The user approves each program, in each folder, once, in a dialog.
//!   Approvals are sealed with the secret, so editing a file can't add one.
//! - Shells, interpreters and anything that just prints its environment are
//!   refused: they'd hand the secret straight back.
//! - The secret is scrubbed from the command's output, also URL-encoded or
//!   base64.
//!
//! The program itself does get the secret, so this is only as safe as the
//! programs the user allows. The dialog says so.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::presets;
use crate::redact::Scrubber;
use crate::store::{ExecGrant, Sealed, Store, Vault};

/// Programs that would give the secret back: shells, interpreters, and
/// tools that print or pass on their environment or input.
const REFUSED: &[&str] = &[
    // shells
    "sh",
    "bash",
    "zsh",
    "fish",
    "dash",
    "ksh",
    "mksh",
    "tcsh",
    "csh",
    "nu",
    "pwsh",
    "elvish",
    "xonsh",
    "busybox",
    "toybox",
    // interpreters
    "python",
    "pypy",
    "node",
    "nodejs",
    "deno",
    "bun",
    "ruby",
    "irb",
    "perl",
    "php",
    "lua",
    "luajit",
    "tclsh",
    "wish",
    "osascript",
    "swift",
    "julia",
    "java",
    "jshell",
    "groovy",
    "R",
    "Rscript",
    "awk",
    "gawk",
    "mawk",
    "nawk",
    "sed",
    "expect",
    // print or pass on the environment
    "env",
    "printenv",
    "echo",
    "printf",
    "cat",
    "tee",
    "xargs",
    "nohup",
    "sudo",
    "doas",
    "su",
    "script",
    "time",
    "nice",
    "timeout",
    "watch",
    "command",
    "exec",
    "screen",
    "tmux",
    "make",
    "strace",
    "dtruss",
    "lldb",
    "gdb",
    "vim",
    "vi",
    "nvim",
    "emacs",
    "nano",
    "less",
    "more",
    "open",
    "launchctl",
    "keymule",
];

/// Whether `name` (a file name) is on the refused list, ignoring a version
/// suffix (`python3.12`, `node22`).
pub fn refused(name: &str) -> bool {
    let base = name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.' || c == '-');
    REFUSED.contains(&name) || REFUSED.contains(&base)
}

/// `path` with the home folder shown as `~`, for dialogs.
pub fn tilde(path: &Path) -> String {
    match std::env::var_os("HOME").map(PathBuf::from) {
        Some(home) => match path.strip_prefix(&home) {
            Ok(rel) if rel.as_os_str().is_empty() => "~".into(),
            Ok(rel) => format!("~/{}", rel.display()),
            Err(_) => path.display().to_string(),
        },
        None => path.display().to_string(),
    }
}

/// The approval `keymule exec` would need for the service's own CLI (gh
/// for GitHub) in `dir`, if that CLI is installed. For approving it ahead,
/// together with another dialog.
pub fn cli_grant(service: &str, dir: &Path) -> Option<(String, ExecGrant)> {
    let (_, cli) = presets::get_or_generic(service).cli_env?;
    let path = resolve(cli).ok()?;
    let dir = dir.canonicalize().ok()?;
    Some((
        cli.to_string(),
        ExecGrant {
            program: path.to_string_lossy().into_owned(),
            dir: dir.to_string_lossy().into_owned(),
        },
    ))
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The program `argv[0]` runs, symlinks resolved, or an error if it's one
/// that would give the secret back.
pub fn resolve(program: &str) -> Result<PathBuf> {
    resolve_in(program, false)
}

/// [`resolve`]; with `allow_all`, shells and interpreters are let through
/// too (the user turned allow-all on for this folder).
pub fn resolve_in(program: &str, allow_all: bool) -> Result<PathBuf> {
    let found = crate::servers::which(program)
        .with_context(|| format!("can't find `{program}` on PATH"))?;
    let real = found.canonicalize().unwrap_or_else(|_| found.clone());
    if allow_all {
        return Ok(real);
    }
    for name in [
        file_name(Path::new(program)),
        file_name(&found),
        file_name(&real),
    ] {
        if refused(&name) {
            bail!(
                "keymule won't hand a credential to `{name}`: a shell, interpreter or tool like it \
                 could print it straight back. Run the real tool directly, e.g. \
                 `keymule exec <name> --env GH_TOKEN -- gh pr create …`."
            );
        }
    }
    if let Some(interp) = script_interpreter(&real)
        && refused(&interp)
    {
        bail!(
            "keymule won't hand a credential to {}: it's a script run by `{interp}`, which could \
             print it straight back. Run the real tool directly.",
            real.display()
        );
    }
    Ok(real)
}

/// For a script (`#!` first line), the program that runs it: the file name
/// after `#!`, or after `#!/usr/bin/env [-S] [VAR=x]`.
fn script_interpreter(path: &Path) -> Option<String> {
    let mut head = [0u8; 256];
    let n = std::fs::File::open(path)
        .and_then(|mut f| f.read(&mut head))
        .ok()?;
    let line = head[..n].strip_prefix(b"#!")?;
    let line = String::from_utf8_lossy(line);
    let mut words = line.lines().next()?.split_whitespace();
    let first = file_name(Path::new(words.next()?));
    if first != "env" {
        return Some(first);
    }
    words
        .find(|w| !w.starts_with('-') && !w.contains('='))
        .map(|w| file_name(Path::new(w)))
}

fn valid_var(v: &str) -> bool {
    !v.is_empty()
        && v.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && !v.starts_with(|c: char| c.is_ascii_digit())
}

/// The env vars to put the secret in: `--env`, else the service's own. AWS
/// keys always go in AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY; a secret
/// handed over only as a file (`--file`) needs no env var.
fn vars_for(store: &Store, name: &str, given: &[String], files: &[String]) -> Result<Vec<String>> {
    let preset = presets::get_or_generic(&store.get(name)?.service);
    if preset.kind == presets::Kind::Login {
        bail!(
            "\"{name}\" is a website login: it's only filled into its site's sign-in page, with sign_in"
        );
    }
    let vars = if preset.kind == presets::Kind::AwsKeys {
        if !given.is_empty() {
            bail!(
                "\"{name}\" is AWS keys: they always go in AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY, so leave out --env"
            );
        }
        vec!["AWS_ACCESS_KEY_ID".into(), "AWS_SECRET_ACCESS_KEY".into()]
    } else if !given.is_empty() || !files.is_empty() {
        given.to_vec()
    } else {
        match preset.cli_env {
            Some((var, _)) => vec![var.to_string()],
            None if preset.kind == presets::Kind::Ssh => bail!(
                "\"{name}\" is an SSH login: run commands on its server with keymule ssh {name} -- <command>"
            ),
            None if preset.kind == presets::Kind::PrivateKey => bail!(
                "say how the command gets the key: as a file, keymule exec {name} --file KEY -- ssh -i {{KEY}} …, or in an env var with --env VAR"
            ),
            None => bail!(
                "say which env var the command reads the secret from: keymule exec {name} --env VAR -- <command…>"
            ),
        }
    };
    if let Some(bad) = vars.iter().chain(files).find(|v| !valid_var(v)) {
        bail!("`{bad}` isn't an env var name (use capitals, digits and _)");
    }
    Ok(vars)
}

/// A secret written to a file for one command: in a folder only you can
/// read, removed when this is dropped (after the command ends).
pub struct KeyFiles {
    dir: PathBuf,
}

impl KeyFiles {
    pub fn new() -> Result<Self> {
        let mut b = [0u8; 8];
        getrandom::fill(&mut b).map_err(|e| anyhow::anyhow!("no randomness: {e}"))?;
        let tag: String = b.iter().map(|x| format!("{x:02x}")).collect();
        let dir = std::env::temp_dir().join(format!("keymule-{tag}"));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&dir)
            .context("making a private folder for the key")?;
        Ok(Self { dir })
    }

    pub fn write(&self, var: &str, secret: &str) -> Result<PathBuf> {
        let path = self.dir.join(var.to_ascii_lowercase());
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&path).context("writing the key file")?;
        f.write_all(secret.as_bytes())?;
        if !secret.ends_with('\n') {
            f.write_all(b"\n")?;
        }
        Ok(path)
    }
}

impl Drop for KeyFiles {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A command ready to run with a secret.
pub struct Prepared {
    pub cmd: Command,
    /// What to scrub from its output.
    pub scrub: String,
    /// Key files it was given; they go away when this is dropped.
    pub files: Option<KeyFiles>,
}

/// Everything up to starting the command: checks, the approval, the
/// command with the secret set. Returns it with the secret, for scrubbing.
#[cfg(test)]
pub fn prepare(
    vault: &dyn Vault,
    store: &Store,
    name: &str,
    given_vars: &[String],
    argv: &[String],
    dir: &Path,
    confirm: &dyn Fn(&str) -> Result<bool>,
) -> Result<(Command, String)> {
    prepare_in(vault, store, name, given_vars, argv, dir, confirm, false)
}

/// [`prepare`], and with `allow_all` (the folder has allow-all on) a new
/// program runs without asking and without being remembered, so turning
/// allow-all off brings the question back.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub fn prepare_in(
    vault: &dyn Vault,
    store: &Store,
    name: &str,
    given_vars: &[String],
    argv: &[String],
    dir: &Path,
    confirm: &dyn Fn(&str) -> Result<bool>,
    allow_all: bool,
) -> Result<(Command, String)> {
    let p = prepare_with(
        vault,
        store,
        name,
        given_vars,
        &[],
        argv,
        dir,
        confirm,
        allow_all,
    )?;
    Ok((p.cmd, p.scrub))
}

/// [`prepare_in`] with `files`: env vars that get the path of a file holding
/// the secret (for a private key), also put where `{VAR}` is in the command.
#[allow(clippy::too_many_arguments)]
pub fn prepare_with(
    vault: &dyn Vault,
    store: &Store,
    name: &str,
    given_vars: &[String],
    files: &[String],
    argv: &[String],
    dir: &Path,
    confirm: &dyn Fn(&str) -> Result<bool>,
    allow_all: bool,
) -> Result<Prepared> {
    let Some(program) = argv.first() else {
        bail!("usage: keymule exec <name> [--env VAR] [--file VAR] -- <command> [args…]");
    };
    let vars = vars_for(store, name, given_vars, files)?;
    let kind = presets::get_or_generic(&store.get(name)?.service).kind;
    let path = resolve_in(program, allow_all)?;
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let mut sealed = Sealed::load(vault, name, store.get(name)?)?;
    let program_s = path.to_string_lossy().into_owned();
    let granted = sealed
        .exec_grants
        .iter()
        .any(|g| g.covers(&program_s, &dir));
    if !granted && allow_all {
        crate::http::audit(
            name,
            "AUTO-ALLOW",
            &format!("{} in {} (allow all)", path.display(), dir.display()),
            0,
        );
    } else if !granted {
        let tool = file_name(&path);
        let allowed = confirm(&format!(
            "Let {tool} use \"{name}\"?\n\n        {}\n\n\
             As {} in {}. keymule hides it from the output.",
            argv.join(" "),
            vars.iter()
                .cloned()
                .chain(files.iter().map(|f| format!("a file ({f})")))
                .collect::<Vec<_>>()
                .join(", "),
            tilde(&dir),
        ))
        .context("keymule needs to ask you about a new program, but can't show a dialog here")?;
        if !allowed {
            bail!(
                "you didn't allow `{program}` to use \"{name}\" in {}; it was not run",
                dir.display()
            );
        }
        sealed.exec_grants.push(ExecGrant {
            program: program_s,
            dir: dir.to_string_lossy().into_owned(),
        });
        sealed.save(vault, name)?;
    }
    let mut args: Vec<String> = argv[1..].to_vec();
    let mut cmd = Command::new(&path);
    cmd.current_dir(&dir);
    for var in crate::servers::STRIP_ENV {
        cmd.env_remove(var);
    }
    let mut scrub = sealed.secret.clone();
    if kind == presets::Kind::AwsKeys {
        let (id, secret) = sealed.secret.split_once('\n').ok_or_else(|| {
            anyhow::anyhow!("\"{name}\" should hold an access key ID and a secret, on two lines")
        })?;
        cmd.env("AWS_ACCESS_KEY_ID", id.trim());
        cmd.env("AWS_SECRET_ACCESS_KEY", secret.trim());
        scrub = secret.trim().to_string();
    } else {
        for var in &vars {
            cmd.env(var, &sealed.secret);
        }
    }
    let key_files = if files.is_empty() {
        None
    } else {
        let kf = KeyFiles::new()?;
        for var in files {
            let file = kf.write(var, &sealed.secret)?;
            let file = file.to_string_lossy().into_owned();
            cmd.env(var, &file);
            for a in &mut args {
                *a = a.replace(&format!("{{{var}}}"), &file);
            }
        }
        Some(kf)
    };
    cmd.args(&args);
    Ok(Prepared {
        cmd,
        scrub,
        files: key_files,
    })
}

pub fn pump(mut from: impl Read, mut to: impl Write, secret: &str) {
    let mut scrub = Scrubber::new(secret);
    let mut buf = [0u8; 8192];
    loop {
        match from.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let _ = to.write_all(&scrub.push(&buf[..n]));
                let _ = to.flush();
            }
        }
    }
    let _ = to.write_all(&scrub.finish());
    let _ = to.flush();
}

/// Run it with its output scrubbed, and exit with its exit code.
pub fn run(
    vault: &dyn Vault,
    name: &str,
    vars: &[String],
    files: &[String],
    argv: &[String],
) -> Result<()> {
    let store = Store::load()?;
    let dir = std::env::current_dir()?;
    let all = crate::trust::covering(vault, &dir, crate::trust::now()).is_some();
    let Prepared {
        mut cmd,
        scrub: secret,
        files: key_files,
    } = prepare_with(
        vault,
        &store,
        name,
        vars,
        files,
        argv,
        &dir,
        &|msg| crate::dialog::confirm(msg),
        all,
    )?;
    crate::http::audit(
        name,
        "EXEC",
        &format!("{} in {}", argv[0], dir.display()),
        0,
    );
    let mut child = cmd
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("starting {}", argv[0]))?;
    let out = child.stdout.take().expect("piped");
    let err = child.stderr.take().expect("piped");
    let s1 = secret.clone();
    let t1 = std::thread::spawn(move || pump(out, std::io::stdout(), &s1));
    let t2 = std::thread::spawn(move || pump(err, std::io::stderr(), &secret));
    let status = child.wait()?;
    let _ = t1.join();
    let _ = t2.join();
    drop(key_files); // remove key files before exiting
    std::process::exit(status.code().unwrap_or(1));
}

#[cfg(test)]
mod tests;
