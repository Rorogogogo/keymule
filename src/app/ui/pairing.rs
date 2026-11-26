//! Pairing a browser for keymule's questions, and signing in with a paired one.

use super::*;

/// How long a login challenge stays good.
pub(super) const CHALLENGE_SECS: u64 = 60;

pub(super) fn challenge(ctx: &Ctx) -> Value {
    let Ok(nonce) = random_token() else {
        return json!({"ok": false});
    };
    let mut list = ctx.challenges.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    list.retain(|(_, until)| *until > now);
    if list.len() >= 32 {
        list.remove(0);
    }
    list.push((nonce.clone(), now + Duration::from_secs(CHALLENGE_SECS)));
    json!({"ok": true, "nonce": nonce})
}

pub(super) fn login(ctx: &Ctx, body: &Value) -> Value {
    let field = |k: &str| body.get(k).and_then(Value::as_str).unwrap_or("");
    let nonce = field("nonce");
    // Each challenge works once, whatever the outcome.
    let fresh = {
        let mut list = ctx.challenges.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let at = list
            .iter()
            .position(|(n, until)| *until > now && same(n.as_bytes(), nonce.as_bytes()));
        at.map(|i| list.remove(i)).is_some()
    };
    if !fresh {
        return json!({"ok": false, "stderr": "that challenge ran out; try again"});
    }
    let vault = crate::store::vault();
    let devices = crate::approvals::devices(vault.as_ref());
    match crate::approvals::verify_login(&devices, field("device"), nonce, field("sig")) {
        Ok(()) => {
            crate::http::audit("-", "PAGE-LOGIN", field("device"), 0);
            json!({"ok": true, "token": ctx.token})
        }
        Err(e) => json!({"ok": false, "stderr": e.to_string()}),
    }
}

/// Pair a browser for page approvals. The first one needs the code printed
/// by `keymule ui` (or, on a Mac, a yes in a dialog); after that, a browser
/// that's already paired has to allow each new one.
pub(super) fn pair(ctx: &Ctx, name: &str, key: &str, code: &str) -> Value {
    let vault = crate::store::vault();
    let device = match crate::approvals::new_device(name, key) {
        Ok(d) => d,
        Err(e) => return json!({"ok": false, "stderr": e.to_string()}),
    };
    let known = crate::approvals::devices(vault.as_ref());
    if known.iter().any(|d| d.id == device.id) {
        return json!({"ok": true, "id": device.id});
    }
    let code = code.replace([' ', '-'], "");
    let code_ok = !code.is_empty() && same(code.as_bytes(), ctx.code.replace('-', "").as_bytes());
    let allowed = if known.is_empty() {
        if !code.is_empty() {
            code_ok
        } else {
            first_pair_on_screen(&device.name)
        }
    } else if !code.is_empty() {
        if !ctx.code_repairs {
            return json!({"ok": false, "stderr": "another browser is paired, so the code only \
                works when `keymule ui` runs in your own terminal. Start it there and use the \
                code it prints"});
        }
        code_ok
    } else {
        matches!(
            crate::dialog::confirm(&format!(
                "Let \"{}\" answer keymule's questions?\n\nA new browser wants to pair. It \
                 could then approve anything keymule asks.",
                device.name
            )),
            Ok(true)
        )
    };
    if !allowed {
        return json!({"ok": false, "stderr": if !code.is_empty() {
            "that code doesn't match the one `keymule ui` printed"
        } else if known.is_empty() {
            "type the code `keymule ui` printed in its terminal"
        } else {
            "a paired browser didn't allow it"
        }});
    }
    let id = device.id.clone();
    match crate::approvals::add_device(vault.as_ref(), device) {
        Ok(()) => {
            DEVICES_GEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            crate::http::audit("-", "PAIR", &id, 0);
            json!({"ok": true, "id": id})
        }
        Err(e) => json!({"ok": false, "stderr": e.to_string()}),
    }
}

/// On a Mac, the first browser can be paired with a yes in a dialog.
pub(super) fn first_pair_on_screen(name: &str) -> bool {
    crate::dialog::confirm_on_screen(&format!(
        "Pair \"{name}\" with keymule?\n\nIt will be able to answer keymule's questions in the page."
    ))
    .unwrap_or(false)
}
