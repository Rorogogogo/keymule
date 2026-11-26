//! Approvals in the keymule page, for machines without a screen keymule can
//! pop a dialog on (Linux, a dev container), or a Mac with
//! `KEYMULE_APPROVALS=page`.
//!
//! A question waits as a file in keymule's config folder; the page (`keymule
//! ui`, opened in a browser, port-forwarded from a container if need be)
//! lists it, and the user answers there. An answer only counts when it's
//! signed by a paired browser: each browser makes a key pair that never
//! leaves it (WebCrypto, not extractable), and its public half is sealed in
//! the vault when it's paired. Something that can write files or call the
//! page's server, an agent included, still can't sign as that browser.
//!
//! A secret typed into the page is encrypted to a one-off key of the keymule
//! process that asked (P-256 ECDH, then AES-256-GCM), so it never sits in a
//! file in the clear.
//!
//! Limits, said plainly: on a machine where the vault's key is in reach of
//! the agent (a passphrase in the environment), an agent that sets out to
//! decrypt the vault itself can. This guards keymule's own paths.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::dialog::{Picked, Spec};
use crate::store::Vault;

/// The vault item with the paired browsers' public keys.
const DEVICES: &str = "approve:devices";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    /// Public key, SEC1 uncompressed, base64.
    pub key: String,
    pub added: u64,
}

pub fn devices(vault: &dyn Vault) -> Vec<Device> {
    vault
        .get(DEVICES)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_devices(vault: &dyn Vault, list: &[Device]) -> Result<()> {
    if list.is_empty() {
        let _ = vault.delete(DEVICES);
        return Ok(());
    }
    vault.set(DEVICES, &serde_json::to_string(list)?)
}

/// Check a browser's public key and make its device record.
pub fn new_device(name: &str, key: &str) -> Result<Device> {
    let bytes = B64.decode(key).context("the key isn't base64")?;
    p256::ecdsa::VerifyingKey::from_sec1_bytes(&bytes).map_err(|_| anyhow!("not a P-256 key"))?;
    let name: String = name.chars().filter(|c| !c.is_control()).take(60).collect();
    Ok(Device {
        id: hex(&Sha256::digest(&bytes)[..8]),
        name: if name.trim().is_empty() {
            "Browser".into()
        } else {
            name.trim().into()
        },
        key: key.into(),
        added: crate::trust::now(),
    })
}

pub fn add_device(vault: &dyn Vault, d: Device) -> Result<()> {
    let mut list = devices(vault);
    list.retain(|x| x.id != d.id);
    list.push(d);
    save_devices(vault, &list)
}

pub fn remove_device(vault: &dyn Vault, id: &str) -> Result<bool> {
    let mut list = devices(vault);
    let before = list.len();
    list.retain(|x| x.id != id);
    save_devices(vault, &list)?;
    Ok(list.len() != before)
}

/// Where questions wait.
fn dir() -> Result<PathBuf> {
    let d = crate::store::config_dir()?.join("approvals");
    std::fs::create_dir_all(&d)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700));
    }
    Ok(d)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn random_id() -> Result<String> {
    let mut b = [0u8; 12];
    getrandom::fill(&mut b).map_err(|e| anyhow!("no randomness: {e}"))?;
    Ok(hex(&b))
}

fn is_id(s: &str) -> bool {
    s.len() == 24 && s.bytes().all(|c| c.is_ascii_hexdigit())
}

/// Write a file whole, so a reader never sees half of it.
fn write_atomic(path: &std::path::Path, text: &str) -> Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// A typed secret, encrypted to the asking process's one-off key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealedSecret {
    /// The browser's one-off ECDH public key, SEC1, base64.
    pub epk: String,
    pub iv: String,
    pub ct: String,
}

/// An answer from the page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reply {
    pub button: String,
    pub device: String,
    /// ECDSA P-256 / SHA-256 over [`message`], raw r‖s, base64.
    pub sig: String,
    #[serde(default)]
    pub secret: Option<SealedSecret>,
}

/// What a browser signs: the question exactly as written, and its answer.
pub fn message(id: &str, request: &str, button: &str, secret: Option<&SealedSecret>) -> String {
    let req = hex(&Sha256::digest(request.as_bytes()));
    let sec = secret
        .map(|s| hex(&Sha256::digest(format!("{}.{}.{}", s.epk, s.iv, s.ct))))
        .unwrap_or_default();
    format!("keymule-approval-v1\n{id}\n{req}\n{button}\n{sec}")
}

/// Check the answer is signed by a paired browser.
pub fn verify(devices: &[Device], id: &str, request: &str, reply: &Reply) -> Result<()> {
    use p256::ecdsa::signature::Verifier;
    let d = devices
        .iter()
        .find(|d| d.id == reply.device)
        .ok_or_else(|| anyhow!("that browser isn't paired"))?;
    let key = p256::ecdsa::VerifyingKey::from_sec1_bytes(&B64.decode(&d.key)?)
        .map_err(|_| anyhow!("bad stored key"))?;
    let sig = p256::ecdsa::Signature::from_slice(&B64.decode(&reply.sig)?)
        .map_err(|_| anyhow!("bad signature"))?;
    key.verify(
        message(id, request, &reply.button, reply.secret.as_ref()).as_bytes(),
        &sig,
    )
    .map_err(|_| anyhow!("the signature doesn't match"))
}

/// What a paired browser signs to open the page without the link's token.
pub fn login_message(nonce: &str) -> String {
    format!("keymule-login-v1\n{nonce}")
}

/// Check a page login: `sig` over [`login_message`] by a paired browser.
pub fn verify_login(devices: &[Device], device: &str, nonce: &str, sig: &str) -> Result<()> {
    use p256::ecdsa::signature::Verifier;
    let d = devices
        .iter()
        .find(|d| d.id == device)
        .ok_or_else(|| anyhow!("that browser isn't paired"))?;
    let key = p256::ecdsa::VerifyingKey::from_sec1_bytes(&B64.decode(&d.key)?)
        .map_err(|_| anyhow!("bad stored key"))?;
    let sig = p256::ecdsa::Signature::from_slice(&B64.decode(sig)?)
        .map_err(|_| anyhow!("bad signature"))?;
    key.verify(login_message(nonce).as_bytes(), &sig)
        .map_err(|_| anyhow!("the signature doesn't match"))
}

/// The AES key both sides derive from the ECDH secret.
fn aes_key(id: &str, shared: &[u8]) -> [u8; 32] {
    derive("keymule-secret-v1", id, shared)
}

fn derive(domain: &str, id: &str, shared: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(format!("{domain}\n{id}\n").as_bytes());
    h.update(shared);
    h.finalize().into()
}

/// Encrypt `plain` for the browser that sent `browser_epk`, with this
/// process's one-off key `sk`. Returns (iv, ciphertext), base64.
fn seal_for_browser(
    sk: &p256::SecretKey,
    id: &str,
    browser_epk: &str,
    plain: &[u8],
) -> Result<(String, String)> {
    use aes_gcm::aead::{Aead, KeyInit};
    let epk = p256::PublicKey::from_sec1_bytes(&B64.decode(browser_epk)?)
        .map_err(|_| anyhow!("bad key from the page"))?;
    let shared = p256::ecdh::diffie_hellman(sk.to_nonzero_scalar(), epk.as_affine());
    let key = derive("keymule-show-v1", id, shared.raw_secret_bytes().as_slice());
    let cipher = aes_gcm::Aes256Gcm::new_from_slice(&key).map_err(|_| anyhow!("bad key"))?;
    let mut iv = [0u8; 12];
    getrandom::fill(&mut iv).map_err(|e| anyhow!("no randomness: {e}"))?;
    let nonce = aes_gcm::Nonce::try_from(iv.as_slice()).map_err(|_| anyhow!("bad nonce"))?;
    let ct = cipher
        .encrypt(&nonce, plain)
        .map_err(|_| anyhow!("encryption failed"))?;
    Ok((B64.encode(iv), B64.encode(ct)))
}

fn open_secret(sk: &p256::SecretKey, id: &str, s: &SealedSecret) -> Result<String> {
    use aes_gcm::aead::{Aead, KeyInit};
    let epk = p256::PublicKey::from_sec1_bytes(&B64.decode(&s.epk)?)
        .map_err(|_| anyhow!("bad key from the page"))?;
    let shared = p256::ecdh::diffie_hellman(sk.to_nonzero_scalar(), epk.as_affine());
    let key = aes_key(id, shared.raw_secret_bytes().as_slice());
    let cipher = aes_gcm::Aes256Gcm::new_from_slice(&key).map_err(|_| anyhow!("bad key"))?;
    let iv = B64.decode(&s.iv)?;
    let nonce = aes_gcm::Nonce::try_from(iv.as_slice()).map_err(|_| anyhow!("bad nonce"))?;
    let plain = cipher
        .decrypt(&nonce, B64.decode(&s.ct)?.as_slice())
        .map_err(|_| anyhow!("couldn't decrypt the secret"))?;
    String::from_utf8(plain).context("the secret isn't text")
}

/// Ask in the page and wait for a signed answer. None if nobody answered in
/// time.
pub fn ask(spec: &Spec) -> Result<Option<Picked>> {
    let Some((id, sk, r)) = ask_signed(spec)? else {
        return Ok(None);
    };
    let text = match (&sk, &r.secret) {
        (Some(k), Some(s)) if spec.secure => Some(open_secret(k, &id, s)?),
        _ => None,
    };
    Ok(Some(Picked {
        button: r.button,
        text,
    }))
}

/// How long revealed values stay readable in the page.
const SHOW_SECS: u64 = 30;

/// Show stored values in the paired browser: ask there, and on Show,
/// encrypt them to the one-off key the browser sent with its signed answer.
/// Only a `{id}.show.json` with ciphertext is written, and removed again
/// after 30 seconds. Returns false if the user didn't say Show.
pub fn show(
    spec: &Spec,
    name: &str,
    items: &[(String, String)],
    copy: Option<&str>,
) -> Result<bool> {
    let Some((id, Some(sk), r)) = ask_signed(spec)? else {
        return Ok(false);
    };
    if r.button != "Show" {
        return Ok(false);
    }
    let epk = r
        .secret
        .as_ref()
        .map(|s| s.epk.clone())
        .ok_or_else(|| anyhow!("the page didn't send a key to show it with"))?;
    let plain = serde_json::to_vec(&json!({ "name": name, "items": items, "copy": copy }))?;
    let (iv, ct) = seal_for_browser(&sk, &id, &epk, &plain)?;
    let path = dir()?.join(format!("{id}.show.json"));
    let expires = crate::trust::now() + SHOW_SECS;
    write_atomic(
        &path,
        &serde_json::to_string(&json!({ "id": id, "iv": iv, "ct": ct, "expires": expires }))?,
    )?;
    std::thread::sleep(Duration::from_secs(SHOW_SECS));
    let _ = std::fs::remove_file(&path);
    Ok(true)
}

/// Values waiting to be shown in the page, still encrypted.
pub fn shows() -> Vec<serde_json::Value> {
    let Ok(d) = dir() else { return vec![] };
    let Ok(entries) = std::fs::read_dir(&d) else {
        return vec![];
    };
    let now = crate::trust::now();
    let mut out = vec![];
    for e in entries.flatten() {
        let path = e.path();
        if !path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".show.json"))
        {
            continue;
        }
        let Some(v) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        else {
            continue;
        };
        if v["expires"].as_u64().unwrap_or(0) <= now {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        out.push(v);
    }
    out
}

/// Ask and wait for a signed answer: the question's id, this process's
/// one-off key (for typed or revealed secrets), and the answer.
fn ask_signed(spec: &Spec) -> Result<Option<(String, Option<p256::SecretKey>, Reply)>> {
    let vault = crate::store::vault();
    let devs = devices(vault.as_ref());
    if devs.is_empty() {
        bail!(
            "keymule needs your OK, and there's no screen here for a dialog. Open the keymule \
             page (run `keymule ui` in your own terminal, forward its port if this is a \
             container) and click \"Pair this browser\" with the code it prints"
        );
    }
    let id = random_id()?;
    let sk = if spec.secure || spec.reveal {
        use p256::elliptic_curve::Generate;
        Some(p256::SecretKey::generate())
    } else {
        None
    };
    let epk = sk
        .as_ref()
        .map(|k| B64.encode(k.public_key().to_sec1_bytes()));
    let now = crate::trust::now();
    let request = serde_json::to_string(&json!({
        "id": id,
        "created": now,
        "expires": now + spec.timeout as u64,
        "spec": spec,
        "epk": epk,
    }))?;
    let d = dir()?;
    let req_path = d.join(format!("{id}.req.json"));
    let ans_path = d.join(format!("{id}.ans.json"));
    let deny_path = d.join(format!("{id}.deny"));
    write_atomic(&req_path, &request)?;
    eprintln!(
        "keymule: waiting for you to answer in the keymule page (up to {} min)…",
        spec.timeout.div_ceil(60)
    );
    let deadline = Instant::now() + Duration::from_secs(spec.timeout as u64);
    let picked = loop {
        if Instant::now() >= deadline {
            break None;
        }
        if deny_path.exists() {
            crate::http::audit("-", "DENIED-IN-PAGE", &id, 0);
            break Some(Reply {
                button: spec.cancel.to_string(),
                device: String::new(),
                sig: String::new(),
                secret: None,
            });
        }
        if let Ok(text) = std::fs::read_to_string(&ans_path) {
            let _ = std::fs::remove_file(&ans_path);
            // Read the paired browsers again: one may have been paired
            // while this question waited (that's how you get unstuck).
            let devs = devices(vault.as_ref());
            let checked = serde_json::from_str::<Reply>(&text)
                .map_err(|e| anyhow!("{e}"))
                .and_then(|r| verify(&devs, &id, &request, &r).map(|_| r));
            match checked {
                Ok(r) if spec.buttons.contains(&r.button.as_str()) => {
                    crate::http::audit(
                        "-",
                        "ANSWERED-IN-PAGE",
                        &format!("{id} {} by {}", r.button, r.device),
                        0,
                    );
                    break Some(r);
                }
                Ok(_) => {}
                Err(e) => crate::http::audit("-", "BAD-APPROVAL", &format!("{id}: {e}"), 0),
            }
        }
        std::thread::sleep(Duration::from_millis(250));
    };
    let _ = std::fs::remove_file(&req_path);
    let _ = std::fs::remove_file(&deny_path);
    Ok(picked.map(|r| (id, sk, r)))
}

/// The questions waiting now, exactly as written (the page signs over
/// them). Old ones are cleared away.
pub fn pending() -> Vec<String> {
    let Ok(d) = dir() else { return vec![] };
    let now = crate::trust::now();
    let mut out: Vec<(u64, String)> = vec![];
    let Ok(entries) = std::fs::read_dir(&d) else {
        return vec![];
    };
    for e in entries.flatten() {
        let path = e.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.ends_with(".req.json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let expires = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v["expires"].as_u64())
            .unwrap_or(0);
        if expires <= now {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        out.push((expires, text));
    }
    out.sort();
    out.into_iter().map(|(_, t)| t).collect()
}

/// Hand an answer from the page to the process that asked. Checked here too,
/// so the page hears straight away when something's wrong.
pub fn answer(vault: &dyn Vault, id: &str, reply: &Reply) -> Result<()> {
    if !is_id(id) {
        bail!("bad id");
    }
    let d = dir()?;
    let request = std::fs::read_to_string(d.join(format!("{id}.req.json")))
        .map_err(|_| anyhow!("that question has already been answered or timed out"))?;
    verify(&devices(vault), id, &request, reply)?;
    write_atomic(
        &d.join(format!("{id}.ans.json")),
        &serde_json::to_string(reply)?,
    )
}

/// Say no to a question. Needs no signature: no can't give anything away,
/// and it means a browser that isn't paired is never stuck behind one.
pub fn deny(id: &str) -> Result<()> {
    if !is_id(id) {
        bail!("bad id");
    }
    let d = dir()?;
    if !d.join(format!("{id}.req.json")).exists() {
        bail!("that question has already been answered or timed out");
    }
    write_atomic(&d.join(format!("{id}.deny")), "")
}

/// Changes whenever the waiting questions or shown values change, so the
/// page can wait for news instead of asking every second.
pub fn version() -> String {
    let id = |t: &str| {
        serde_json::from_str::<serde_json::Value>(t)
            .ok()
            .and_then(|v| v["id"].as_str().map(str::to_string))
    };
    let mut ids: Vec<String> = pending().iter().filter_map(|t| id(t)).collect();
    ids.extend(
        shows()
            .iter()
            .filter_map(|v| v["id"].as_str().map(str::to_string)),
    );
    ids.join(",")
}

#[cfg(test)]
mod tests;
