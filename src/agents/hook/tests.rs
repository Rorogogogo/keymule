use super::*;
use crate::store::testing::MemVault;

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::load_from(dir.path().join("credentials.toml")).unwrap();
    (dir, s)
}

const GH: &str = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";

fn context(o: Outcome) -> (String, String) {
    match o {
        Outcome::Context { context, notice } => (context, notice),
        other => panic!("expected context, got {other:?}"),
    }
}

// Split so the source never holds something that looks like a real key.
const STRIPE: &str = concat!("sk_", "live_", "51ExampleExampleExample0123456789abcd");

/// A folder like ~/Developer/LaunchStory: a git repo named LaunchStory.
fn repo_dir(name: &str) -> (tempfile::TempDir, String) {
    let d = tempfile::tempdir().unwrap();
    let repo = d.path().join(name);
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(repo.join("client/src")).unwrap();
    let sub = repo.join("client/src").to_str().unwrap().to_string();
    (d, sub)
}

#[test]
fn a_bare_stripe_key_is_named_after_the_repo_and_locked_to_stripe() {
    let (_d, mut s) = store();
    let v = MemVault::default();
    let (_r, cwd) = repo_dir("LaunchStory");
    let Outcome::Block(q) = handle_as(Agent::Claude, STRIPE, "s", Some(&cwd), &mut s, &v, 1) else {
        panic!("should block")
    };
    assert!(
        q.contains("Stripe live key") && q.contains("`stripe-launchstory`"),
        "{q}"
    );
    // The answer comes in a later prompt; the held one remembers the folder.
    handle_as(Agent::Claude, "y", "s", None, &mut s, &v, 2);
    let c = &s.creds["stripe-launchstory"];
    assert_eq!(c.service, "stripe");
    assert_eq!(c.url, "https://api.stripe.com");
    assert_eq!(c.hosts, vec!["api.stripe.com".to_string()]);
    assert_eq!(
        Sealed::peek_secret(&v, "stripe-launchstory").unwrap(),
        STRIPE
    );
}

#[test]
fn the_message_beats_the_folder_and_no_repo_means_no_suffix() {
    let (_d, mut s) = store();
    let v = MemVault::default();
    let (_r, cwd) = repo_dir("LaunchStory");
    let Outcome::Block(q) = handle_as(
        Agent::Claude,
        &format!("{STRIPE} for acme"),
        "s",
        Some(&cwd),
        &mut s,
        &v,
        1,
    ) else {
        panic!()
    };
    assert!(q.contains("`stripe-acme`"), "{q}");
    let plain = tempfile::tempdir().unwrap();
    assert_eq!(project_hint(plain.path().to_str().unwrap()), None);
    let Outcome::Block(q) = handle_as(
        Agent::Claude,
        STRIPE,
        "t",
        plain.path().to_str(),
        &mut s,
        &v,
        1,
    ) else {
        panic!()
    };
    assert!(q.contains("`stripe`"), "{q}");
}

#[test]
fn holds_the_prompt_and_asks_in_the_terminal() {
    let (_d, mut s) = store();
    let v = MemVault::default();
    let Outcome::Block(q) = handle(&format!("use {GH} please"), "s1", &mut s, &v, 100) else {
        panic!("should block")
    };
    assert!(q.contains("y <name>") && q.contains("`github`"), "{q}");
    assert!(!q.contains(GH), "the question must not echo the secret");
    assert!(s.creds.is_empty(), "nothing is saved before the answer");
}

#[test]
fn y_saves_and_sends_the_message_without_the_secret() {
    let (_d, mut s) = store();
    let v = MemVault::default();
    handle(&format!("use {GH} for launchstory"), "s1", &mut s, &v, 100);
    let (ctx, notice) = context(handle("y github-launchstory", "s1", &mut s, &v, 130));
    assert_eq!(Sealed::peek_secret(&v, "github-launchstory").unwrap(), GH);
    assert!(ctx.contains("use [saved in keymule as `github-launchstory`] for launchstory"));
    assert!(!ctx.contains(GH) && !notice.contains(GH));
    assert!(v.get(PENDING).is_err(), "the held prompt is gone");
}

#[test]
fn names_and_describes_it_from_the_message() {
    let (_d, mut s) = store();
    let v = MemVault::default();
    let msg = format!("this is the pat for launchstory github account {GH}, keep my own one too");
    let Outcome::Block(q) = handle(&msg, "s", &mut s, &v, 1) else {
        panic!()
    };
    assert!(q.contains("`github-launchstory`"), "{q}");
    handle("y", "s", &mut s, &v, 2);
    let c = &s.creds["github-launchstory"];
    assert!(
        c.description.starts_with("GitHub token for launchstory"),
        "{}",
        c.description
    );
    assert!(c.description.contains("pat for launchstory github account"));
    assert!(!c.description.contains(GH));
    assert_eq!(
        context_hint("use it for the acme org", &[]).as_deref(),
        Some("acme")
    );
    assert_eq!(context_hint("check my actions runs", &[]), None);
}

#[test]
fn scan_and_save_for_the_inline_question() {
    let (_d, mut s) = store();
    let v = MemVault::default();
    let msg = format!("the pat for the acme org is {GH} thanks");
    let scan = scan_json(&msg, None, &s, &v);
    let txt = scan.to_string();
    assert!(!txt.contains(GH), "scan output must not hold the secret");
    assert_eq!(scan["found"][0]["suggested"], "github-acme");
    assert_eq!(
        scan["stripped"],
        "the pat for the acme org is [secret removed by keymule] thanks"
    );
    let saved = save_json(&msg, None, Some("gh-acme"), &mut s, &v);
    assert_eq!(
        saved["text"],
        "the pat for the acme org is [saved in keymule as `gh-acme`] thanks"
    );
    assert!(!saved.to_string().contains(GH));
    assert_eq!(Sealed::peek_secret(&v, "gh-acme").unwrap(), GH);
    assert_eq!(
        scan_json(&msg, None, &s, &v)["found"][0]["known"],
        "gh-acme"
    );
}

#[test]
fn n_sends_it_as_it_was() {
    let (_d, mut s) = store();
    let v = MemVault::default();
    handle(GH, "s1", &mut s, &v, 100);
    let (ctx, _) = context(handle("n", "s1", &mut s, &v, 110));
    assert!(ctx.contains(GH));
    assert!(s.creds.is_empty());
}

#[test]
fn an_answer_from_another_session_or_too_late_is_a_normal_prompt() {
    let (_d, mut s) = store();
    let v = MemVault::default();
    handle(GH, "s1", &mut s, &v, 100);
    assert_eq!(handle("y", "s2", &mut s, &v, 110), Outcome::Pass);
    handle(GH, "s1", &mut s, &v, 100);
    assert_eq!(
        handle("y", "s1", &mut s, &v, 100 + PENDING_SECS + 1),
        Outcome::Pass
    );
    assert!(s.creds.is_empty());
}

#[test]
fn bamboo_token_needs_its_url_later() {
    let (_d, mut s) = store();
    let v = MemVault::default();
    handle(
        "my bamboo token is NjQ3MjE4MzQ1Njc4OkxQmPq9Zr2Wd+Hs8 check the build",
        "s",
        &mut s,
        &v,
        1,
    );
    let (_, notice) = context(handle("y", "s", &mut s, &v, 2));
    assert!(
        notice.contains("`bamboo`") && notice.contains("keymule edit bamboo --url"),
        "{notice}"
    );
    assert_eq!(s.creds["bamboo"].added_by, "paste-hook");
}

#[test]
fn github_token_is_ready_to_use() {
    let (_d, mut s) = store();
    let v = MemVault::default();
    handle(GH, "s", &mut s, &v, 1);
    let (_, notice) = context(handle("yes", "s", &mut s, &v, 2));
    assert!(!notice.contains("finish setup"));
    assert_eq!(
        s.creds["github"].hosts,
        vec!["api.github.com", "github.com", "uploads.github.com"]
    );
}

#[test]
fn database_url_becomes_an_mcp_secret() {
    let (_d, mut s) = store();
    let v = MemVault::default();
    let url = "postgres://app:Vq7Lp2Xk9Rt4@db.internal:5432/app";
    handle(&format!("add a pg mcp for {url}"), "s", &mut s, &v, 1);
    let (ctx, _) = context(handle("y", "s", &mut s, &v, 2));
    assert!(
        ctx.contains("`postgres`") && !ctx.contains("Vq7Lp2Xk9Rt4"),
        "{ctx}"
    );
    let b = crate::servers::Binding::load(&v, "postgres").unwrap();
    assert_eq!(b.env["DATABASE_URL"], url);
    assert!(b.commands.is_empty(), "no command is approved yet");
}

#[test]
fn a_secret_already_saved_is_not_saved_twice() {
    let (_d, mut s) = store();
    let v = MemVault::default();
    handle(GH, "s", &mut s, &v, 1);
    handle("y", "s", &mut s, &v, 2);
    let Outcome::Block(q) = handle(GH, "s", &mut s, &v, 3) else {
        panic!()
    };
    assert!(q.contains("already in keymule as `github`"), "{q}");
    let (_, notice) = context(handle("y", "s", &mut s, &v, 4));
    assert!(notice.contains("already saved"));
    assert_eq!(s.creds.len(), 1);
}

#[test]
fn lets_clean_prompts_and_ignore_marker_through() {
    let (_d, mut s) = store();
    let v = MemVault::default();
    assert!(run(r#"{"prompt":"check my bamboo pipeline"}"#, &mut s, &v).is_none());
    assert_eq!(
        handle("y", "s", &mut s, &v, 1),
        Outcome::Pass,
        "a lone y with nothing held"
    );
    assert_eq!(
        handle(&format!("{GH} {IGNORE_MARKER}"), "s", &mut s, &v, 1),
        Outcome::Pass
    );
    assert!(s.creds.is_empty());
}

#[test]
fn hook_json_hides_the_prompt_and_carries_the_context() {
    let (_d, mut s) = store();
    let v = MemVault::default();
    let block = run(
        &format!(r#"{{"prompt":"{GH}","session_id":"s"}}"#),
        &mut s,
        &v,
    )
    .unwrap();
    let j: serde_json::Value = serde_json::from_str(&block).unwrap();
    assert_eq!(j["decision"], "block");
    assert_eq!(j["hookSpecificOutput"]["suppressOriginalPrompt"], true);
    let ok = run(r#"{"prompt":"y","session_id":"s"}"#, &mut s, &v).unwrap();
    let j: serde_json::Value = serde_json::from_str(&ok).unwrap();
    assert!(j.get("decision").is_none());
    assert!(
        j["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .contains("`github`")
    );
}

#[test]
fn codex_gets_a_plain_block_then_the_context() {
    let (_d, mut s) = store();
    let v = MemVault::default();
    let held = format!(r#"{{"prompt":"{GH}","session_id":"c1","turn_id":"t"}}"#);
    let j: serde_json::Value =
        serde_json::from_str(&run_as(Agent::Codex, &held, &mut s, &v, &|_| None).unwrap()).unwrap();
    assert_eq!(j["decision"], "block");
    assert!(j.get("hookSpecificOutput").is_none(), "{j}");
    assert!(!j.to_string().contains(GH));
    let ok = run_as(
        Agent::Codex,
        r#"{"prompt":"y","session_id":"c1"}"#,
        &mut s,
        &v,
        &|_| None,
    )
    .unwrap();
    let j: serde_json::Value = serde_json::from_str(&ok).unwrap();
    assert_eq!(j["hookSpecificOutput"]["hookEventName"], "UserPromptSubmit");
    assert!(
        j["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .contains("`github`")
    );
    assert_eq!(s.creds.len(), 1);
}

fn codex(
    prompt: &str,
    session: &str,
    s: &mut Store,
    v: &MemVault,
    pick: Option<PasteChoice>,
) -> serde_json::Value {
    let input = serde_json::json!({ "prompt": prompt, "session_id": session }).to_string();
    let asked = std::cell::Cell::new(false);
    let ask = |q: &str| {
        asked.set(true);
        assert!(
            q.contains("Your message to Codex has a") && !q.contains(GH),
            "{q}"
        );
        pick
    };
    match run_as(Agent::Codex, &input, s, v, &ask) {
        Some(out) => serde_json::from_str(&out).unwrap(),
        None => serde_json::Value::Null,
    }
}

#[test]
fn codex_asks_with_a_choice_instead_of_stopping_with_text() {
    let (_d, mut s) = store();
    let v = MemVault::default();
    // Send Anyway: no stop at all.
    let j = codex(GH, "c1", &mut s, &v, Some(PasteChoice::SendAsIs));
    assert!(j.is_null(), "{j}");
    assert!(s.creds.is_empty());
    // Don't Send.
    let j = codex(GH, "c1", &mut s, &v, Some(PasteChoice::DontSend));
    assert!(j["reason"].as_str().unwrap().starts_with("Not sent"), "{j}");
    assert!(s.creds.is_empty());
    // Save: saved now, then y sends it with the name in place.
    let j = codex(
        &format!("deploy with {GH}"),
        "c2",
        &mut s,
        &v,
        Some(PasteChoice::Save),
    );
    let why = j["reason"].as_str().unwrap();
    assert!(
        why.contains("keymule saved") && why.contains("Type y"),
        "{why}"
    );
    assert!(!j.to_string().contains(GH));
    assert_eq!(s.creds.len(), 1);
    let j = codex("y", "c2", &mut s, &v, None);
    let ctx = j["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(
        ctx.contains("deploy with [saved in keymule as `github`]") && !ctx.contains(GH),
        "{ctx}"
    );
    assert_eq!(s.creds.len(), 1, "not saved twice");
}

#[test]
fn no_dialog_falls_back_to_the_text_question() {
    let (_d, mut s) = store();
    let v = MemVault::default();
    let j = codex(GH, "c1", &mut s, &v, None);
    assert!(
        j["reason"]
            .as_str()
            .unwrap()
            .contains("held back your message"),
        "{j}"
    );
    assert!(s.creds.is_empty());
}

#[test]
fn allow_all_saves_without_asking() {
    let (_d, mut s) = store();
    let v = MemVault::default();
    let dir = tempfile::tempdir().unwrap();
    crate::trust::grant(&v, dir.path(), None, 1).unwrap();
    let input = serde_json::json!({
        "prompt": GH, "session_id": "c1", "cwd": dir.path().to_str().unwrap()
    })
    .to_string();
    let never = |_: &str| -> Option<PasteChoice> { panic!("allow all: no dialog") };
    let out = run_as(Agent::Codex, &input, &mut s, &v, &never).unwrap();
    assert!(
        out.contains("allow all is on here") && !out.contains(GH),
        "{out}"
    );
    assert_eq!(s.creds.len(), 1);
}

#[test]
fn cursor_saves_and_hands_back_the_message_to_send() {
    let (_d, mut s) = store();
    let v = MemVault::default();
    let cursor = |prompt: &str, s: &mut Store| -> serde_json::Value {
        let input = serde_json::json!({ "prompt": prompt, "conversation_id": "k1" });
        serde_json::from_str(&run_as(Agent::Cursor, &input.to_string(), s, &v, &|_| None).unwrap())
            .unwrap()
    };
    assert_eq!(cursor("check my actions runs", &mut s)["continue"], true);
    let j = cursor(&format!("use {GH} for the acme repo"), &mut s);
    assert_eq!(j["continue"], false);
    let q = j["user_message"].as_str().unwrap();
    assert!(
        q.contains("get your message back") && q.contains(IGNORE_MARKER),
        "{q}"
    );
    assert!(!q.contains(GH));
    let j = cursor("y", &mut s);
    assert_eq!(j["continue"], false);
    let back = j["user_message"].as_str().unwrap();
    assert!(
        back.contains("send this instead") && back.contains("[saved in keymule as `github"),
        "{back}"
    );
    assert!(!back.contains(GH));
    assert_eq!(s.creds.len(), 1);
    // `n` can't send it in Cursor: it says how.
    cursor(GH, &mut s);
    let j = cursor("n", &mut s);
    assert!(j["user_message"].as_str().unwrap().contains(IGNORE_MARKER));
}

#[test]
fn variable_names_become_secret_names() {
    assert_eq!(
        var_hint("HOMEBREW_TAP_TOKEN").as_deref(),
        Some("homebrew-tap")
    );
    assert_eq!(var_hint("CWS_CLIENT_SECRET").as_deref(), Some("cws"));
    assert_eq!(var_hint("GITHUB_TOKEN"), None, "says nothing");
    assert_eq!(var_hint("GH_TOKEN"), None);
    assert_eq!(var_hint("OPENAI_API_KEY"), None);
    assert_eq!(var_hint("ACME_DEPLOY_PAT").as_deref(), Some("acme-deploy"));
}

#[test]
fn the_conversation_names_a_bare_paste() {
    let (_d, mut s) = store();
    let v = MemVault::default();
    set_conversation(
        "Publishing to Homebrew needs a `HOMEBREW_TAP_TOKEN` repo secret that can push to \
             Rorogogogo/homebrew-tap. Paste it in your next message."
            .into(),
    );
    let found = detect::scan(GH);
    let hint = name_hint(GH, &found, None);
    assert_eq!(hint.name(), Some("homebrew-tap"));
    assert!(hint.why.as_deref().unwrap().contains("HOMEBREW_TAP_TOKEN"));

    let (msg, notes) = save_all(&format!("ok here's the token {GH}"), None, None, &mut s, &v);
    assert!(notes[0].starts_with("`github-homebrew-tap`"), "{notes:?}");
    assert!(msg.contains("github-homebrew-tap") && !msg.contains(GH));
    let d = &s.creds["github-homebrew-tap"].description;
    assert!(
        d.contains("named from the conversation: HOMEBREW_TAP_TOKEN"),
        "{d}"
    );

    // What the message says still comes first.
    let hint = name_hint(&format!("the PAT for our acme account {GH}"), &found, None);
    assert_eq!(hint.name(), Some("acme"));
    assert_eq!(hint.why, None);
    set_conversation(String::new());
}

#[test]
fn a_secret_in_the_conversation_never_becomes_a_name() {
    set_conversation(format!("earlier I pasted {GH} by mistake"));
    let found = detect::scan(GH);
    let hint = name_hint(GH, &found, None);
    assert!(hint.name().is_none_or(|n| !n.contains("ghp")), "{hint:?}");
    set_conversation(String::new());
}

#[test]
fn transcripts_from_claude_code_and_codex_are_read() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join("c.jsonl");
    std::fs::write(
            &claude,
            [
                r#"{"type":"user","message":{"role":"user","content":"set up the release"}}"#,
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"It needs HOMEBREW_TAP_TOKEN."},{"type":"tool_use","name":"Bash","input":{"command":"echo SECRET_TOOL_TOKEN"}}]}}"#,
                "not json",
            ]
            .join("\n"),
        )
        .unwrap();
    let t = conversation_from_transcript(claude.to_str().unwrap());
    assert!(
        t.contains("HOMEBREW_TAP_TOKEN") && t.contains("set up the release"),
        "{t}"
    );
    assert!(!t.contains("SECRET_TOOL_TOKEN"), "tool calls are left out");

    let codex = dir.path().join("x.jsonl");
    std::fs::write(
            &codex,
            r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Add ACME_DEPLOY_TOKEN to CI"}]}}"#,
        )
        .unwrap();
    let t = conversation_from_transcript(codex.to_str().unwrap());
    assert_eq!(t, "Add ACME_DEPLOY_TOKEN to CI");
    assert_eq!(conversation_from_transcript("/no/such/file"), "");
}

#[test]
fn the_mod_can_send_the_conversation() {
    let input = serde_json::json!({
        "keymule_input": 1,
        "prompt": "here it is",
        "messages": [
            {"role": "assistant", "content": [{"type": "text", "text": "I need STRIPE_LIVE_KEY for shop"}]},
        ],
    });
    assert_eq!(mod_input(&input.to_string()), "here it is");
    assert!(CONVERSATION.with(|c| c.borrow().contains("STRIPE_LIVE_KEY")));
    assert_eq!(mod_input("plain text"), "plain text");
    set_conversation(String::new());
}

#[test]
fn a_pasted_secret_is_held_until_the_user_names_it() {
    let (_d, mut s) = store();
    let v = MemVault::default();
    set_conversation("we need HOMEBREW_TAP_TOKEN for the release".into());
    let (msg, notes) = hold_all(&format!("ok here's the token {GH}"), None, &s, &v, 1000);
    assert!(!msg.contains(GH), "{msg}");
    let HeldNote::Held { id, guess, why, .. } = &notes[0] else {
        panic!("{notes:?}")
    };
    assert_eq!(guess, "github-homebrew-tap");
    assert!(why.as_deref().unwrap().contains("HOMEBREW_TAP_TOKEN"));
    assert!(msg.contains(&format!("id {id}")));
    assert!(s.creds.is_empty(), "nothing usable until it's named");

    let ctx = hold_context(&notes);
    assert!(
        ctx.contains("save_held")
            && ctx.contains("Don't save it")
            && ctx.contains("github-homebrew-tap")
    );
    assert!(!ctx.contains(GH));

    assert!(save_held(id, "bad name", "x", &mut s, &v, 1000).is_err());
    let note = save_held(
        id,
        "github-tap",
        "pushes the Homebrew formula",
        &mut s,
        &v,
        1000,
    )
    .unwrap();
    assert!(note.contains("github-tap"), "{note}");
    assert_eq!(
        s.creds["github-tap"].description,
        "pushes the Homebrew formula"
    );
    assert_eq!(
        known_as(&detect::scan(GH)[0], &s, &v).as_deref(),
        Some("github-tap")
    );
    assert!(
        save_held(id, "again", "x", &mut s, &v, 1000).is_err(),
        "only once"
    );
    assert!(held_ids(&v).is_empty());

    // Pasting it again: already saved, nothing held.
    let (msg, notes) = hold_all(GH, None, &s, &v, 1000);
    assert_eq!(notes, vec![HeldNote::Known("github-tap".into())]);
    assert!(msg.contains("`github-tap`"));
    set_conversation(String::new());
}

#[test]
fn held_secrets_can_be_thrown_away_and_time_out() {
    let (_d, s) = store();
    let v = MemVault::default();
    let (_, notes) = hold_all(GH, None, &s, &v, 1000);
    let HeldNote::Held { id, .. } = &notes[0] else {
        panic!()
    };
    assert!(discard_held(id, &v).is_ok());
    assert!(v.get(&held_item(id)).is_err());

    let (_, notes) = hold_all(GH, None, &s, &v, 1000);
    let HeldNote::Held { id, .. } = &notes[0] else {
        panic!()
    };
    prune_held(&v, 1000 + HELD_SECS - 1);
    assert!(load_held(&v, id).is_some(), "still in time");
    prune_held(&v, 1000 + HELD_SECS);
    assert!(load_held(&v, id).is_none(), "thrown away");
    assert!(held_ids(&v).is_empty());
}

#[test]
fn a_private_key_is_held_whole() {
    let (_d, s) = store();
    let v = MemVault::default();
    let pem = "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA0abc\nxyz123\n-----END RSA PRIVATE KEY-----";
    let (msg, notes) = hold_all(&format!("deploy key:\n{pem}"), None, &s, &v, 5);
    assert!(!msg.contains("MIIEow") && !msg.contains("BEGIN"), "{msg}");
    let HeldNote::Held { id, label, .. } = &notes[0] else {
        panic!()
    };
    assert_eq!(label, "RSA private key");
    assert_eq!(load_held(&v, id).unwrap().value, pem);
}

#[test]
fn passwords_keys_and_aws_keys_get_names_that_say_what_they_are() {
    let (_d, s) = store();
    let v = MemVault::default();
    set_conversation(String::new());
    let (_, notes) = hold_all("the staging password is Tr0ub4dor&3x", None, &s, &v, 1);
    let HeldNote::Held { guess, label, .. } = &notes[0] else {
        panic!("{notes:?}")
    };
    assert_eq!(label, "password");
    assert_eq!(guess, "password", "no hint: no \"generic\" in it");

    let (_, notes) = hold_all(
        "the password for our acme account is Tr0ub4dor&3x",
        None,
        &s,
        &v,
        1,
    );
    let HeldNote::Held { guess, .. } = &notes[0] else {
        panic!()
    };
    assert_eq!(guess, "acme-password");

    let (aws_id, aws_key) = (detect::fake::aws_id(), detect::fake::aws_secret());
    let aws = format!("id {aws_id} secret {aws_key}");
    let (msg, notes) = hold_all(&aws, None, &s, &v, 1);
    assert_eq!(
        notes.len(),
        1,
        "the ID and secret are held together: {notes:?}"
    );
    let HeldNote::Held {
        id, label, guess, ..
    } = &notes[0]
    else {
        panic!()
    };
    assert_eq!(label, "AWS access keys");
    assert_eq!(guess, "aws");
    assert!(!msg.contains(&aws_id) && !msg.contains(&aws_key), "{msg}");
    assert_eq!(
        load_held(&v, id).unwrap().value,
        format!("{aws_id}\n{aws_key}")
    );
}

#[test]
fn a_held_password_is_saved_as_a_password_not_an_api_token() {
    let (_d, mut s) = store();
    let v = MemVault::default();
    let (_, notes) = hold_all("db password: Tr0ub4dor&3x", None, &s, &v, 1);
    let HeldNote::Held { id, .. } = &notes[0] else {
        panic!()
    };
    let note = save_held(id, "staging-db-password", "staging db", &mut s, &v, 1).unwrap();
    assert!(!note.contains("--url"), "nothing to set up: {note}");
    assert_eq!(s.creds["staging-db-password"].service, "password");
    assert!(s.creds["staging-db-password"].hosts.is_empty());
}
