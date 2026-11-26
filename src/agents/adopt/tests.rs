
use super::*;
use crate::store::testing::MemVault;

const KEY: &str = "ak_9fK2mQ7vX4pL8nR3tW6yB1cD5hG0sA2eZ";
const DB: &str = "postgres://app:Vq7Lp2Xk9Rt4@localhost:5432/app";

fn fake_home() -> (tempfile::TempDir, Options) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let claude = serde_json::json!({
        "numStartups": 3,
        "mcpServers": {
            "db": {"type": "stdio", "command": "postgres-mcp", "args": ["--access-mode=unrestricted"],
                   "env": {"DATABASE_URI": DB}},
            "remote": {"type": "http", "url": "https://x.example.com/mcp",
                       "headers": {"x-api-key": "k_8Hq2Vn5Rx7Lm3Pz9Tc4Wb6Yd1", "X-Client": "test"}},
            "open": {"type": "http", "url": "https://open.example.com/mcp"},
            "plain": {"type": "stdio", "command": "npx", "args": ["-y", "plain-mcp"], "env": {}}
        },
        "projects": {
            "/work/a": {"mcpServers": {"acme": {"type": "stdio", "command": "npx",
                "args": ["-y", "acme-mcp"], "env": {"ACME_API_URL": "https://acme.dev", "ACME_API_KEY": KEY}}}},
            "/work/b": {"mcpServers": {"acme": {"type": "stdio", "command": "npx",
                "args": ["-y", "acme-mcp"], "env": {"ACME_API_KEY": KEY}}}},
            "/work/c": {"mcpServers": {"notes": {"type": "stdio",
                "command": format!("NOTES_TOKEN={KEY}x NOTES_PAGE=a1b2c3 npx notes-mcp"), "args": [], "env": {}}}}
        }
    });
    std::fs::write(
        home.join(".claude.json"),
        serde_json::to_string_pretty(&claude).unwrap() + "\n",
    )
    .unwrap();
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    std::fs::write(
            home.join(".codex/config.toml"),
            format!(
                "# my codex config\nmodel = \"x\"\n\n[mcp_servers.acme]\ncommand = \"npx\"\nargs = [\"-y\", \"acme-mcp\"]\n\n[mcp_servers.acme.env]\nACME_API_KEY = \"{KEY}\"\n\n[mcp_servers.docs]\nurl = \"https://docs.example.com/mcp\"\nhttp_headers = {{ Authorization = \"Bearer tok_4Rv8Qm2Lx7Wn3Kp9Zt6Hc1\" }}\n"
            ),
        )
        .unwrap();
    let opts = Options {
        dir: home.join("project"),
        home,
        exe: "/bin/keymule".into(),
        force: Vec::new(),
    };
    (dir, opts)
}

#[test]
fn plans_and_applies() {
    let (dir, opts) = fake_home();
    let mut store = Store::load_from(dir.path().join("cfg/credentials.toml")).unwrap();
    let vault = MemVault::default();

    let plan = plan(&opts, &store, &vault).unwrap();
    let mut got: Vec<_> = plan
        .targets
        .iter()
        .map(|t| format!("{} → {}", t.place, t.binding))
        .collect();
    got.sort();
    assert_eq!(
        got,
        [
            "~/.claude.json · db → db",
            "~/.claude.json · project /work/a: acme → acme",
            "~/.claude.json · project /work/b: acme → acme",
            "~/.claude.json · project /work/c: notes → notes",
            "~/.claude.json · remote → remote",
            "~/.codex/config.toml · acme → acme",
            "~/.codex/config.toml · docs → docs",
        ]
    );
    assert!(plan.skipped.is_empty(), "{:?}", plan.skipped);
    let text = plan.render();
    assert!(
        !text.contains(KEY) && !text.contains("Vq7Lp2Xk9Rt4") && !text.contains("8Hq2Vn5Rx7Lm3Pz9"),
        "{text}"
    );
    assert!(
        text.contains("remote remote https://x.example.com/mcp"),
        "{text}"
    );

    let applied = apply(&plan, &mut store, &vault, &opts.home).unwrap();
    assert_eq!(applied.bindings, ["acme", "db", "docs", "notes", "remote"]);
    assert_eq!(applied.backups.len(), 2);

    let claude = std::fs::read_to_string(opts.home.join(".claude.json")).unwrap();
    assert!(!claude.contains(KEY) && !claude.contains("Vq7Lp2Xk9Rt4"));
    let v: Value = serde_json::from_str(&claude).unwrap();
    let acme = &v["projects"]["/work/a"]["mcpServers"]["acme"];
    assert_eq!(acme["command"], "/bin/keymule");
    assert_eq!(
        acme["args"],
        serde_json::json!(["exec", "acme", "--", "npx", "-y", "acme-mcp"])
    );
    assert_eq!(
        acme["env"],
        serde_json::json!({"ACME_API_URL": "https://acme.dev"})
    );
    let notes = &v["projects"]["/work/c"]["mcpServers"]["notes"];
    assert_eq!(
        notes["args"],
        serde_json::json!(["exec", "notes", "--", "npx", "notes-mcp"])
    );
    assert_eq!(notes["env"], serde_json::json!({"NOTES_PAGE": "a1b2c3"}));
    assert_eq!(v["mcpServers"]["plain"]["command"], "npx");
    let remote = &v["mcpServers"]["remote"];
    assert_eq!(
        remote,
        &serde_json::json!({"type": "stdio", "command": "/bin/keymule",
                "args": ["remote", "remote", "https://x.example.com/mcp", "--header=X-Client: test"]})
    );
    assert_eq!(
        v["mcpServers"]["open"]["url"],
        "https://open.example.com/mcp"
    );
    let b = Binding::load(&vault, "remote").unwrap();
    assert_eq!(b.headers["x-api-key"], "k_8Hq2Vn5Rx7Lm3Pz9Tc4Wb6Yd1");
    assert_eq!(b.urls, ["https://x.example.com/mcp"]);
    assert_eq!(store.servers["remote"].headers, ["x-api-key"]);
    assert_eq!(v["numStartups"], 3);

    let codex = std::fs::read_to_string(opts.home.join(".codex/config.toml")).unwrap();
    assert!(
        codex.starts_with("# my codex config\nmodel = \"x\"\n"),
        "{codex}"
    );
    assert!(
        !codex.contains(KEY) && !codex.contains("[mcp_servers.acme.env]"),
        "{codex}"
    );
    assert!(codex.contains(r#"args = ["exec", "acme", "--", "npx", "-y", "acme-mcp"]"#));
    assert!(
        !codex.contains("tok_4Rv8")
            && !codex.contains("docs.example.com/mcp\"\nhttp")
            && codex.contains(r#"args = ["remote", "docs", "https://docs.example.com/mcp"]"#),
        "{codex}"
    );
    assert_eq!(
        Binding::load(&vault, "docs").unwrap().headers["Authorization"],
        "Bearer tok_4Rv8Qm2Lx7Wn3Kp9Zt6Hc1"
    );

    let acme = Binding::load(&vault, "acme").unwrap();
    assert_eq!(acme.env["ACME_API_KEY"], KEY);
    assert!(
        acme.command("acme", &["npx".into(), "-y".into(), "acme-mcp".into()])
            .is_ok()
    );
    assert_eq!(Binding::load(&vault, "db").unwrap().env["DATABASE_URI"], DB);
    assert_eq!(store.servers["acme"].used_by.len(), 3);

    // Running it again finds nothing more to do.
    let again = super::plan(&opts, &store, &vault).unwrap();
    assert!(again.targets.is_empty(), "{:?}", again.fingerprint());
}

#[test]
fn same_name_different_secret_gets_its_own_binding() {
    let (dir, opts) = fake_home();
    let path = opts.home.join(".claude.json");
    let text =
        std::fs::read_to_string(&path)
            .unwrap()
            .replacen(KEY, "ak_0Zt5Wq8Rm2Nx6Kp4Lv7Hc3Bd9Fs", 1);
    std::fs::write(&path, text).unwrap();
    let store = Store::load_from(dir.path().join("cfg/credentials.toml")).unwrap();
    let plan = plan(&opts, &store, &MemVault::default()).unwrap();
    let mut names: Vec<_> = plan.targets.iter().map(|t| t.binding.as_str()).collect();
    names.sort();
    names.dedup();
    assert_eq!(names, ["acme", "acme-2", "db", "docs", "notes", "remote"]);
}

#[test]
fn names_and_shell_lines() {
    assert_eq!(binding_name("@magicuidesign/mcp"), "magicuidesign-mcp");
    assert_eq!(binding_name("pg-mcp-local"), "pg-mcp-local");
    let server = serde_json::json!({"command": format!("TOKEN='{KEY}' npx x"), "args": []});
    assert!(matches!(analyze(&server, false), Analysis::Skip(_)));
    let reference = serde_json::json!({"command": "npx", "env": {"API_KEY": "${API_KEY}"}});
    assert!(matches!(analyze(&reference, false), Analysis::Nothing));
    assert!(matches!(analyze(&reference, true), Analysis::Nothing));

    // A local database with a default password is left alone unless named.
    let local = serde_json::json!({"command": "postgres-mcp",
            "env": {"DATABASE_URI": "postgres://postgres:postgres@127.0.0.1:5432/app", "MODE": "ro"}});
    assert!(matches!(analyze(&local, false), Analysis::Nothing));
    match analyze(&local, true) {
        Analysis::Adopt(t) => {
            assert_eq!(t.secret_env.keys().collect::<Vec<_>>(), ["DATABASE_URI"]);
            assert_eq!(t.keep_env.get("MODE").and_then(Value::as_str), Some("ro"));
        }
        _ => panic!("named server should be adopted"),
    }
}
