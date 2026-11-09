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
}

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
    /// Config entries that run it, e.g. "~/.claude.json · acme".
    #[serde(default)]
    pub used_by: Vec<String>,
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

    /// `base`, or `base-2`, `base-3`… whichever is free.
    pub fn free_name(&self, base: &str) -> String {
        if !self.creds.contains_key(base) {
            return base.to_string();
        }
        (2..)
            .map(|n| format!("{base}-{n}"))
            .find(|n| !self.creds.contains_key(n))
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

// ---- secret storage -------------------------------------------------------

pub trait Vault {
    fn set(&self, name: &str, secret: &str) -> Result<()>;
    fn get(&self, name: &str) -> Result<String>;
    fn delete(&self, name: &str) -> Result<()>;
}

/// macOS Keychain / Windows Credential Manager / Secret Service.
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

/// The secret store to use. Debug builds can point `KEYMULE_TEST_VAULT` at
/// a JSON file for end-to-end tests; release builds always use the keychain.
pub fn vault() -> Box<dyn Vault> {
    #[cfg(debug_assertions)]
    if let Some(path) = std::env::var_os("KEYMULE_TEST_VAULT") {
        return Box::new(FileVault(PathBuf::from(path)));
    }
    Box::new(Keychain)
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
mod tests {
    use super::*;

    #[test]
    fn host_of_keeps_nondefault_port() {
        assert_eq!(
            host_of("https://Bamboo.Corp.com/bamboo").unwrap(),
            "bamboo.corp.com"
        );
        assert_eq!(host_of("http://localhost:8085").unwrap(), "localhost:8085");
        assert!(host_of("file:///etc/passwd").is_err());
    }

    #[test]
    fn roundtrip_and_free_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.toml");
        let mut store = Store::load_from(path.clone()).unwrap();
        store.creds.insert(
            "bamboo".into(),
            Credential {
                service: "bamboo".into(),
                url: "https://bamboo.corp.com".into(),
                description: String::new(),
                auth: Auth::Basic {
                    username: "roro".into(),
                },
                hosts: vec!["bamboo.corp.com".into()],
                added_by: "cli".into(),
            },
        );
        store.save().unwrap();
        let again = Store::load_from(path).unwrap();
        assert_eq!(again.creds, store.creds);
        assert_eq!(again.free_name("bamboo"), "bamboo-2");
        assert_eq!(again.free_name("github"), "github");
    }

    #[test]
    fn names_are_validated() {
        assert!(validate_name("github-actions_2").is_ok());
        assert!(validate_name("bad name").is_err());
        assert!(validate_name("").is_err());
    }
}
