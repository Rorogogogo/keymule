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
    fn new() -> Result<Self> {
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

    fn write(&self, var: &str, secret: &str) -> Result<PathBuf> {
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

fn pump(mut from: impl Read, mut to: impl Write, secret: &str) {
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
mod tests {
    use super::*;
    use crate::store::testing::MemVault;
    use crate::store::{Auth, Credential};
    use std::cell::Cell;

    const TOKEN: &str = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";

    fn setup() -> (tempfile::TempDir, Store, MemVault) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(dir.path().join("c.toml")).unwrap();
        let cred = Credential {
            service: "github".into(),
            url: "https://api.github.com".into(),
            description: String::new(),
            auth: Auth::Bearer,
            hosts: vec!["api.github.com".into()],
            added_by: "cli".into(),
        };
        let vault = MemVault::default();
        Sealed::new(TOKEN, &cred)
            .save(&vault, "github-acme")
            .unwrap();
        store.creds.insert("github-acme".into(), cred);
        (dir, store, vault)
    }

    fn argv(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn refuses_shells_interpreters_and_env_printers() {
        for p in [
            "sh",
            "bash",
            "zsh",
            "python3",
            "python3.12",
            "node",
            "perl",
            "env",
            "printenv",
            "echo",
            "cat",
        ] {
            assert!(refused(p), "{p}");
        }
        for p in ["gh", "glab", "git", "curl", "terraform"] {
            assert!(!refused(p), "{p}");
        }
        let (dir, store, vault) = setup();
        let never = |_: &str| -> Result<bool> { panic!("no dialog for a refused program") };
        for cmd in [
            "sh -c true",
            "env",
            "printenv GH_TOKEN",
            "/bin/cat /dev/null",
        ] {
            let err = prepare(
                &vault,
                &store,
                "github-acme",
                &[],
                &argv(cmd),
                dir.path(),
                &never,
            )
            .unwrap_err()
            .to_string();
            assert!(err.contains("won't hand a credential"), "{cmd}: {err}");
        }
    }

    #[test]
    fn a_symlink_to_a_shell_is_still_a_shell() {
        let (dir, store, vault) = setup();
        let link = dir.path().join("innocent-tool");
        std::os::unix::fs::symlink("/bin/sh", &link).unwrap();
        let never = |_: &str| -> Result<bool> { panic!("no dialog") };
        let err = prepare(
            &vault,
            &store,
            "github-acme",
            &[],
            &[link.to_string_lossy().into()],
            dir.path(),
            &never,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("won't hand a credential"), "{err}"); // sh, or dash on Linux
    }

    #[test]
    fn scripts_run_by_a_shell_or_interpreter_are_refused() {
        let (dir, store, vault) = setup();
        let never = |_: &str| -> Result<bool> { panic!("no dialog") };
        for (i, shebang) in [
            "#!/bin/sh",
            "#!/usr/bin/env python3",
            "#!/usr/bin/env -S node --flag",
            "#!/bin/bash -e",
        ]
        .iter()
        .enumerate()
        {
            let p = dir.path().join(format!("tool{i}"));
            std::fs::write(&p, format!("{shebang}\nprintenv GH_TOKEN > /tmp/x\n")).unwrap();
            std::fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o755))
                .unwrap();
            let err = prepare(
                &vault,
                &store,
                "github-acme",
                &[],
                &[p.to_string_lossy().into()],
                dir.path(),
                &never,
            )
            .unwrap_err()
            .to_string();
            assert!(err.contains("it's a script run by"), "{shebang}: {err}");
        }
    }

    #[test]
    fn asks_once_per_program_and_folder_and_sets_the_env_var() {
        let (dir, store, vault) = setup();
        let asked = Cell::new(0);
        let yes = |msg: &str| -> Result<bool> {
            asked.set(asked.get() + 1);
            assert!(
                msg.contains("github-acme") && msg.contains("GH_TOKEN") && !msg.contains(TOKEN)
            );
            Ok(true)
        };
        let run = |args: &str, d: &Path| {
            prepare(&vault, &store, "github-acme", &[], &argv(args), d, &yes)
        };
        let (cmd, secret) = run("true", dir.path()).unwrap();
        assert_eq!(secret, TOKEN);
        assert!(
            cmd.get_envs()
                .any(|(k, v)| k == "GH_TOKEN" && v.is_some_and(|v| v == TOKEN))
        );
        assert!(
            cmd.get_envs()
                .any(|(k, v)| k == "NODE_OPTIONS" && v.is_none())
        );
        run("true --other args", dir.path()).unwrap();
        assert_eq!(asked.get(), 1, "same program, same folder");
        let sub = dir.path().join("web/app");
        std::fs::create_dir_all(&sub).unwrap();
        run("true", &sub).unwrap();
        assert_eq!(asked.get(), 1, "a folder inside it is covered");
        let other = tempfile::tempdir().unwrap();
        run("true", other.path()).unwrap();
        assert_eq!(asked.get(), 2, "a new folder asks again");

        let no = |_: &str| -> Result<bool> { Ok(false) };
        let err = prepare(
            &vault,
            &store,
            "github-acme",
            &[],
            &argv("false"),
            dir.path(),
            &no,
        )
        .unwrap_err();
        assert!(err.to_string().contains("didn't allow"));
    }

    #[test]
    fn allow_all_runs_a_new_program_without_asking_or_remembering_it() {
        let (dir, store, vault) = setup();
        let never = |_: &str| -> Result<bool> { panic!("allow all: no dialog") };
        prepare_in(
            &vault,
            &store,
            "github-acme",
            &[],
            &argv("true"),
            dir.path(),
            &never,
            true,
        )
        .unwrap();
        let sealed =
            Sealed::load(&vault, "github-acme", store.get("github-acme").unwrap()).unwrap();
        assert!(sealed.exec_grants.is_empty(), "nothing remembered");
        prepare_in(
            &vault,
            &store,
            "github-acme",
            &[],
            &argv("sh -c true"),
            dir.path(),
            &never,
            true,
        )
        .expect("allow all lets shells through too");
        let err = prepare(
            &vault,
            &store,
            "github-acme",
            &[],
            &argv("sh -c true"),
            dir.path(),
            &never,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("won't hand a credential"),
            "without it, still refused"
        );
    }

    #[test]
    fn needs_a_valid_env_var_when_the_service_has_none() {
        let (dir, mut store, vault) = setup();
        store.creds.get_mut("github-acme").unwrap().service = "generic".into();
        let yes = |_: &str| -> Result<bool> { Ok(true) };
        let err = prepare(
            &vault,
            &store,
            "github-acme",
            &[],
            &argv("true"),
            dir.path(),
            &yes,
        )
        .unwrap_err();
        assert!(err.to_string().contains("--env VAR"), "{err}");
        let err = prepare(
            &vault,
            &store,
            "github-acme",
            &["gh token".into()],
            &argv("true"),
            dir.path(),
            &yes,
        )
        .unwrap_err();
        assert!(err.to_string().contains("isn't an env var"), "{err}");
        assert!(
            prepare(
                &vault,
                &store,
                "github-acme",
                &["ACME_TOKEN".into()],
                &argv("true"),
                dir.path(),
                &yes
            )
            .is_ok()
        );
    }

    fn kind_store(service: &str, secret: &str) -> (tempfile::TempDir, Store, MemVault) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(dir.path().join("credentials.toml")).unwrap();
        let vault = MemVault::default();
        let cred = Credential {
            service: service.into(),
            url: String::new(),
            description: String::new(),
            auth: Auth::Bearer,
            hosts: vec![],
            added_by: "test".into(),
        };
        Sealed::new(secret, &cred).save(&vault, "it").unwrap();
        store.creds.insert("it".into(), cred);
        (dir, store, vault)
    }

    fn env_of(cmd: &Command, var: &str) -> Option<String> {
        cmd.get_envs()
            .find(|(k, _)| *k == var)
            .and_then(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
    }

    #[test]
    fn a_private_key_goes_to_the_command_as_a_private_file() {
        let pem = "-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n-----END OPENSSH PRIVATE KEY-----";
        let (dir, store, vault) = kind_store("private-key", pem);
        let yes = |_: &str| -> Result<bool> { Ok(true) };
        let err = prepare_with(
            &vault,
            &store,
            "it",
            &[],
            &[],
            &argv("true"),
            dir.path(),
            &yes,
            false,
        )
        .err()
        .unwrap();
        assert!(err.to_string().contains("--file KEY"), "{err}");

        let p = prepare_with(
            &vault,
            &store,
            "it",
            &[],
            &["KEY".into()],
            &argv("true -i {KEY} host"),
            dir.path(),
            &yes,
            false,
        )
        .unwrap();
        let file = env_of(&p.cmd, "KEY").unwrap();
        let args: Vec<String> = p
            .cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, ["-i", file.as_str(), "host"]);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), format!("{pem}\n"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        drop(p);
        assert!(!std::path::Path::new(&file).exists(), "removed afterwards");
    }

    #[test]
    fn aws_keys_go_in_their_two_variables() {
        let (id, key) = (
            crate::detect::fake::aws_id(),
            crate::detect::fake::aws_secret(),
        );
        let (dir, store, vault) = kind_store("aws", &format!("{id}\n{key}"));
        let yes = |_: &str| -> Result<bool> { Ok(true) };
        let p = prepare_with(
            &vault,
            &store,
            "it",
            &[],
            &[],
            &argv("true"),
            dir.path(),
            &yes,
            false,
        )
        .unwrap();
        assert_eq!(
            env_of(&p.cmd, "AWS_ACCESS_KEY_ID").as_deref(),
            Some(id.as_str())
        );
        assert_eq!(
            env_of(&p.cmd, "AWS_SECRET_ACCESS_KEY").as_deref(),
            Some(key.as_str())
        );
        assert_eq!(p.scrub, key);
        assert!(
            prepare_with(
                &vault,
                &store,
                "it",
                &["X".into()],
                &[],
                &argv("true"),
                dir.path(),
                &yes,
                false
            )
            .is_err()
        );
    }

    #[test]
    fn a_password_needs_its_env_var_named() {
        let (dir, store, vault) = kind_store("password", "Tr0ub4dor&3x");
        let yes = |_: &str| -> Result<bool> { Ok(true) };
        let err = prepare_with(
            &vault,
            &store,
            "it",
            &[],
            &[],
            &argv("true"),
            dir.path(),
            &yes,
            false,
        )
        .err()
        .unwrap();
        assert!(err.to_string().contains("--env VAR"), "{err}");
        let p = prepare_with(
            &vault,
            &store,
            "it",
            &["PGPASSWORD".into()],
            &[],
            &argv("true"),
            dir.path(),
            &yes,
            false,
        )
        .unwrap();
        assert_eq!(
            env_of(&p.cmd, "PGPASSWORD").as_deref(),
            Some("Tr0ub4dor&3x")
        );
    }
}
