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
    let found = crate::servers::which(program)
        .with_context(|| format!("can't find `{program}` on PATH"))?;
    let real = found.canonicalize().unwrap_or_else(|_| found.clone());
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

/// The env vars to put the secret in: `--env`, else the service's own.
fn vars_for(store: &Store, name: &str, given: &[String]) -> Result<Vec<String>> {
    let vars = if given.is_empty() {
        let service = &store.get(name)?.service;
        match presets::get_or_generic(service).cli_env {
            Some((var, _)) => vec![var.to_string()],
            None => bail!(
                "say which env var the command reads the secret from: keymule exec {name} --env VAR -- <command…>"
            ),
        }
    } else {
        given.to_vec()
    };
    if let Some(bad) = vars.iter().find(|v| !valid_var(v)) {
        bail!("`{bad}` isn't an env var name (use capitals, digits and _)");
    }
    Ok(vars)
}

/// Everything up to starting the command: checks, the approval, the
/// command with the secret set. Returns it with the secret, for scrubbing.
pub fn prepare(
    vault: &dyn Vault,
    store: &Store,
    name: &str,
    given_vars: &[String],
    argv: &[String],
    dir: &Path,
    confirm: &dyn Fn(&str) -> Result<bool>,
) -> Result<(Command, String)> {
    let Some(program) = argv.first() else {
        bail!("usage: keymule exec <name> [--env VAR] -- <command> [args…]");
    };
    let vars = vars_for(store, name, given_vars)?;
    let path = resolve(program)?;
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let mut sealed = Sealed::load(vault, name, store.get(name)?)?;
    let program_s = path.to_string_lossy().into_owned();
    if !sealed
        .exec_grants
        .iter()
        .any(|g| g.covers(&program_s, &dir))
    {
        let tool = file_name(&path);
        let allowed = confirm(&format!(
            "Let {tool} use \"{name}\" in {}?\n\n\
             It gets the token as {}; keymule hides it in {tool}'s output. You won't be asked \
             again for {tool} in this folder.\n\n\
             Command:  {}\nProgram:  {}",
            tilde(&dir),
            vars.join(", "),
            argv.join(" "),
            tilde(&path),
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
    let mut cmd = Command::new(&path);
    cmd.args(&argv[1..]).current_dir(&dir);
    for var in crate::servers::STRIP_ENV {
        cmd.env_remove(var);
    }
    for var in &vars {
        cmd.env(var, &sealed.secret);
    }
    Ok((cmd, sealed.secret))
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
pub fn run(vault: &dyn Vault, name: &str, vars: &[String], argv: &[String]) -> Result<()> {
    let store = Store::load()?;
    let dir = std::env::current_dir()?;
    let (mut cmd, secret) = prepare(vault, &store, name, vars, argv, &dir, &|msg| {
        crate::dialog::confirm(msg)
    })?;
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
}
