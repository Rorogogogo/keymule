//! Credential metadata (never secrets) lives in a TOML file; secret values
//! live in the OS keychain under the service name "keymule".

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

pub const KEYCHAIN_SERVICE: &str = "keymule";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Auth {
    /// `Authorization: Bearer <secret>`
    Bearer,
    /// `Authorization: Basic base64(username:secret)`
    Basic { username: String },
    /// `<name>: <secret>`
    Header { name: String },
}

impl Auth {
    pub fn label(&self) -> String {
        match self {
            Auth::Bearer => "bearer".into(),
            Auth::Basic { username } => format!("basic ({username})"),
            Auth::Header { name } => format!("header {name}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credential {
    /// Preset id, e.g. "bamboo", "github", "generic".
    pub service: String,
    /// Base URL requests are resolved against. Empty until the user sets it.
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub description: String,
    pub auth: Auth,
    /// Hosts this secret may be sent to.
    #[serde(default)]
    pub hosts: Vec<String>,
    /// "cli" or "paste-hook".
    #[serde(default)]
    pub added_by: String,
    /// For an SSH login: the server and account it's for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh: Option<SshTarget>,
}

/// How an SSH login authenticates: the secret is a password or a private key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SshAuth {
    Password,
    /// A private key held in keymule's vault (pasted once).
    Key,
    /// A private key file already on this machine (`key_file`), used where
    /// it is; the vault keeps the password, if any, alongside.
    #[serde(rename = "key-file")]
    KeyFile,
}

/// The server an SSH login is for. Sealed with the secret, so editing
/// credentials.toml can't point the password at another machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshTarget {
    /// Host name or IP address.
    pub host: String,
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    pub username: String,
    pub auth: SshAuth,
    /// For `KeyFile`: the private key's absolute path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_file: Option<String>,
}

fn default_ssh_port() -> u16 {
    22
}

impl SshTarget {
    /// A checked target: host and username can't be taken for ssh options.
    pub fn new(host: &str, port: Option<u16>, username: &str, auth: SshAuth) -> Result<Self> {
        let host = host.trim().trim_start_matches('[').trim_end_matches(']');
        let ok_host = !host.is_empty()
            && host.len() <= 253
            && !host.starts_with('-')
            && host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '_'));
        if !ok_host {
            bail!("host should be a host name or IP address, like 203.0.113.7 or vps.example.com");
        }
        let username = username.trim();
        let ok_user = !username.is_empty()
            && username.len() <= 64
            && !username.starts_with('-')
            && username
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
        if !ok_user {
            bail!("username should be a login name like root or deploy");
        }
        let port = port.unwrap_or(22);
        if port == 0 {
            bail!("port should be between 1 and 65535");
        }
        Ok(Self {
            host: host.to_ascii_lowercase(),
            port,
            username: username.to_string(),
            auth,
            key_file: None,
        })
    }

    /// Log in with the private key at `path` (on this machine), checked to be
    /// one: a readable file whose first line is a PEM private key header.
    pub fn with_key_file(mut self, path: &str) -> Result<Self> {
        self.auth = SshAuth::KeyFile;
        self.key_file = Some(check_key_file(path)?);
        Ok(self)
    }

    /// The key file's path, with the home folder as `~`, if it has one.
    pub fn key_file_label(&self) -> Option<String> {
        self.key_file
            .as_deref()
            .map(|p| crate::cliexec::tilde(std::path::Path::new(p)))
    }

    /// "root@203.0.113.7" (with ":port" when it isn't 22).
    pub fn label(&self) -> String {
        if self.port == 22 {
            format!("{}@{}", self.username, self.host)
        } else {
            format!("{}@{}:{}", self.username, self.host, self.port)
        }
    }

    pub fn auth_label(&self) -> String {
        match self.auth {
            SshAuth::Password => "password".into(),
            SshAuth::Key => "private key".into(),
            SshAuth::KeyFile => format!(
                "key file {}",
                self.key_file_label().unwrap_or_else(|| "(none set)".into())
            ),
        }
    }
}

/// `path` (`~` expanded) made absolute, if it's a private key file. Only
/// its first line is read, to check the header.
pub fn check_key_file(path: &str) -> Result<String> {
    use std::io::Read;
    let path = path.trim();
    let expanded = match path.strip_prefix("~/") {
        Some(rest) => {
            let home = std::env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
            PathBuf::from(home).join(rest)
        }
        None => PathBuf::from(path),
    };
    if !expanded.is_absolute() {
        bail!("give the key file's full path, like ~/.ssh/id_ed25519");
    }
    if expanded.extension().is_some_and(|e| e == "pub") {
        bail!("{path} is a public key; give the private key (the same name without .pub)");
    }
    let real = expanded
        .canonicalize()
        .with_context(|| format!("can't find a key file at {path}"))?;
    if !real.is_file() {
        bail!("{path} isn't a file");
    }
    let mut head = [0u8; 80];
    let n = std::fs::File::open(&real)
        .and_then(|mut f| f.read(&mut head))
        .with_context(|| format!("can't read {path}"))?;
    let first = String::from_utf8_lossy(&head[..n]);
    let first = first.lines().next().unwrap_or("");
    if !(first.starts_with("-----BEGIN") && first.contains("PRIVATE KEY")) {
        bail!(
            "{path} doesn't look like a private key (it should start with -----BEGIN … PRIVATE KEY-----)"
        );
    }
    Ok(real.to_string_lossy().into_owned())
}

impl SshTarget {}

/// An MCP server whose secrets keymule holds (see `keymule adopt`). This is
/// a description for humans; what the secrets may be handed to is sealed in
/// the keychain item, not here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerMeta {
    /// Env vars keymule sets (names only).
    #[serde(default)]
    pub env: Vec<String>,
    /// Number of secrets keymule puts into arguments.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub arg_secrets: usize,
    /// HTTP headers keymule adds, for a remote server (names only).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<String>,
    /// Config entries that run it, e.g. "~/.claude.json · acme".
    #[serde(default)]
    pub used_by: Vec<String>,
    /// What it is, in a line (from add_mcp_secret or `keymule edit`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct FileFormat {
    #[serde(default)]
    credentials: BTreeMap<String, Credential>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    servers: BTreeMap<String, ServerMeta>,
}

pub struct Store {
    path: PathBuf,
    pub creds: BTreeMap<String, Credential>,
    pub servers: BTreeMap<String, ServerMeta>,
}

pub fn config_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("KEYMULE_HOME") {
        return Ok(PathBuf::from(dir));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
    Ok(PathBuf::from(home).join(".config").join("keymule"))
}

impl Store {
    pub fn load() -> Result<Self> {
        Self::load_from(config_dir()?.join("credentials.toml"))
    }

    pub fn load_from(path: PathBuf) -> Result<Self> {
        let file = match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str::<FileFormat>(&text)
                .with_context(|| format!("parsing {}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => FileFormat::default(),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        Ok(Self {
            path,
            creds: file.credentials,
            servers: file.servers,
        })
    }

    pub fn save(&self) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(&FileFormat {
            credentials: self.creds.clone(),
            servers: self.servers.clone(),
        })?;
        let tmp = self.path.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    pub fn get(&self, name: &str) -> Result<&Credential> {
        self.creds.get(name).ok_or_else(|| {
            anyhow!("no credential named \"{name}\" (run `keymule list` to see what is stored)")
        })
    }

    /// Move a credential to a new name: its vault item (secret, sealed
    /// hosts, approvals, git copies) and its entry. MCP server secrets keep
    /// their names, since agent configs run them by name.
    pub fn rename(
        &mut self,
        vault: &dyn Vault,
        name: &str,
        new_name: &str,
        description: Option<String>,
    ) -> Result<()> {
        validate_name(new_name)?;
        if self.servers.contains_key(name) && !self.creds.contains_key(name) {
            anyhow::bail!(
                "\"{name}\" is an MCP server secret; configs run it by that name, so it can't be renamed"
            );
        }
        let mut cred = self.get(name)?.clone();
        if new_name == name {
            anyhow::bail!("it's already called \"{name}\"");
        }
        if self.creds.contains_key(new_name) || self.servers.contains_key(new_name) {
            anyhow::bail!("\"{new_name}\" is taken; pick another name");
        }
        let raw = vault.get(name)?;
        vault.set(new_name, &raw)?;
        if let Some(d) = description {
            cred.description = d;
        }
        self.creds.remove(name);
        self.creds.insert(new_name.to_string(), cred);
        if let Err(e) = self.save() {
            let _ = vault.delete(new_name);
            return Err(e);
        }
        vault.delete(name)?;
        Ok(())
    }

    /// `base`, or `base-2`, `base-3`… whichever is free.
    pub fn free_name(&self, base: &str) -> String {
        let taken = |n: &str| self.creds.contains_key(n) || self.servers.contains_key(n);
        if !taken(base) {
            return base.to_string();
        }
        (2..)
            .map(|n| format!("{base}-{n}"))
            .find(|n| !taken(n))
            .unwrap()
    }
}

pub fn validate_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok {
        bail!("credential names may only use letters, digits, '-' and '_'");
    }
    Ok(())
}

/// "host" or "host:port" for a URL, lowercased. Errors on non-http(s) URLs.
pub fn host_of(url: &str) -> Result<String> {
    let parsed = reqwest::Url::parse(url).with_context(|| format!("invalid URL: {url}"))?;
    if parsed.scheme() != "https" && parsed.scheme() != "http" {
        bail!("only http(s) URLs are supported: {url}");
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow!("URL has no host: {url}"))?
        .to_ascii_lowercase();
    Ok(match parsed.port() {
        Some(port) => format!("{host}:{port}"),
        None => host,
    })
}

// ---- sealed credentials ----------------------------------------------------

/// Marks a keychain item that holds a [`Sealed`] credential rather than a
/// bare secret (items written before 0.4).
const SEAL_TAG: &str = "keymule-sealed-v1:";

/// A secret together with the rules for using it, stored as one keychain
/// item. credentials.toml is a plain file any agent can edit, so the hosts a
/// secret may go to and how it is sent are taken from here, never from the
/// file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sealed {
    pub secret: String,
    pub hosts: Vec<String>,
    pub auth: Auth,
    /// Programs (and the folder) the user let `keymule exec` hand it to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exec_grants: Vec<ExecGrant>,
    /// Copies the user put in git's credential store (`keymule install`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub installs: Vec<Install>,
    /// For an SSH login, the only server it's used for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh: Option<SshTarget>,
}

/// One program in one folder that may get a credential via `keymule exec`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecGrant {
    /// Absolute path, symlinks resolved.
    pub program: String,
    /// Absolute path of the working directory.
    pub dir: String,
}

impl ExecGrant {
    /// Whether this lets `program` run in `dir`: the same program, in the
    /// approved folder or any folder inside it.
    pub fn covers(&self, program: &str, dir: &std::path::Path) -> bool {
        self.program == program && dir.starts_with(&self.dir)
    }
}

/// A copy of a credential in git's credential store, for https://<host>
/// as <username>.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Install {
    pub host: String,
    pub username: String,
}

impl Install {
    pub fn label(&self) -> String {
        format!("git · {}@{}", self.username, self.host)
    }
}

impl Sealed {
    pub fn new(secret: impl Into<String>, cred: &Credential) -> Self {
        Self {
            secret: secret.into(),
            hosts: cred.hosts.clone(),
            auth: cred.auth.clone(),
            exec_grants: vec![],
            installs: vec![],
            ssh: cred.ssh.clone(),
        }
    }

    /// Parse a keychain item. Old bare secrets are sealed with `cred`'s
    /// current rules; the bool says whether that happened.
    fn parse(raw: String, cred: &Credential) -> Result<(Self, bool)> {
        match raw.strip_prefix(SEAL_TAG) {
            Some(json) => Ok((
                serde_json::from_str(json).context("keychain item is damaged")?,
                false,
            )),
            None => Ok((Self::new(raw, cred), true)),
        }
    }

    /// Load `name`, upgrading an old bare secret to a sealed item.
    pub fn load(vault: &dyn Vault, name: &str, cred: &Credential) -> Result<Self> {
        let (sealed, legacy) = Self::parse(vault.get(name)?, cred)?;
        if legacy {
            sealed.save(vault, name)?;
        }
        Ok(sealed)
    }

    /// Just the secret, without upgrading anything (for the doctor).
    pub fn peek_secret(vault: &dyn Vault, name: &str) -> Option<String> {
        let raw = vault.get(name).ok()?;
        match raw.strip_prefix(SEAL_TAG) {
            Some(json) => serde_json::from_str::<Self>(json).ok().map(|s| s.secret),
            None => Some(raw),
        }
    }

    pub fn save(&self, vault: &dyn Vault, name: &str) -> Result<()> {
        vault.set(name, &format!("{SEAL_TAG}{}", serde_json::to_string(self)?))
    }

    /// `cred` with the sealed hosts and auth: what requests must use.
    pub fn apply(&self, cred: &Credential) -> Credential {
        Credential {
            hosts: self.hosts.clone(),
            auth: self.auth.clone(),
            ssh: self.ssh.clone(),
            ..cred.clone()
        }
    }

    /// True if credentials.toml disagrees with the sealed rules, i.e. the
    /// file was edited by hand (or by something else).
    pub fn differs_from(&self, cred: &Credential) -> bool {
        self.hosts != cred.hosts || self.auth != cred.auth || self.ssh != cred.ssh
    }
}

// ---- secret storage -------------------------------------------------------

pub trait Vault {
    fn set(&self, name: &str, secret: &str) -> Result<()>;
    fn get(&self, name: &str) -> Result<String>;
    fn delete(&self, name: &str) -> Result<()>;
}

/// macOS Keychain / Windows Credential Manager / Secret Service, one item
/// per secret: how keymule stored secrets before `vault.rs`, and still the
/// place the vault's key lives.
pub struct Keychain;

impl Vault for Keychain {
    fn set(&self, name: &str, secret: &str) -> Result<()> {
        keyring::Entry::new(KEYCHAIN_SERVICE, name)?
            .set_password(secret)
            .context("writing to the keychain")
    }

    fn get(&self, name: &str) -> Result<String> {
        keyring::Entry::new(KEYCHAIN_SERVICE, name)?
            .get_password()
            .with_context(|| format!("reading \"{name}\" from the keychain"))
    }

    fn delete(&self, name: &str) -> Result<()> {
        match keyring::Entry::new(KEYCHAIN_SERVICE, name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e).context("deleting from the keychain"),
        }
    }
}

/// The secret store to use: keymule's encrypted file, keyed by the keychain
/// (see `vault.rs`). Debug builds can point `KEYMULE_TEST_VAULT` at a plain
/// JSON file for end-to-end tests.
pub fn vault() -> Box<dyn Vault> {
    #[cfg(debug_assertions)]
    if let Some(path) = std::env::var_os("KEYMULE_TEST_VAULT") {
        return Box::new(FileVault(PathBuf::from(path)));
    }
    match config_dir() {
        Ok(dir) => Box::new(crate::vault::EncVault::open_default(dir)),
        Err(_) => Box::new(Keychain),
    }
}

#[cfg(debug_assertions)]
struct FileVault(PathBuf);

#[cfg(debug_assertions)]
impl FileVault {
    fn read(&self) -> BTreeMap<String, String> {
        std::fs::read_to_string(&self.0)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }
    fn write(&self, map: &BTreeMap<String, String>) -> Result<()> {
        std::fs::write(&self.0, serde_json::to_string(map)?)?;
        Ok(())
    }
}

#[cfg(debug_assertions)]
impl Vault for FileVault {
    fn set(&self, name: &str, secret: &str) -> Result<()> {
        let mut m = self.read();
        m.insert(name.into(), secret.into());
        self.write(&m)
    }
    fn get(&self, name: &str) -> Result<String> {
        self.read()
            .remove(name)
            .ok_or_else(|| anyhow!("no secret for \"{name}\""))
    }
    fn delete(&self, name: &str) -> Result<()> {
        let mut m = self.read();
        m.remove(name);
        self.write(&m)
    }
}

#[cfg(test)]
pub mod testing {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Default)]
    pub struct MemVault(pub RefCell<HashMap<String, String>>);

    impl Vault for MemVault {
        fn set(&self, name: &str, secret: &str) -> Result<()> {
            self.0.borrow_mut().insert(name.into(), secret.into());
            Ok(())
        }
        fn get(&self, name: &str) -> Result<String> {
            self.0
                .borrow()
                .get(name)
                .cloned()
                .ok_or_else(|| anyhow!("missing"))
        }
        fn delete(&self, name: &str) -> Result<()> {
            self.0.borrow_mut().remove(name);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests;
