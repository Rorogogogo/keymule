use super::*;

#[test]
fn hook_install_preserves_other_settings_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("settings.json");
    std::fs::write(
            &p,
            r#"{"model":"opus","hooks":{"UserPromptSubmit":[{"hooks":[{"type":"command","command":"other"}]}]}}"#,
        )
        .unwrap();
    assert!(install_hook(&p, "'/bin/keymule' hook prompt", false).unwrap());
    assert!(!install_hook(&p, "'/bin/keymule' hook prompt", false).unwrap());
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    assert_eq!(v["model"], "opus");
    assert_eq!(v["hooks"]["UserPromptSubmit"].as_array().unwrap().len(), 2);
    assert!(p.with_extension("json.keymule-bak").exists());
}

#[test]
fn md_block_added_once() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("CLAUDE.md");
    std::fs::write(&p, "# mine\n").unwrap();
    assert!(install_md(&p, false).unwrap());
    assert!(!install_md(&p, false).unwrap());
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(text.starts_with("# mine\n\n<!-- keymule:start -->"));
}

#[test]
fn outdated_md_block_is_replaced_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("CLAUDE.md");
    std::fs::write(
        &p,
        format!("# mine\n\n{MD_START}\nold text\n{MD_END}\n\n# after\n"),
    )
    .unwrap();
    assert!(install_md(&p, false).unwrap());
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(!text.contains("old text") && text.contains("add_credential"));
    assert!(text.starts_with("# mine\n\n") && text.ends_with("\n\n# after\n"));
    assert!(!install_md(&p, false).unwrap());
}

#[test]
fn quotes_paths_with_spaces() {
    assert_eq!(shell_quote("/a b/keymule"), "'/a b/keymule'");
}

#[test]
fn sets_up_every_other_agent_once() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    std::fs::write(
        home.join(".codex/config.toml"),
        "# mine\nmodel = \"x\"\n\n[mcp_servers.acme]\ncommand = \"npx\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(home.join(".cursor")).unwrap();
    std::fs::write(
        home.join(".cursor/hooks.json"),
        r#"{"version":1,"hooks":{"afterFileEdit":[{"command":"./fmt.sh"}]}}"#,
    )
    .unwrap();
    let all = Found {
        claude_desktop: true,
        codex: true,
        cursor: true,
        antigravity: true,
    };
    let exe = "/opt/km/keymule";
    let lines = setup_others(home, exe, &all, false).unwrap().join("\n");
    assert!(lines.contains("type /hooks"), "{lines}");

    let codex = std::fs::read_to_string(home.join(".codex/config.toml")).unwrap();
    assert!(codex.starts_with("# mine\nmodel = \"x\"\n"), "{codex}");
    assert!(codex.contains("[mcp_servers.acme]"), "{codex}");
    let t: toml::Table = codex.parse().unwrap();
    assert_eq!(t["mcp_servers"]["keymule"]["command"].as_str(), Some(exe));
    assert_eq!(t["mcp_servers"]["keymule"]["args"][0].as_str(), Some("mcp"));
    let ch: Value = read_json(&home.join(".codex/hooks.json")).unwrap();
    assert_eq!(
        ch["hooks"]["UserPromptSubmit"][0]["hooks"][0]["command"],
        "'/opt/km/keymule' hook prompt --agent codex"
    );
    assert!(
        std::fs::read_to_string(home.join(".codex/AGENTS.md"))
            .unwrap()
            .contains(MD_START)
    );

    let cm = read_json(&home.join(".cursor/mcp.json")).unwrap();
    assert_eq!(cm["mcpServers"]["keymule"]["args"][0], "mcp");
    let chk = read_json(&home.join(".cursor/hooks.json")).unwrap();
    assert_eq!(chk["version"], 1);
    assert_eq!(chk["hooks"]["afterFileEdit"][0]["command"], "./fmt.sh");
    assert_eq!(
        chk["hooks"]["beforeSubmitPrompt"][0]["command"],
        "'/opt/km/keymule' hook prompt --agent cursor"
    );
    assert!(home.join(".cursor/hooks.json.keymule-bak").exists());

    let desk =
        read_json(&home.join("Library/Application Support/Claude/claude_desktop_config.json"))
            .unwrap();
    assert_eq!(desk["mcpServers"]["keymule"]["command"], exe);
    let ag = read_json(&home.join(".gemini/config/mcp_config.json")).unwrap();
    assert_eq!(ag["mcpServers"]["keymule"]["command"], exe);
    assert!(
        std::fs::read_to_string(home.join(".gemini/GEMINI.md"))
            .unwrap()
            .contains(MD_START)
    );

    // Again: nothing changes.
    let again = setup_others(home, exe, &all, false).unwrap();
    assert!(
        again.iter().all(|l| !l.starts_with('✓')),
        "{}",
        again.join("\n")
    );
    assert!(!again.join("\n").contains("type /hooks"));
}

#[test]
fn codex_override_file_wins_and_dry_run_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    std::fs::write(home.join(".codex/AGENTS.override.md"), "# team\n").unwrap();
    let codex = Found {
        codex: true,
        ..Found::default()
    };
    let lines = setup_others(home, "/k", &codex, true).unwrap();
    assert!(lines.iter().any(|l| l.starts_with("would")));
    assert!(!home.join(".codex/config.toml").exists());
    assert!(!home.join(".codex/hooks.json").exists());
    setup_others(home, "/k", &codex, false).unwrap();
    assert!(!home.join(".codex/AGENTS.md").exists());
    assert!(
        std::fs::read_to_string(home.join(".codex/AGENTS.override.md"))
            .unwrap()
            .contains(MD_START)
    );
}

#[test]
fn keeps_an_existing_keymule_entry_and_writes_through_symlinks() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("dotfiles-mcp.json");
    std::fs::write(&real, r#"{"mcpServers":{"other":{"command":"x"}}}"#).unwrap();
    let link = dir.path().join("mcp.json");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    assert!(install_mcp_json(&link, "/k", false).unwrap());
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        std::fs::read_to_string(&real)
            .unwrap()
            .contains("\"keymule\"")
    );
    assert!(!install_mcp_json(&link, "/other/keymule", false).unwrap());
}
