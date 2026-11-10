//! MCP servers whose secrets keymule holds. Their config entry becomes
//! `keymule exec <name> -- <original command>`. The secrets and the exact
//! command lines they may be handed to are sealed together in one keychain
//! item, so editing a config file (or running `keymule exec <name> -- env`)
//! doesn't get them out.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::store::Vault;

/// Keychain item names for servers start with this, so they can't clash
/// with HTTP credentials.
const ITEM_PREFIX: &str = "mcp:";

/// Placeholders like `{keymule:1}` in arguments are replaced by secrets.
pub fn placeholder(n: usize) -> String {
    format!("{{keymule:{n}}}")
}

/// Env vars that make an interpreter load extra code. They're dropped so a
/// caller can't use them to run something that dumps the secrets.
const STRIP_ENV: &[&str] = &[
    "NODE_OPTIONS",
    "NODE_PATH",
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "PYTHONSTARTUP",
    "PYTHONINSPECT",
    "PERL5OPT",
    "RUBYOPT",
    "BASH_ENV",
];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    /// Env vars to set for the server.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// `{keymule:N}` placeholder → secret, for secrets passed as arguments.
    #[serde(default)]
    pub args: BTreeMap<String, String>,
    /// Command lines (program and arguments, with placeholders) allowed to
    /// receive the secrets.
    #[serde(default)]
    pub commands: Vec<Vec<String>>,
    /// Program name → absolute path it resolved to when adopted, so a
    /// changed PATH can't swap in a different program.
    #[serde(default)]
    pub programs: BTreeMap<String, String>,
}

impl Binding {
    pub fn load(vault: &dyn Vault, name: &str) -> Result<Self> {
        let raw = vault.get(&item(name))?;
        serde_json::from_str(&raw)
            .with_context(|| format!("keychain item for \"{name}\" is damaged"))
    }

    pub fn save(&self, vault: &dyn Vault, name: &str) -> Result<()> {
        vault.set(&item(name), &serde_json::to_string(self)?)
    }

    pub fn delete(vault: &dyn Vault, name: &str) -> Result<()> {
        vault.delete(&item(name))
    }

    /// Every secret value it holds.
    pub fn secrets(&self) -> impl Iterator<Item = &String> {
        self.env.values().chain(self.args.values())
    }

    /// Allow `argv` too, pinning its program's current location.
    pub fn allow(&mut self, argv: &[String]) {
        if !self.commands.iter().any(|c| c == argv) {
            self.commands.push(argv.to_vec());
        }
        if let Some(program) = argv.first()
            && !self.programs.contains_key(program)
            && let Some(path) = which(program)
        {
            self.programs
                .insert(program.clone(), path.to_string_lossy().into_owned());
        }
    }

    /// The command to run for `argv`, with secrets filled in. Refuses
    /// anything that isn't one of the allowed command lines.
    pub fn command(&self, name: &str, argv: &[String]) -> Result<std::process::Command> {
        if !self.commands.iter().any(|c| c == argv) {
            let allowed: Vec<String> = self.commands.iter().map(|c| c.join(" ")).collect();
            bail!(
                "\"{name}\" may only start: {}\nkeymule hands its secrets to the commands they were adopted with and nothing else.",
                allowed.join(" | ")
            );
        }
        let program = &argv[0];
        let path = match self.programs.get(program) {
            Some(p) if Path::new(p).exists() => PathBuf::from(p),
            Some(p) => {
                eprintln!("keymule: {p} no longer exists, looking up {program} in PATH");
                PathBuf::from(program)
            }
            None => PathBuf::from(program),
        };
        let mut cmd = std::process::Command::new(path);
        cmd.args(argv[1..].iter().map(|a| self.fill(a)));
        for var in STRIP_ENV {
            cmd.env_remove(var);
        }
        cmd.envs(&self.env);
        Ok(cmd)
    }

    fn fill(&self, arg: &str) -> String {
        let mut out = arg.to_string();
        for (ph, secret) in &self.args {
            out = out.replace(ph, secret);
        }
        out
    }
}

/// Ask the user whether `argv` may receive `name`'s secrets.
fn approve(name: &str, binding: &Binding, argv: &[String]) -> Result<()> {
    let vars: Vec<&str> = binding.env.keys().map(String::as_str).collect();
    let what = if vars.is_empty() {
        String::new()
    } else {
        format!(" ({})", vars.join(", "))
    };
    let allowed = crate::dialog::confirm(&format!(
        "An MCP server wants to start with your keymule secret \"{name}\"{what}.\n\n\
         Command:\n\n        {}\n\n\
         Only allow this if you set up this server.",
        argv.join(" ")
    ))
    .context("keymule needs to ask you about a new command, but can't show a dialog here")?;
    if !allowed {
        bail!(
            "you didn't allow `{}` to use \"{name}\"; it was not started",
            argv.join(" ")
        );
    }
    Ok(())
}

fn item(name: &str) -> String {
    format!("{ITEM_PREFIX}{name}")
}

/// Where `program` is found on PATH (not following symlinks, so Homebrew's
/// stable `/opt/homebrew/bin/npx` is kept rather than a versioned path).
pub fn which(program: &str) -> Option<PathBuf> {
    if program.contains('/') {
        let p = PathBuf::from(program);
        return p.is_file().then_some(p);
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(program))
        .find(|p| is_executable(p))
}

fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.metadata()
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// `keymule exec <name> [--env VAR] -- <command…>`: start an MCP server with
/// its secrets. A command line keymule hasn't seen with these secrets needs
/// the user's OK in a dialog first. `--env VAR` puts a single-value secret
/// into VAR (servers name their variable differently). On Unix keymule
/// replaces itself with the server, so stdio and signals behave as before.
pub fn exec(vault: &dyn Vault, name: &str, rename: &[String], argv: &[String]) -> Result<()> {
    if argv.is_empty() {
        bail!("usage: keymule exec <name> [--env VAR] -- <command> [args…]");
    }
    let mut binding = Binding::load(vault, name)
        .with_context(|| format!("keymule has no MCP server secret named \"{name}\""))?;
    if !binding.commands.iter().any(|c| c == argv) {
        approve(name, &binding, argv)?;
        binding.allow(argv);
        binding.save(vault, name)?;
    }
    let mut cmd = binding.command(name, argv)?;
    if !rename.is_empty() {
        let [(var, value)] = binding.env.iter().collect::<Vec<_>>()[..] else {
            bail!(
                "--env only works for a secret with exactly one value; \"{name}\" has {}",
                binding.env.len()
            );
        };
        cmd.env_remove(var);
        for to in rename {
            cmd.env(to, value);
        }
    }
    crate::http::audit(name, "START", &argv.join(" "), 0);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        Err(err).with_context(|| format!("starting {}", argv[0]))
    }
    #[cfg(not(unix))]
    {
        let status = cmd
            .status()
            .with_context(|| format!("starting {}", argv[0]))?;
        std::process::exit(status.code().unwrap_or(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::testing::MemVault;

    fn argv(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn only_allowed_commands_get_secrets() {
        let mut b = Binding::default();
        b.env.insert("API_KEY".into(), "s3cr3t-value".into());
        b.args.insert(placeholder(1), "arg-secret".into());
        b.allow(&argv("sh -c true --key={keymule:1}"));

        let vault = MemVault::default();
        b.save(&vault, "demo").unwrap();
        let b = Binding::load(&vault, "demo").unwrap();

        let cmd = b
            .command("demo", &argv("sh -c true --key={keymule:1}"))
            .unwrap();
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert_eq!(args, ["-c", "true", "--key=arg-secret"]);
        assert!(cmd.get_envs().any(|(k, v)| k == "API_KEY" && v.is_some()));
        assert!(
            cmd.get_envs()
                .any(|(k, v)| k == "NODE_OPTIONS" && v.is_none())
        );

        let err = b.command("demo", &argv("env")).unwrap_err().to_string();
        assert!(err.contains("may only start"), "{err}");
        assert!(!err.contains("s3cr3t"));
        assert!(b.command("demo", &argv("sh -c true")).is_err());
    }

    #[test]
    fn programs_are_pinned() {
        let mut b = Binding::default();
        b.allow(&argv("sh -c true"));
        let pinned = b.programs.get("sh").expect("sh is on PATH");
        assert!(pinned.ends_with("/sh"));
    }
}
