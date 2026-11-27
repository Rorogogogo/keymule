//! keymule browser --open: keymule's Chrome in a window of its own, for
//! browser agents that live in the browser (Claude in Chrome) rather than
//! run as an MCP server.

use super::*;

/// The Claude in Chrome extension.
pub(super) const CLAUDE_EXTENSION: &str = "fcoeoabgfenejglbffodgkkbkcdhcgfn";

fn store_page() -> String {
    format!("https://chromewebstore.google.com/detail/{CLAUDE_EXTENSION}")
}

/// Whether keymule's profile has Claude in Chrome installed.
pub(super) fn has_claude(profile: &std::path::Path) -> bool {
    profile
        .join("Default")
        .join("Extensions")
        .join(CLAUDE_EXTENSION)
        .is_dir()
}

/// Open keymule's Chrome at `url` (empty: the Claude in Chrome extension's
/// page until it's installed, else just the window).
pub async fn open(url: &str) -> Result<()> {
    if !url.is_empty() && !(url.starts_with("https://") || url.starts_with("http://")) {
        bail!("give an http:// or https:// address to open");
    }
    let profile = profile_dir()?;
    let claude = has_claude(&profile);
    let first = match (url.is_empty(), claude) {
        (false, _) => url.to_string(),
        (true, false) => store_page(),
        (true, true) => String::new(),
    };
    let (state, started) = ensure_chrome(Start {
        url: if first.is_empty() {
            "about:blank"
        } else {
            &first
        },
        window: true,
    })
    .await?;
    if started.is_none() && !first.is_empty() {
        let mut cdp = Cdp::connect(&state.ws).await?;
        cdp.call("Target.createTarget", json!({ "url": first }), None)
            .await?;
    }
    // Dropping the child leaves Chrome running; it's in its own process group.
    drop(started);
    if claude {
        println!(
            "keymule's Chrome is open, with Claude in Chrome. When Claude signs in to a site in it, \
             it calls keymule's sign_in: you approve, and keymule types the password into the page."
        );
    } else {
        println!(
            "keymule's Chrome is open on the Claude in Chrome extension's page.\n\
             \n  1. Add the extension there and sign in to Claude in it. This Chrome has its own profile\
             \n     ({}), apart from your everyday Chrome.\
             \n  2. In Claude Code, run /chrome; pick this browser if it asks which.\
             \n  3. Ask Claude to sign in to a site. It opens the sign-in page here and calls sign_in;\
             \n     you approve, and keymule types the password into the page. Claude never sees it.\
             \n\nClosed the window? `keymule browser --open` brings it back.",
            profile.display()
        );
    }
    Ok(())
}
