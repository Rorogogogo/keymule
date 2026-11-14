//! `keymule gh-secret`: copy a stored secret into a GitHub Actions secret.
//!
//! GitHub wants each secret encrypted with the repo's public key (a libsodium
//! sealed box) before upload, so keymule fetches that key with a stored GitHub
//! credential, seals the value locally and uploads only the ciphertext. The
//! value never reaches the agent, a shell or a command line.

use std::sync::LazyLock;

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use regex::Regex;

use crate::http::{self, Request};
use crate::store::{Credential, Sealed, Store, Vault};

static REPO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$").unwrap());
static SECRET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$").unwrap());

/// GitHub's rules for repo and secret names.
pub fn validate(repo: &str, secret: &str) -> Result<()> {
    if !REPO.is_match(repo) {
        bail!("repo must look like owner/name, got \"{repo}\"");
    }
    if !SECRET.is_match(secret) {
        bail!("secret names use letters, digits and _ and can't start with a digit: \"{secret}\"");
    }
    if secret.to_ascii_uppercase().starts_with("GITHUB_") {
        bail!("GitHub reserves secret names starting with GITHUB_");
    }
    Ok(())
}

/// Encrypt `value` for a repo whose public key is `public_key_b64`.
pub fn seal(public_key_b64: &str, value: &str) -> Result<String> {
    let raw = B64
        .decode(public_key_b64.trim())
        .context("GitHub sent a public key that isn't base64")?;
    let bytes: [u8; 32] = raw
        .try_into()
        .map_err(|_| anyhow::anyhow!("GitHub sent a public key that isn't 32 bytes"))?;
    let key = crypto_box::PublicKey::from(bytes);
    let sealed = key
        .seal(&mut crypto_box::aead::OsRng, value.as_bytes())
        .map_err(|_| anyhow::anyhow!("couldn't encrypt the secret"))?;
    Ok(B64.encode(sealed))
}

/// The one GitHub credential to use: `via` if given, else the only one.
pub fn github_credential<'a>(store: &'a Store, via: Option<&'a str>) -> Result<&'a str> {
    if let Some(v) = via {
        let c = store.get(v)?;
        if c.service != "github" {
            bail!("\"{v}\" is a {} credential, not a GitHub one", c.service);
        }
        return Ok(v);
    }
    let mut gh = store
        .creds
        .iter()
        .filter(|(_, c)| c.service == "github")
        .map(|(n, _)| n.as_str());
    match (gh.next(), gh.next()) {
        (Some(one), None) => Ok(one),
        (None, _) => bail!("no GitHub credential stored; add one first (keymule add)"),
        (Some(_), Some(_)) => bail!("more than one GitHub credential: say which with --via <name>"),
    }
}

pub fn question(name: &str, hosts: &[String], repo: &str, secret: &str, via: &str) -> String {
    let saved_for = if hosts.is_empty() {
        String::new()
    } else {
        format!(" (saved for {})", hosts.join(", "))
    };
    format!(
        "Copy \"{name}\"{saved_for} into the GitHub Actions secret {secret} of {repo}?\n\n\
         keymule encrypts it with the repo's key and uploads it using \"{via}\". Anyone who can \
         change that repo's workflows can then use it, and an existing {secret} is replaced.\n\n\
         keymule does not keep this copy in step: after rotating \"{name}\", run this again."
    )
}

/// Copy `name` into `repo`'s Actions secret `secret`, after the user agrees.
pub async fn push(
    vault: &dyn Vault,
    store: &Store,
    name: &str,
    repo: &str,
    secret: &str,
    via: Option<&str>,
    confirm: &dyn Fn(&str) -> Result<bool>,
) -> Result<String> {
    validate(repo, secret)?;
    let source = Sealed::load(vault, name, store.get(name)?)?;
    let via = github_credential(store, via)?;
    if via == name {
        bail!("\"{name}\" is the GitHub credential itself; pick another secret to copy");
    }
    let gh_sealed = Sealed::load(vault, via, store.get(via)?)?;
    let gh: Credential = gh_sealed.apply(store.get(via)?);

    let allowed = confirm(&question(name, &source.hosts, repo, secret, via))
        .context("keymule needs to ask you first, but can't show a dialog here")?;
    if !allowed {
        bail!("the user didn't allow it; nothing changed");
    }

    let key_res = http::send(
        via,
        &gh,
        &gh_sealed.secret,
        &Request {
            method: "GET".into(),
            path: format!("/repos/{repo}/actions/secrets/public-key"),
            max_bytes: 10_000,
            ..Default::default()
        },
    )
    .await?;
    if !key_res.status.is_success() {
        bail!(
            "GitHub wouldn't give the repo's key (HTTP {}). \"{via}\" needs admin on {repo}.\n{}",
            key_res.status.as_u16(),
            key_res.body
        );
    }
    let key: serde_json::Value =
        serde_json::from_str(&key_res.body).context("unexpected reply from GitHub")?;
    let (Some(key_id), Some(public_key)) = (key["key_id"].as_str(), key["key"].as_str()) else {
        bail!("GitHub's reply had no key_id/key");
    };
    let encrypted = seal(public_key, &source.secret)?;

    let put = http::send(
        via,
        &gh,
        &gh_sealed.secret,
        &Request {
            method: "PUT".into(),
            path: format!("/repos/{repo}/actions/secrets/{secret}"),
            body: Some(serde_json::json!({ "encrypted_value": encrypted, "key_id": key_id })),
            max_bytes: 10_000,
            ..Default::default()
        },
    )
    .await?;
    http::audit(
        name,
        "GH-SECRET",
        &format!("https://github.com/{repo} {secret} via {via}"),
        put.status.as_u16(),
    );
    match put.status.as_u16() {
        201 => Ok(format!(
            "Created the Actions secret {secret} in {repo} from \"{name}\"."
        )),
        204 => Ok(format!(
            "Updated the Actions secret {secret} in {repo} from \"{name}\"."
        )),
        s => bail!("GitHub refused the secret (HTTP {s}).\n{}", put.body),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_value_opens_with_the_repo_key_only() {
        let sk = crypto_box::SecretKey::generate(&mut crypto_box::aead::OsRng);
        let pk = B64.encode(sk.public_key().as_bytes());
        let ct = seal(&pk, "sk_live_example_value").unwrap();
        let opened = sk.unseal(&B64.decode(ct).unwrap()).unwrap();
        assert_eq!(opened, b"sk_live_example_value");
        let other = crypto_box::SecretKey::generate(&mut crypto_box::aead::OsRng);
        assert!(
            other
                .unseal(&B64.decode(seal(&pk, "x").unwrap()).unwrap())
                .is_err()
        );
    }

    #[test]
    fn rejects_bad_keys_and_names() {
        assert!(seal("not base64!", "x").is_err());
        assert!(seal(&B64.encode([0u8; 16]), "x").is_err());
        assert!(validate("acme/app", "STRIPE_SECRET_KEY").is_ok());
        assert!(validate("acme", "X").is_err());
        assert!(validate("acme/app", "1BAD").is_err());
        assert!(validate("acme/app", "github_token").is_err());
        assert!(validate("acme/app", "has-dash").is_err());
    }

    #[test]
    fn picks_the_only_github_credential_or_asks() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::load_from(dir.path().join("credentials.toml")).unwrap();
        let cred = |svc: &str| Credential {
            service: svc.into(),
            url: String::new(),
            description: String::new(),
            auth: crate::store::Auth::Bearer,
            hosts: vec![],
            added_by: String::new(),
        };
        assert!(github_credential(&s, None).is_err());
        s.creds.insert("stripe-acme".into(), cred("stripe"));
        s.creds.insert("github-acme".into(), cred("github"));
        assert_eq!(github_credential(&s, None).unwrap(), "github-acme");
        assert!(github_credential(&s, Some("stripe-acme")).is_err());
        s.creds.insert("github-work".into(), cred("github"));
        assert!(github_credential(&s, None).is_err());
        assert_eq!(
            github_credential(&s, Some("github-work")).unwrap(),
            "github-work"
        );
    }
}
