//! keymule's secret store: every secret in one encrypted file
//! (`vault.enc`, XChaCha20-Poly1305), and only the file's key in the OS
//! keychain. macOS then asks about one keychain item, once, instead of one
//! per secret, and the keychain's per-app rule still guards the key: another
//! program (an agent running `security`, say) can't read it silently.
//!
//! Where there is no keychain (some Linux boxes, WSL, CI), the key comes from
//! a passphrase in `KEYMULE_PASSPHRASE`, stretched with Argon2id.
//!
//! Secrets keymule kept as separate keychain items before this move into the
//! file the first time they are read (or all at once with `keymule migrate`).

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

use anyhow::{Context, Result, anyhow, bail};
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit},
};
use serde::{Deserialize, Serialize};

use crate::store::{KEYCHAIN_SERVICE, Vault};

/// The keychain item that holds the file's key.
const KEY_ITEM: &str = "vault-key";
const FORMAT: u32 = 1;

/// The secrets, and the passphrase salt if there is one.
type Contents = (BTreeMap<String, String>, Option<Vec<u8>>);

/// What's on disk: the secrets, encrypted, as JSON.
#[derive(Serialize, Deserialize)]
struct Envelope {
    v: u32,
    /// Argon2id salt, when the key comes from a passphrase.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    salt: Option<String>,
    nonce: String,
    data: String,
}

pub enum KeySource {
    Keychain,
    Passphrase(String),
    #[cfg(test)]
    Fixed([u8; 32]),
}

pub struct EncVault {
    path: PathBuf,
    source: KeySource,
    /// The keychain key, read once per process.
    key: OnceLock<[u8; 32]>,
    /// Where secrets were kept before (the keychain, one item each).
    legacy: Option<Box<dyn Vault>>,
}

fn random<const N: usize>() -> Result<[u8; N]> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|e| anyhow!("no randomness: {e}"))?;
    Ok(b)
}

impl EncVault {
    pub fn new(path: PathBuf, source: KeySource, legacy: Option<Box<dyn Vault>>) -> Self {
        Self {
            path,
            source,
            key: OnceLock::new(),
            legacy,
        }
    }

    /// `~/.config/keymule/vault.enc`, keyed by the keychain, or by
    /// `KEYMULE_PASSPHRASE` when that is set.
    pub fn open_default(dir: PathBuf) -> Self {
        let source = match std::env::var("KEYMULE_PASSPHRASE") {
            Ok(p) if !p.is_empty() => KeySource::Passphrase(p),
            _ => KeySource::Keychain,
        };
        Self::new(
            dir.join("vault.enc"),
            source,
            Some(Box::new(crate::store::Keychain)),
        )
    }

    fn keychain_key(&self) -> Result<[u8; 32]> {
        if let Some(k) = self.key.get() {
            return Ok(*k);
        }
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, KEY_ITEM)?;
        let key = match entry.get_password() {
            Ok(b64) => {
                let raw = B64.decode(b64.trim()).context("the vault key is damaged")?;
                <[u8; 32]>::try_from(raw.as_slice())
                    .map_err(|_| anyhow!("the vault key is damaged"))?
            }
            Err(keyring::Error::NoEntry) => {
                // Never replace a lost key while its file exists: the secrets
                // would be unreadable, then overwritten.
                if self.path.exists() {
                    bail!(
                        "keymule's vault key is missing from the keychain, so {} can't be opened. \
                         If you moved Macs, restore the \"keymule\" / \"{KEY_ITEM}\" keychain item.",
                        self.path.display()
                    );
                }
                let k = random::<32>()?;
                entry
                    .set_password(&B64.encode(k))
                    .context("saving keymule's vault key to the keychain")?;
                k
            }
            Err(e) => {
                return Err(anyhow!(e)).context(
                    "can't reach the keychain for keymule's vault key; \
                     set KEYMULE_PASSPHRASE to use a passphrase instead",
                );
            }
        };
        let _ = self.key.set(key);
        Ok(key)
    }

    fn key_for(&self, salt: Option<&[u8]>) -> Result<[u8; 32]> {
        match &self.source {
            KeySource::Keychain => self.keychain_key(),
            KeySource::Passphrase(p) => {
                let salt = salt.ok_or_else(|| anyhow!("the vault has no salt for a passphrase"))?;
                let mut k = [0u8; 32];
                argon2::Argon2::default()
                    .hash_password_into(p.as_bytes(), salt, &mut k)
                    .map_err(|e| anyhow!("deriving the key from KEYMULE_PASSPHRASE: {e}"))?;
                Ok(k)
            }
            #[cfg(test)]
            KeySource::Fixed(k) => Ok(*k),
        }
    }

    fn read(&self) -> Result<Contents> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok((BTreeMap::new(), None));
            }
            Err(e) => return Err(e).with_context(|| format!("reading {}", self.path.display())),
        };
        let env: Envelope = serde_json::from_str(&text)
            .with_context(|| format!("{} is damaged", self.path.display()))?;
        if env.v != FORMAT {
            bail!("{} was written by a newer keymule", self.path.display());
        }
        let salt = env.salt.as_deref().map(|s| B64.decode(s)).transpose()?;
        let key = self.key_for(salt.as_deref())?;
        let nonce = <[u8; 24]>::try_from(B64.decode(&env.nonce)?.as_slice())
            .map_err(|_| anyhow!("{} is damaged", self.path.display()))?;
        let plain = XChaCha20Poly1305::new_from_slice(&key)
            .map_err(|_| anyhow!("bad key length"))?
            .decrypt(&XNonce::from(nonce), B64.decode(&env.data)?.as_slice())
            .map_err(|_| {
                anyhow!(
                    "can't decrypt {}: wrong passphrase or vault key",
                    self.path.display()
                )
            })?;
        let map = serde_json::from_slice(&plain)
            .with_context(|| format!("{} is damaged", self.path.display()))?;
        Ok((map, salt))
    }

    fn write(&self, map: &BTreeMap<String, String>, salt: Option<Vec<u8>>) -> Result<()> {
        let salt = match (&self.source, salt) {
            (KeySource::Passphrase(_), None) => Some(random::<16>()?.to_vec()),
            (KeySource::Passphrase(_), s) => s,
            _ => None,
        };
        let key = self.key_for(salt.as_deref())?;
        let nonce = random::<24>()?;
        let data = XChaCha20Poly1305::new_from_slice(&key)
            .map_err(|_| anyhow!("bad key length"))?
            .encrypt(&XNonce::from(nonce), serde_json::to_vec(map)?.as_slice())
            .map_err(|_| anyhow!("encryption failed"))?;
        let env = Envelope {
            v: FORMAT,
            salt: salt.map(|s| B64.encode(s)),
            nonce: B64.encode(nonce),
            data: B64.encode(data),
        };
        let dir = self.path.parent().unwrap_or(std::path::Path::new("."));
        std::fs::create_dir_all(dir)?;
        let tmp = self.path.with_extension("enc.tmp");
        {
            let mut f = private_file(&tmp)?;
            f.write_all(serde_json::to_string(&env)?.as_bytes())?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("writing {}", self.path.display()))
    }

    /// Run a read-change-write under a lock, so two keymule processes (an
    /// MCP server and the paste hook, say) don't lose each other's change.
    fn change<T>(&self, f: impl FnOnce(&mut BTreeMap<String, String>) -> T) -> Result<T> {
        let dir = self.path.parent().unwrap_or(std::path::Path::new("."));
        std::fs::create_dir_all(dir)?;
        let lock = private_file_keep(&self.path.with_extension("lock"))?;
        lock.lock().context("locking keymule's vault")?;
        let (mut map, salt) = self.read()?;
        let before = map.clone();
        let out = f(&mut map);
        if map != before {
            self.write(&map, salt)?;
        }
        Ok(out)
    }

    /// Move every named secret still in the old keychain items into the file.
    /// Returns how many moved.
    pub fn migrate(&self, names: &[String]) -> Result<usize> {
        let mut moved = 0;
        for n in names {
            if self.read()?.0.contains_key(n) {
                continue;
            }
            if self.get(n).is_ok() {
                moved += 1;
            }
        }
        Ok(moved)
    }
}

#[cfg(unix)]
fn open_private(path: &std::path::Path, truncate: bool) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(truncate)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn open_private(path: &std::path::Path, truncate: bool) -> std::io::Result<File> {
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(truncate)
        .open(path)
}

fn private_file(path: &std::path::Path) -> Result<File> {
    open_private(path, true).with_context(|| format!("writing {}", path.display()))
}

fn private_file_keep(path: &std::path::Path) -> Result<File> {
    open_private(path, false).with_context(|| format!("opening {}", path.display()))
}

impl Vault for EncVault {
    fn set(&self, name: &str, secret: &str) -> Result<()> {
        self.change(|m| {
            m.insert(name.to_string(), secret.to_string());
        })
    }

    fn get(&self, name: &str) -> Result<String> {
        if let Some(v) = self.read()?.0.get(name) {
            return Ok(v.clone());
        }
        // Kept by an older keymule as its own keychain item: move it in.
        if let Some(legacy) = &self.legacy
            && let Ok(v) = legacy.get(name)
        {
            self.change(|m| {
                m.entry(name.to_string()).or_insert_with(|| v.clone());
            })?;
            let _ = legacy.delete(name);
            return Ok(v);
        }
        bail!("keymule holds no secret named \"{name}\"")
    }

    fn delete(&self, name: &str) -> Result<()> {
        self.change(|m| {
            m.remove(name);
        })?;
        if let Some(legacy) = &self.legacy {
            let _ = legacy.delete(name);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::testing::MemVault;

    fn vault(
        dir: &tempfile::TempDir,
        source: KeySource,
        legacy: Option<Box<dyn Vault>>,
    ) -> EncVault {
        EncVault::new(dir.path().join("vault.enc"), source, legacy)
    }

    #[test]
    fn keeps_secrets_encrypted_on_disk() {
        let d = tempfile::tempdir().unwrap();
        let v = vault(&d, KeySource::Fixed([7; 32]), None);
        v.set("github", "ghp_secret_value_123").unwrap();
        v.set("mcp:pg", "{\"env\":{}}").unwrap();
        assert_eq!(v.get("github").unwrap(), "ghp_secret_value_123");
        let disk = std::fs::read_to_string(d.path().join("vault.enc")).unwrap();
        assert!(!disk.contains("ghp_secret_value_123") && !disk.contains("github"));
        v.delete("github").unwrap();
        assert!(v.get("github").is_err());
        assert!(v.get("mcp:pg").is_ok());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(d.path().join("vault.enc"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn the_wrong_key_cannot_read_it() {
        let d = tempfile::tempdir().unwrap();
        vault(&d, KeySource::Fixed([1; 32]), None)
            .set("a", "b")
            .unwrap();
        let err = vault(&d, KeySource::Fixed([2; 32]), None)
            .get("a")
            .unwrap_err();
        assert!(format!("{err:#}").contains("can't decrypt"), "{err:#}");
    }

    #[test]
    fn a_passphrase_works_where_there_is_no_keychain() {
        let d = tempfile::tempdir().unwrap();
        vault(&d, KeySource::Passphrase("correct horse".into()), None)
            .set("a", "b")
            .unwrap();
        let again = vault(&d, KeySource::Passphrase("correct horse".into()), None);
        assert_eq!(again.get("a").unwrap(), "b");
        assert!(
            vault(&d, KeySource::Passphrase("wrong".into()), None)
                .get("a")
                .is_err()
        );
    }

    #[test]
    fn old_keychain_items_move_into_the_file() {
        let d = tempfile::tempdir().unwrap();
        let old = MemVault::default();
        old.set("notion", "secret_old").unwrap();
        old.set("21st", "key_old").unwrap();
        let legacy: Box<dyn Vault> = Box::new(old);
        let v = vault(&d, KeySource::Fixed([3; 32]), Some(legacy));
        assert_eq!(v.get("notion").unwrap(), "secret_old");
        assert_eq!(
            v.migrate(&["notion".into(), "21st".into(), "nothing".into()])
                .unwrap(),
            1
        );
        // Both now in the file, and gone from the old place.
        let reopened = vault(&d, KeySource::Fixed([3; 32]), None);
        assert_eq!(reopened.get("notion").unwrap(), "secret_old");
        assert_eq!(reopened.get("21st").unwrap(), "key_old");
        let Some(legacy) = &v.legacy else { panic!() };
        assert!(legacy.get("notion").is_err() && legacy.get("21st").is_err());
    }
}
