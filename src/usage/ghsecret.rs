//! `keymule gh-secret`: copy stored secrets into a repo's GitHub Actions
//! secrets.
//!
//! GitHub wants each secret encrypted with the repo's public key (a libsodium
//! sealed box) before upload, so keymule fetches that key, seals the values
//! locally and uploads only ciphertext. The values never reach the agent, a
//! shell or a command line.
//!
//! Who uploads: the account the `gh` CLI is already signed in as, or a
//! GitHub credential stored in keymule. keymule asks each one who it is and
//! what it may do on the repo *before* the dialog, so the dialog names the
//! real account, and a token for the wrong account is caught up front. gh's
//! token is borrowed inside this process (`gh auth token`) and used only for
//! these api.github.com requests; it is never printed or stored. keymule
//! makes the requests itself rather than piping the values into `gh secret
//! set`, so a gh config or proxy that the agent could have changed can't
//! swap in its own repo key.
//!
//! Several secrets go in one dialog: one approval for one repo.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::LazyLock;

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use regex::Regex;

use crate::http::{self, Request};
use crate::store::{Auth, Credential, Sealed, Store, Vault};

static REPO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$").unwrap());
static SECRET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$").unwrap());

/// GitHub's rules for repo names.
fn validate_repo(repo: &str) -> Result<()> {
    if !REPO.is_match(repo) {
        bail!("repo must look like owner/name, got \"{repo}\"");
    }
    Ok(())
}

/// GitHub's rules for secret names.
fn validate_secret(secret: &str) -> Result<()> {
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
        .map_err(|_| anyhow!("GitHub sent a public key that isn't 32 bytes"))?;
    let key = crypto_box::PublicKey::from(bytes);
    let sealed = key
        .seal(&mut crypto_box::aead::OsRng, value.as_bytes())
        .map_err(|_| anyhow!("couldn't encrypt the secret"))?;
    Ok(B64.encode(sealed))
}

// ---- what to copy -----------------------------------------------------------

/// One stored credential going into one Actions secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transfer {
    /// The Actions secret, e.g. CWS_CLIENT_SECRET.
    pub secret: String,
    /// The keymule credential whose value it gets.
    pub cred: String,
}

/// `SECRET=credential …`, or one `credential` with `--as SECRET`.
pub fn parse_transfers(items: &[String], as_secret: Option<&str>) -> Result<Vec<Transfer>> {
    let list = match as_secret {
        Some(secret) => match items {
            [cred] if !cred.contains('=') => vec![Transfer {
                secret: secret.to_string(),
                cred: cred.clone(),
            }],
            _ => bail!(
                "--as names one secret for one credential. For several, drop --as and list them \
                 as SECRET=credential: keymule gh-secret --repo <owner>/<repo> \
                 API_KEY=acme-key OTHER=acme-other"
            ),
        },
        None => items
            .iter()
            .map(|item| match item.split_once('=') {
                Some((secret, cred)) if !secret.is_empty() && !cred.is_empty() => Ok(Transfer {
                    secret: secret.to_string(),
                    cred: cred.to_string(),
                }),
                _ => bail!(
                    "\"{item}\": say which Actions secret it goes into, as SECRET=credential \
                     (or one credential with --as SECRET)"
                ),
            })
            .collect::<Result<_>>()?,
    };
    if list.is_empty() {
        bail!("say what to copy: keymule gh-secret --repo <owner>/<repo> SECRET=credential …");
    }
    for (i, t) in list.iter().enumerate() {
        validate_secret(&t.secret)?;
        if list[..i].iter().any(|o| o.secret == t.secret) {
            bail!("{} is listed twice", t.secret);
        }
    }
    Ok(list)
}

// ---- who uploads ------------------------------------------------------------

/// Which login to upload with, from `--via`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Via {
    /// The gh CLI's account: the active one, or the named one (`gh:<login>`).
    Gh(Option<String>),
    /// A GitHub credential stored in keymule.
    Cred(String),
}

impl Via {
    pub fn parse(s: &str) -> Via {
        match s {
            "gh" => Via::Gh(None),
            _ => match s.strip_prefix("gh:") {
                Some(login) if !login.is_empty() => Via::Gh(Some(login.to_string())),
                _ => Via::Cred(s.to_string()),
            },
        }
    }
}

/// What a login may do on the repo, as GitHub reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Access {
    Admin,
    Write,
    Read,
    /// GitHub said 404: the repo doesn't exist or this login can't see it.
    Hidden,
    /// GitHub didn't say (some app tokens). Worth a try.
    Unknown,
    /// The token didn't work (HTTP status).
    Rejected(u16),
    /// keymule couldn't use this login at all (gh missing, signed out…).
    Unavailable(String),
}

impl Access {
    /// From `GET /repos/{repo}`'s `permissions`.
    pub fn from_repo(json: &serde_json::Value) -> Access {
        let p = &json["permissions"];
        if !p.is_object() {
            return Access::Unknown;
        }
        let yes = |k: &str| p[k].as_bool().unwrap_or(false);
        if yes("admin") {
            Access::Admin
        } else if yes("push") || yes("maintain") {
            Access::Write
        } else {
            Access::Read
        }
    }

    fn usable(&self) -> bool {
        matches!(self, Access::Admin | Access::Unknown)
    }
}

/// A login keymule could upload with, and what it found out about it.
pub struct Uploader {
    /// How the user names it: "gh", "gh:<login>", or a credential name.
    pub source: String,
    /// The account GitHub says the token belongs to.
    pub login: Option<String>,
    pub access: Access,
    /// For gh, the program keymule asked.
    pub program: Option<PathBuf>,
    cred: Credential,
    token: String,
}

impl Uploader {
    fn is_gh(&self) -> bool {
        self.program.is_some()
    }

    /// "your gh login" or "keymule credential \"x\"".
    pub fn how(&self) -> String {
        match &self.program {
            Some(p) => format!("your gh login ({})", crate::cliexec::tilde(p)),
            None => format!("the keymule credential \"{}\"", self.source),
        }
    }

    /// One line for errors and --dry-run.
    pub fn describe(&self, repo: &str) -> String {
        let who = match &self.login {
            Some(l) => format!("signed in as {l}"),
            None => "account unknown".into(),
        };
        let what = match &self.access {
            Access::Admin => format!("admin on {repo}"),
            Access::Write => format!("write but not admin on {repo} (Actions secrets need admin)"),
            Access::Read => format!("read-only on {repo} (Actions secrets need admin)"),
            Access::Hidden => format!("can't see {repo} (GitHub says 404)"),
            Access::Unknown => format!("GitHub didn't say what it may do on {repo}"),
            Access::Rejected(s) => format!("GitHub rejected its token (HTTP {s})"),
            Access::Unavailable(why) => return format!("{}: {why}", self.source),
        };
        format!("{}: {who}, {what}", self.source)
    }
}

fn github_cred() -> Credential {
    Credential {
        service: "github".into(),
        url: "https://api.github.com".into(),
        description: String::new(),
        auth: Auth::Bearer,
        hosts: vec!["api.github.com".into()],
        added_by: String::new(),
    }
}

/// Env vars that would make gh hand over some other token or host.
const GH_ENV: &[&str] = &[
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "GH_ENTERPRISE_TOKEN",
    "GITHUB_ENTERPRISE_TOKEN",
    "GH_HOST",
];

/// The token gh uses for github.com (the active account, or `user`). Kept
/// in this process; never printed.
fn gh_token(user: Option<&str>) -> Result<(PathBuf, String)> {
    let found = crate::servers::which("gh").context("the gh CLI isn't installed (not on PATH)")?;
    let program = found.canonicalize().unwrap_or_else(|_| found.clone());
    let mut cmd = Command::new(&program);
    cmd.args(["auth", "token", "--hostname", "github.com"]);
    if let Some(u) = user {
        cmd.args(["--user", u]);
    }
    for var in GH_ENV.iter().chain(crate::servers::STRIP_ENV) {
        cmd.env_remove(var);
    }
    let out = cmd
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("running {}", program.display()))?;
    let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || token.is_empty() || token.contains(char::is_whitespace) {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = http::redact(err.trim(), &token);
        bail!(
            "gh isn't signed in to github.com{} ({}). Run `gh auth login` in a terminal.",
            user.map(|u| format!(" as {u}")).unwrap_or_default(),
            if err.is_empty() { "no token" } else { &err }
        );
    }
    Ok((found, token)) // the PATH entry, which reads better in the dialog than the Cellar path
}

async fn get(source: &str, cred: &Credential, token: &str, path: &str) -> Result<http::Response> {
    http::send(
        source,
        cred,
        token,
        &Request {
            method: "GET".into(),
            path: path.into(),
            max_bytes: 200_000,
            ..Default::default()
        },
    )
    .await
}

/// Ask GitHub who this token is and what it may do on `repo`.
async fn probe(
    source: String,
    program: Option<PathBuf>,
    cred: Credential,
    token: String,
    repo: &str,
) -> Uploader {
    let mut up = Uploader {
        source,
        login: None,
        access: Access::Unknown,
        program,
        cred,
        token,
    };
    match get(&up.source, &up.cred, &up.token, "/user").await {
        Ok(r) if r.status.is_success() => {
            up.login = serde_json::from_str::<serde_json::Value>(&r.body)
                .ok()
                .and_then(|j| j["login"].as_str().map(String::from));
        }
        Ok(r) if r.status.as_u16() == 401 => {
            up.access = Access::Rejected(401);
            return up;
        }
        Ok(_) => {} // app tokens can't read /user; the repo check still says enough
        Err(e) => {
            up.access = Access::Unavailable(format!("couldn't reach GitHub: {e:#}"));
            return up;
        }
    }
    up.access = match get(&up.source, &up.cred, &up.token, &format!("/repos/{repo}")).await {
        Ok(r) if r.status.is_success() => serde_json::from_str(&r.body)
            .map(|j| Access::from_repo(&j))
            .unwrap_or(Access::Unknown),
        Ok(r) if r.status.as_u16() == 404 => Access::Hidden,
        Ok(r) => Access::Rejected(r.status.as_u16()),
        Err(e) => Access::Unavailable(format!("couldn't reach GitHub: {e:#}")),
    };
    up
}

fn unavailable(source: &str, program: Option<PathBuf>, why: String) -> Uploader {
    Uploader {
        source: source.into(),
        login: None,
        access: Access::Unavailable(why),
        program,
        cred: github_cred(),
        token: String::new(),
    }
}

async fn probe_gh(user: Option<&str>, repo: &str) -> Uploader {
    let source = match user {
        Some(u) => format!("gh:{u}"),
        None => "gh".into(),
    };
    match gh_token(user) {
        Ok((program, token)) => probe(source, Some(program), github_cred(), token, repo).await,
        Err(e) => unavailable(&source, None, format!("{e:#}")),
    }
}

async fn probe_cred(vault: &dyn Vault, store: &Store, name: &str, repo: &str) -> Uploader {
    let loaded = store
        .get(name)
        .and_then(|c| Sealed::load(vault, name, c).map(|s| (s.apply(c), s.secret)));
    match loaded {
        Ok((cred, token)) => probe(name.into(), None, cred, token, repo).await,
        Err(e) => unavailable(name, None, format!("{e:#}")),
    }
}

/// Every login keymule could use, the one asked for (if any) first.
async fn candidates(
    vault: &dyn Vault,
    store: &Store,
    via: Option<&Via>,
    repo: &str,
) -> Result<Vec<Uploader>> {
    let mut out = vec![];
    match via {
        Some(Via::Cred(name)) => {
            let c = store.get(name)?;
            if c.service != "github" {
                bail!(
                    "\"{name}\" is a {} credential, not a GitHub one. Use --via gh for the gh \
                     CLI's login, or --via <a GitHub credential>",
                    c.service
                );
            }
            out.push(probe_cred(vault, store, name, repo).await);
        }
        Some(Via::Gh(user)) => out.push(probe_gh(user.as_deref(), repo).await),
        None => {}
    }
    if !matches!(via, Some(Via::Gh(None))) {
        let gh = probe_gh(None, repo).await;
        let done = via.is_none() && gh.access == Access::Admin;
        out.push(gh);
        if done {
            return Ok(out); // the usual case; no need to try stored tokens too
        }
    }
    for (name, c) in &store.creds {
        if c.service == "github" && !matches!(via, Some(Via::Cred(v)) if v == name) {
            out.push(probe_cred(vault, store, name, repo).await);
        }
    }
    Ok(out)
}

/// The login to upload with: the one asked for, else the first that may
/// (the gh login before stored credentials; admin before "didn't say").
pub fn choose(mut cands: Vec<Uploader>, asked: bool, repo: &str) -> Result<Uploader> {
    let pick = if asked {
        cands
            .first()
            .is_some_and(|c| c.access.usable())
            .then_some(0)
    } else {
        cands
            .iter()
            .position(|c| c.access == Access::Admin)
            .or_else(|| cands.iter().position(|c| c.access.usable()))
    };
    if let Some(i) = pick {
        return Ok(cands.swap_remove(i));
    }
    let lines: Vec<String> = cands
        .iter()
        .map(|c| format!("  {}", c.describe(repo)))
        .collect();
    let other = cands.iter().skip(1).find(|c| asked && c.access.usable());
    let hint = match other {
        Some(o) => format!("\n\n{} can: use --via {}.", o.source, o.source),
        None => format!(
            "\n\nSign gh in as an account with admin on {repo} (`gh auth login` in a terminal; \
             `--via gh:<login>` picks one of several), or add a GitHub token with admin there \
             (add_credential)."
        ),
    };
    bail!(
        "nothing keymule can use may set Actions secrets on {repo}:\n{}{hint}",
        lines.join("\n")
    )
}

// ---- the copy ---------------------------------------------------------------

/// Names of the repo's Actions secrets.
async fn secret_names(up: &Uploader, repo: &str) -> Result<Vec<String>> {
    let r = get(
        &up.source,
        &up.cred,
        &up.token,
        &format!("/repos/{repo}/actions/secrets?per_page=100"),
    )
    .await?;
    if !r.status.is_success() {
        bail!(
            "GitHub wouldn't list {repo}'s secrets (HTTP {})",
            r.status.as_u16()
        );
    }
    let j: serde_json::Value =
        serde_json::from_str(&r.body).context("unexpected reply from GitHub")?;
    Ok(j["secrets"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| s["name"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default())
}

pub fn question(transfers: &[Transfer], repo: &str, up: &Uploader, existing: &[String]) -> String {
    let n = transfers.len();
    let what = if n == 1 {
        "a secret".to_string()
    } else {
        format!("{n} secrets")
    };
    let lines: Vec<String> = transfers
        .iter()
        .map(|t| {
            let replaces = if existing.contains(&t.secret) {
                "  (replaces)"
            } else {
                ""
            };
            format!("        {}  ←  {}{replaces}", t.secret, t.cred)
        })
        .collect();
    let who = match &up.login {
        Some(l) => format!("as {l}"),
        None => "as an account GitHub didn't name".into(),
    };
    let access = if up.access == Access::Admin {
        " (admin there)"
    } else {
        ""
    };
    format!(
        "Copy {what} into {repo}'s GitHub Actions?\n\n{}\n\n\
         Uploaded {who}{access}, using {}. Encrypted; the agent never sees {}.",
        lines.join("\n"),
        up.how(),
        if n == 1 { "it" } else { "them" },
    )
}

/// Copy each transfer's credential into `repo`'s Actions secrets, after the
/// user agrees to all of them in one dialog. With `dry_run`, only say what
/// would happen.
pub async fn push(
    vault: &dyn Vault,
    store: &Store,
    transfers: &[Transfer],
    repo: &str,
    via: Option<&Via>,
    dry_run: bool,
    confirm: &dyn Fn(&str) -> Result<bool>,
) -> Result<String> {
    validate_repo(repo)?;
    let mut sources = vec![];
    for t in transfers {
        validate_secret(&t.secret)?;
        if matches!(via, Some(Via::Cred(v)) if v == &t.cred) {
            bail!(
                "\"{}\" is the GitHub credential uploading; pick another to copy",
                t.cred
            );
        }
        sources.push(Sealed::load(vault, &t.cred, store.get(&t.cred)?)?);
    }
    let up = choose(
        candidates(vault, store, via, repo).await?,
        via.is_some(),
        repo,
    )?;
    let existing = secret_names(&up, repo).await.unwrap_or_default();

    if dry_run {
        let mut out = format!("Would upload with {}\n", up.describe(repo));
        for t in transfers {
            let how = if existing.contains(&t.secret) {
                "replace"
            } else {
                "create"
            };
            out.push_str(&format!("  {how} {} from \"{}\"\n", t.secret, t.cred));
        }
        out.push_str("Nothing was asked or changed (--dry-run).");
        return Ok(out);
    }

    let allowed = confirm(&question(transfers, repo, &up, &existing))
        .context("keymule needs to ask you first, but can't show a dialog here")?;
    if !allowed {
        bail!("the user didn't allow it; nothing changed");
    }

    let key_res = get(
        &up.source,
        &up.cred,
        &up.token,
        &format!("/repos/{repo}/actions/secrets/public-key"),
    )
    .await?;
    if !key_res.status.is_success() {
        bail!(
            "GitHub wouldn't give {repo}'s key (HTTP {}), so nothing was copied. {} needs admin \
             on it (a fine-grained token also needs \"Secrets: write\").",
            key_res.status.as_u16(),
            up.source
        );
    }
    let key: serde_json::Value =
        serde_json::from_str(&key_res.body).context("unexpected reply from GitHub")?;
    let (Some(key_id), Some(public_key)) = (key["key_id"].as_str(), key["key"].as_str()) else {
        bail!("GitHub's reply had no key_id/key");
    };

    let via_label = match &up.login {
        Some(l) if up.is_gh() => format!("gh:{l}"),
        _ => up.source.clone(),
    };
    let mut done = vec![];
    for (t, source) in transfers.iter().zip(&sources) {
        let put = http::send(
            &up.source,
            &up.cred,
            &up.token,
            &Request {
                method: "PUT".into(),
                path: format!("/repos/{repo}/actions/secrets/{}", t.secret),
                body: Some(serde_json::json!({
                    "encrypted_value": seal(public_key, &source.secret)?,
                    "key_id": key_id,
                })),
                max_bytes: 10_000,
                ..Default::default()
            },
        )
        .await;
        let status = put.as_ref().map(|r| r.status.as_u16()).unwrap_or(0);
        http::audit(
            &t.cred,
            "GH-SECRET",
            &format!("https://github.com/{repo} {} via {via_label}", t.secret),
            status,
        );
        match status {
            201 => done.push(format!("created {} from \"{}\"", t.secret, t.cred)),
            204 => done.push(format!("updated {} from \"{}\"", t.secret, t.cred)),
            _ => {
                let why = match put {
                    Ok(r) => format!("HTTP {status}: {}", r.body),
                    Err(e) => format!("{e:#}"),
                };
                let so_far = if done.is_empty() {
                    "Nothing was copied.".to_string()
                } else {
                    format!("Already done: {}.", done.join(", "))
                };
                bail!("GitHub refused {} ({why}). {so_far}", t.secret);
            }
        }
    }

    let mut out = format!(
        "In {repo}, as {}: {}.",
        up.login.as_deref().unwrap_or(&up.source),
        done.join(", ")
    );
    match secret_names(&up, repo).await {
        Ok(names) => {
            let missing: Vec<&str> = transfers
                .iter()
                .filter(|t| !names.contains(&t.secret))
                .map(|t| t.secret.as_str())
                .collect();
            if missing.is_empty() {
                out.push_str(" GitHub now lists all of them.");
            } else {
                out.push_str(&format!(
                    " But GitHub doesn't list {} yet; check the repo's settings.",
                    missing.join(", ")
                ));
            }
        }
        Err(e) => out.push_str(&format!(" (Couldn't check the list afterwards: {e:#}.)")),
    }
    Ok(out)
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
        assert!(validate_repo("acme/app").is_ok() && validate_secret("STRIPE_SECRET_KEY").is_ok());
        assert!(validate_repo("acme").is_err());
        assert!(validate_secret("1BAD").is_err());
        assert!(validate_secret("github_token").is_err());
        assert!(validate_secret("has-dash").is_err());
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parses_one_or_many_transfers() {
        let one = parse_transfers(&s(&["stripe-acme"]), Some("STRIPE_SECRET_KEY")).unwrap();
        assert_eq!(
            one,
            vec![Transfer {
                secret: "STRIPE_SECRET_KEY".into(),
                cred: "stripe-acme".into()
            }]
        );
        let many = parse_transfers(
            &s(&["CWS_CLIENT_ID=cws-id", "CWS_CLIENT_SECRET=cws-secret"]),
            None,
        )
        .unwrap();
        assert_eq!(many.len(), 2);
        assert_eq!(many[1].secret, "CWS_CLIENT_SECRET");
        assert_eq!(many[1].cred, "cws-secret");

        for (items, as_) in [
            (s(&["a", "b"]), Some("X")),
            (s(&["X=a"]), Some("X")),
            (s(&["no-equals"]), None),
            (s(&["=a"]), None),
            (s(&["X="]), None),
            (s(&["X=a", "X=b"]), None),
            (s(&["bad-name=a"]), None),
            (s(&[]), None),
        ] {
            assert!(parse_transfers(&items, as_).is_err(), "{items:?} {as_:?}");
        }
    }

    #[test]
    fn via_names_gh_or_a_credential() {
        assert_eq!(Via::parse("gh"), Via::Gh(None));
        assert_eq!(Via::parse("gh:octo"), Via::Gh(Some("octo".into())));
        assert_eq!(Via::parse("github-acme"), Via::Cred("github-acme".into()));
        assert_eq!(Via::parse("gh:"), Via::Cred("gh:".into()));
    }

    #[test]
    fn reads_repo_permissions() {
        let j = |v: serde_json::Value| Access::from_repo(&serde_json::json!({ "permissions": v }));
        assert_eq!(
            j(serde_json::json!({"admin": true, "push": true})),
            Access::Admin
        );
        assert_eq!(
            j(serde_json::json!({"admin": false, "push": true})),
            Access::Write
        );
        assert_eq!(
            j(serde_json::json!({"admin": false, "pull": true})),
            Access::Read
        );
        assert_eq!(Access::from_repo(&serde_json::json!({})), Access::Unknown);
    }

    fn up(source: &str, gh: bool, login: &str, access: Access) -> Uploader {
        Uploader {
            source: source.into(),
            login: Some(login.into()),
            access,
            program: gh.then(|| PathBuf::from("/opt/homebrew/bin/gh")),
            cred: github_cred(),
            token: "t".into(),
        }
    }

    const REPO: &str = "octo/ext";

    #[test]
    fn picks_an_admin_login_and_explains_when_there_is_none() {
        // The session this was built from: gh is the owner, the stored
        // token belongs to another account.
        let got = choose(
            vec![
                up("gh", true, "octo", Access::Admin),
                up("github-work", false, "workbot", Access::Hidden),
            ],
            false,
            REPO,
        )
        .unwrap();
        assert_eq!(got.source, "gh");

        // Admin wins over "didn't say", wherever it is.
        let got = choose(
            vec![
                up("gh", true, "octo", Access::Unknown),
                up("github-work", false, "workbot", Access::Admin),
            ],
            false,
            REPO,
        )
        .unwrap();
        assert_eq!(got.source, "github-work");

        let err = choose(
            vec![
                up("gh", true, "octo", Access::Write),
                up("github-work", false, "workbot", Access::Hidden),
            ],
            false,
            REPO,
        )
        .err()
        .unwrap()
        .to_string();
        assert!(
            err.contains("gh: signed in as octo, write but not admin on octo/ext"),
            "{err}"
        );
        assert!(
            err.contains("github-work: signed in as workbot, can't see octo/ext"),
            "{err}"
        );
        assert!(err.contains("gh auth login"), "{err}");
    }

    #[test]
    fn a_login_asked_for_must_work_itself_and_the_error_names_one_that_does() {
        let err = choose(
            vec![
                up("github-work", false, "workbot", Access::Hidden),
                up("gh", true, "octo", Access::Admin),
            ],
            true,
            REPO,
        )
        .err()
        .unwrap()
        .to_string();
        assert!(err.contains("can't see octo/ext"), "{err}");
        assert!(err.contains("gh can: use --via gh"), "{err}");

        let got = choose(
            vec![
                up("github-work", false, "workbot", Access::Admin),
                up("gh", true, "octo", Access::Admin),
            ],
            true,
            REPO,
        )
        .unwrap();
        assert_eq!(got.source, "github-work");
    }

    #[test]
    fn one_dialog_lists_every_secret_and_the_real_account() {
        let t = parse_transfers(
            &s(&[
                "CWS_CLIENT_ID=cws-id",
                "CWS_CLIENT_SECRET=cws-secret",
                "CWS_REFRESH_TOKEN=cws-refresh",
            ]),
            None,
        )
        .unwrap();
        let q = question(
            &t,
            REPO,
            &up("gh", true, "octo", Access::Admin),
            &["CWS_REFRESH_TOKEN".into()],
        );
        assert!(
            q.starts_with("Copy 3 secrets into octo/ext's GitHub Actions?"),
            "{q}"
        );
        assert!(q.contains("        CWS_CLIENT_ID  ←  cws-id\n"), "{q}");
        assert!(
            q.contains("CWS_REFRESH_TOKEN  ←  cws-refresh  (replaces)"),
            "{q}"
        );
        assert!(
            q.contains("Uploaded as octo (admin there), using your gh login"),
            "{q}"
        );
        assert!(!q.contains(" t "), "no token in the dialog");
    }
}
