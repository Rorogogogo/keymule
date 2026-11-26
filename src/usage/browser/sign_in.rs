//! sign_in: filling a website login into the tab on its site.

use super::*;

/// Fills the login form in the page. It checks the page's origin first,
/// every time, and returns {status}: "submitted", "username" (filled the
/// username of a two-step form), "wait" (nothing to fill yet), "nofield",
/// or "origin" (with `at`: the page is somewhere else).
pub(super) const FILL: &str = r#"async ({ origin, username, password, phase }) => {
  if (location.origin !== origin) return { status: 'origin', at: location.origin };
  const visible = (el) => {
    if (!el || el.disabled || el.readOnly) return false;
    const r = el.getBoundingClientRect(), s = getComputedStyle(el);
    return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.display !== 'none';
  };
  const all = (root, sel) => [...root.querySelectorAll(sel)].filter(visible);
  const set = (el, v) => {
    el.focus();
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, v);
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
  };
  const pws = all(document, 'input[type="password"]');
  const pw = pws.find((e) => !/new-password/.test(e.autocomplete)) || pws[0];
  const scope = (pw && pw.form) || document;
  const textish = (e) => ['text', 'email', 'tel'].includes(e.type);
  let user = all(scope, 'input[autocomplete~="username"], input[autocomplete~="email"], input[type="email"], input[name*="user" i], input[name*="email" i], input[name*="login" i], input[id*="user" i], input[id*="email" i], input[id*="login" i]').filter(textish)[0];
  if (!user && pw) {
    user = all(scope, 'input').filter(textish)
      .filter((e) => e.compareDocumentPosition(pw) & Node.DOCUMENT_POSITION_FOLLOWING).pop();
  }
  if (!pw && (phase > 0 || !user)) return { status: phase > 0 ? 'wait' : 'nofield' };
  if (user && user.value !== username) set(user, username);
  if (pw) set(pw, password);
  const field = pw || user;
  const form = field.form;
  const root = form || document;
  const button = all(root, 'button[type="submit"], input[type="submit"]')[0]
    || all(root, 'button, [role="button"]').find((b) => /^\s*(sign in|log in|login|continue|next|submit)\s*$/i.test(b.textContent || b.value || ''));
  await new Promise((r) => setTimeout(r, 200));
  if (location.origin !== origin) return { status: 'origin', at: location.origin };
  // Gone after a page load: AFTER tells a reloaded sign-in page (a refusal)
  // from the same page still waiting.
  if (pw) window.__keymuleSubmitted = true;
  if (button) button.click();
  else if (form && form.requestSubmit) form.requestSubmit();
  else field.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', code: 'Enter', keyCode: 13, bubbles: true }));
  return { status: pw ? 'submitted' : 'username' };
}"#;

/// After submitting: where the tab is, whether a sign-in form is still
/// showing (`form`: a visible password field, filled or not), whether the
/// page loaded anew since (`fresh`), whether it has finished loading
/// (`ready`: mid-redirect a page is blank, which isn't a verdict), and
/// whether a field still holds a value
/// (clearing it with `clear`, so the password doesn't sit in the page).
pub(super) const AFTER: &str = r#"({ clear }) => {
  const all = [...document.querySelectorAll('input[type="password"]')];
  const held = all.filter((e) => e.value);
  if (clear) held.forEach((e) => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(e, '');
    e.dispatchEvent(new Event('input', { bubbles: true }));
  });
  const form = all.some((e) => {
    const r = e.getBoundingClientRect(), s = getComputedStyle(e);
    return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.display !== 'none';
  });
  return {
    url: location.href, held: held.length > 0, form, fresh: !window.__keymuleSubmitted,
    ready: document.readyState === 'complete',
  };
}"#;

/// What `sign_in` needs from a stored website login.
pub struct Login {
    pub origin: String,
    pub username: String,
    pub password: String,
}

/// The website login `name`, with its site and username as sealed.
pub fn load_login(vault: &dyn Vault, name: &str) -> Result<Login> {
    let store = Store::load()?;
    let cred = store.get(name)?;
    if presets::get_or_generic(&cred.service).kind != presets::Kind::Login {
        bail!(
            "\"{name}\" isn't a website login; sign_in only fills logins added with add_website_login"
        );
    }
    let sealed = Sealed::load(vault, name, cred)?;
    let cred = sealed.apply(cred);
    let [host] = cred.hosts.as_slice() else {
        bail!("\"{name}\" has no site set; add it again with add_website_login");
    };
    let Auth::Basic { username } = &cred.auth else {
        bail!("\"{name}\" has no username; set it with update_credential(username=...)");
    };
    Ok(Login {
        origin: origin_for(host, &cred.url),
        username: username.clone(),
        password: sealed.secret,
    })
}

/// A new website login's metadata (its password goes in a [`Sealed`]).
pub fn login_credential(url: &str, username: &str, description: String) -> Result<Credential> {
    let host = login_host(url)?;
    if username.trim().is_empty() {
        bail!("a website login needs the username (or email) the user signs in with");
    }
    Ok(Credential {
        service: "login".into(),
        url: origin_for(&host, url),
        description,
        auth: Auth::Basic {
            username: username.trim().to_string(),
        },
        hosts: vec![host],
        added_by: "mcp".into(),
        ssh: None,
    })
}

/// The tab to fill: the first one on `origin`.
pub(super) async fn find_tab(cdp: &mut Cdp, origin: &str) -> Result<(String, Vec<String>)> {
    let targets = cdp.call("Target.getTargets", json!({}), None).await?;
    let pages: Vec<(String, String)> = targets["targetInfos"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|t| t["type"] == "page")
        .filter_map(|t| {
            Some((
                t["targetId"].as_str()?.to_string(),
                t["url"].as_str()?.to_string(),
            ))
        })
        .collect();
    let open: Vec<String> = pages.iter().map(|(_, u)| u.clone()).collect();
    match pages
        .into_iter()
        .find(|(_, u)| origin_of(u).as_deref() == Some(origin))
    {
        Some((id, _)) => Ok((id, open)),
        None => Ok((String::new(), open)),
    }
}

pub const NOT_RUNNING: &str = "The keymule browser isn't running. sign_in fills logins into the \
Chrome that `keymule browser` starts, so run your browser tool through it, e.g. for Claude Code:\n  \
claude mcp add playwright -- keymule browser -- npx @playwright/mcp@latest\n\
then reconnect it (/mcp) and open the site's sign-in page with it.";

/// Fill `login` into the keymule browser's tab on its site, after `ask`
/// says yes. Returns what happened, for the agent.
pub async fn sign_in(
    name: &str,
    login: Login,
    ask: impl FnOnce(&str) -> Result<bool>,
) -> Result<String> {
    let state = running().await.ok_or_else(|| anyhow!(NOT_RUNNING))?;
    let mut cdp = Cdp::connect(&state.ws).await?;
    let (target, open) = find_tab(&mut cdp, &login.origin).await?;
    if target.is_empty() {
        let open = if open.is_empty() {
            "no tabs".to_string()
        } else {
            open.join(", ")
        };
        bail!(
            "No tab in the keymule browser is on {} (open: {open}). Open the site's sign-in page \
             with your browser tool first, then call sign_in again.",
            login.origin
        );
    }
    let host = login.origin.split("://").nth(1).unwrap_or(&login.origin);
    let question = format!(
        "Sign in to {host} as {}?\n\nYour agent asked to use \"{name}\". keymule types the \
         password into the page on {}; the agent never sees it.",
        login.username, login.origin
    );
    if !ask(&question)? {
        bail!("The user didn't allow signing in to {host}. Nothing was filled.");
    }
    let session = cdp
        .call(
            "Target.attachToTarget",
            json!({ "targetId": target, "flatten": true }),
            None,
        )
        .await?["sessionId"]
        .as_str()
        .ok_or_else(|| anyhow!("couldn't attach to the tab"))?
        .to_string();
    let arg = |phase: u8| json!({ "origin": login.origin, "username": login.username, "password": login.password, "phase": phase });
    let mut phase = 0;
    let deadline = Instant::now() + Duration::from_secs(20);
    let result = loop {
        let r = cdp.run(&session, FILL, &arg(phase)).await?;
        let status = r
            .as_ref()
            .and_then(|v| v["status"].as_str())
            .unwrap_or("wait");
        match status {
            "submitted" => break Ok(()),
            "username" => phase = 1,
            "origin" => {
                let at = r
                    .as_ref()
                    .and_then(|v| v["at"].as_str())
                    .unwrap_or("another site");
                break Err(anyhow!(
                    "The tab moved to {at}, so keymule stopped: \"{name}\" is only filled on {}. \
                     If the site signs in somewhere else (single sign-on), that needs its own login.",
                    login.origin
                ));
            }
            "nofield" => {
                break Err(anyhow!(
                    "keymule found no sign-in form on the tab ({}). Open the page with the \
                     username and password fields, then call sign_in again.",
                    login.origin
                ));
            }
            _ => {}
        }
        if Instant::now() > deadline {
            break Err(anyhow!(
                "The password field didn't appear within 20 seconds of entering the username. \
                 Take a snapshot to see what the site is asking for."
            ));
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    };
    // Wait for the outcome: the sign-in form gone (signed in), or a new page
    // that shows it again (refused). A form still up on the same page after
    // 8 seconds means the site wants something else (a code, a CAPTCHA) or
    // showed an error in place. Then make sure the password doesn't stay.
    let mut after = None;
    if result.is_ok() {
        for _ in 0..16 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            if let Some(v) = cdp.run(&session, AFTER, &json!({ "clear": false })).await? {
                let form = v["form"].as_bool().unwrap_or(false);
                let fresh = v["fresh"].as_bool().unwrap_or(false);
                let ready = v["ready"].as_bool().unwrap_or(false);
                after = Some(v);
                if ready && (!form || fresh) {
                    break;
                }
            }
        }
    }
    let _ = cdp.run(&session, AFTER, &json!({ "clear": true })).await;
    let _ = cdp
        .call(
            "Target.detachFromTarget",
            json!({ "sessionId": session }),
            None,
        )
        .await;
    result?;
    let url = after
        .as_ref()
        .and_then(|v| v["url"].as_str())
        .unwrap_or(&login.origin)
        .to_string();
    let still = after
        .as_ref()
        .is_none_or(|v| v["form"].as_bool().unwrap_or(false));
    // In the activity log: 200 when the form went away, 401 when it's still there.
    crate::http::audit(
        name,
        "SIGN-IN",
        &login.origin,
        if still { 401 } else { 200 },
    );
    Ok(if still {
        format!(
            "keymule filled \"{name}\" on {} and submitted, but the sign-in form is still there \
             (now {url}): the password may be wrong, or the site wants more (a code, a CAPTCHA). \
             Take a snapshot to see. If the password changed, offer rotate_credential.",
            login.origin
        )
    } else {
        format!(
            "Signed in to {host} as {} (the tab is now on {url}). Take a snapshot to carry on.",
            login.username
        )
    })
}
