//! Asking the user: the secret dialogs, and checking what they typed.

use super::*;

pub(super) async fn ask_secret(message: String) -> anyhow::Result<String> {
    ask_secret_with(message, false).await
}

pub(super) async fn ask_secret_with(message: String, multiline: bool) -> anyhow::Result<String> {
    match tokio::task::spawn_blocking(move || dialog::secret_with(&message, multiline)).await?? {
        Answer::Text(s) => Ok(s),
        Answer::Cancelled => bail!("The user cancelled the dialog. Nothing was saved."),
        Answer::TimedOut => bail!("The dialog timed out after 3 minutes. Nothing was saved."),
    }
}

pub(super) async fn ask_confirm(message: String) -> anyhow::Result<bool> {
    tokio::task::spawn_blocking(move || crate::trust::confirm(&message)).await?
}

/// Verify before saving. Returns a note for the result message.
pub(super) async fn check(
    name: &str,
    cred: &Credential,
    secret: &str,
    verify: bool,
) -> anyhow::Result<String> {
    if !verify || cred.url.is_empty() {
        return Ok(" (not verified)".into());
    }
    match http::verify(name, cred, secret).await {
        Ok(Some(who)) => Ok(format!(", verified as {who}")),
        Ok(None) => Ok(" (this service has no verify endpoint)".into()),
        Err(e) => bail!(
            "The token did not work, so it was NOT saved: {}. Ask the user to check the token and \
             URL and try again (verify=false skips the check, e.g. when the service is only \
             reachable on VPN).",
            http::redact(&format!("{e:#}"), secret)
        ),
    }
}

/// Also used by `keymule add --dialog`.
/// What to ask for, and whether the field takes several lines, for a
/// secret that isn't an HTTP token.
pub(super) fn other_kind_prompt(
    kind: presets::Kind,
    name: &str,
    ssh: Option<&SshTarget>,
) -> (String, bool) {
    if let Some(t) = ssh {
        return match t.auth {
            SshAuth::Password => (
                format!(
                    "Type the SSH password for {}\n\n\"{name}\": keymule only uses it to log in \
                     to that server. Your agent never sees it.",
                    t.label()
                ),
                false,
            ),
            SshAuth::Key => (
                format!(
                    "Paste the private key for {}\n\n\"{name}\": the whole block, from -----BEGIN \
                     to -----END. keymule only uses it to log in to that server.",
                    t.label()
                ),
                true,
            ),
            SshAuth::KeyFile => (
                format!(
                    "Type the password for {}\n\n\"{name}\" logs in with {}; keymule keeps \
                     the password with it (e.g. for the provider's console). Your agent never sees it.",
                    t.label(),
                    t.key_file_label().unwrap_or_default()
                ),
                false,
            ),
        };
    }
    match kind {
        presets::Kind::PrivateKey => (
            format!(
                "Paste the private key for \"{name}\"\n\nThe whole block, from -----BEGIN to -----END. \
                 It's only ever handed to programs you approve, as a file only you can read."
            ),
            true,
        ),
        presets::Kind::AwsKeys => (
            format!(
                "Paste the AWS keys for \"{name}\"\n\nThe access key ID on the first line, the \
                 secret access key on the second."
            ),
            true,
        ),
        presets::Kind::Login => (
            format!(
                "Type the new password for \"{name}\"\n\nkeymule only fills it into that site's \
                 sign-in page, and asks you each time."
            ),
            false,
        ),
        _ => (
            format!(
                "Type the password for \"{name}\"\n\nIt's only ever handed to programs you approve."
            ),
            false,
        ),
    }
}

/// Check a pasted private key or AWS key pair looks right; returns it tidied.
pub fn check_other_kind(kind: presets::Kind, secret: &str) -> anyhow::Result<String> {
    let secret = secret.trim().replace("\r\n", "\n");
    if secret.is_empty() {
        bail!("nothing was typed; nothing was saved");
    }
    match kind {
        presets::Kind::PrivateKey => {
            if !(secret.starts_with("-----BEGIN") && secret.contains("PRIVATE KEY-----")) {
                bail!(
                    "that doesn't look like a private key (it should start with -----BEGIN … PRIVATE KEY-----)"
                );
            }
            Ok(secret)
        }
        presets::Kind::AwsKeys => {
            let lines: Vec<&str> = secret
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect();
            match lines.as_slice() {
                [id, key]
                    if (id.starts_with("AKIA") || id.starts_with("ASIA")) && key.len() >= 30 =>
                {
                    Ok(format!("{id}\n{key}"))
                }
                _ => bail!(
                    "paste the access key ID (AKIA…) on the first line and the secret access key on the second"
                ),
            }
        }
        _ => Ok(secret),
    }
}
