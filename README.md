# 🔑 agentkey

> Let your AI coding agents use your API tokens and passwords without ever seeing them.

---

## 🤔 Why?

You ask Claude Code to check a Bamboo pipeline, so you paste your token into the chat. That token
now sits in the model provider's logs and in your transcript files, so you should rotate it. Next
week you paste a new one, and the cycle repeats.

agentkey breaks the cycle. Tokens live in your OS keychain. The agent asks agentkey to make the
call, agentkey attaches the token itself, and the agent only ever sees the response.

```text
You:    check why my bamboo pipeline failed
Agent:  → list_credentials()            ← names and API hints only
        → http_request(credential="bamboo", path="/rest/api/latest/result/PROJ-PLAN")
                                        ← agentkey adds the token, strips it from the reply
Agent:  Build #482 failed in "Integration tests"…
```

---

## ✨ Features

- 🙈 **The agent never sees the secret.** It refers to credentials by name; the secret is attached inside agentkey and redacted from every response.
- 🔒 **Locked to hosts.** A Bamboo token only goes to your Bamboo host. Requests elsewhere are refused, and redirects to other hosts are followed *without* the token.
- ✋ **Paste-to-save.** Paste a token into Claude Code by habit and a hook catches it *before* it is sent, saves it to the keychain and blocks the message.
- 🧑‍💻 **A human approves the dangerous bits.** Pointing a credential at a new host or deleting it needs you at a real terminal, so a prompt-injected agent can't redirect your token.
- 🚫 **No reveal command.** There is deliberately no `agentkey get`; an agent with a shell could just run it.
- 📒 **Audit log.** Every request is logged to `~/.config/agentkey/audit.log` (without secrets).
- 🧩 **Service presets.** Bamboo, GitHub (incl. Actions), GitLab, Jenkins, or any HTTP API. Presets know the auth scheme, how to verify a token, and give the agent an API cheat-sheet.

---

## 🚀 Quick start

```sh
cargo install --path .        # crates.io release coming
agentkey init                 # registers the MCP server, paste hook and CLAUDE.md note
```

Restart Claude Code, then just ask:

> *connect my bamboo, it's at https://bamboo.yourcompany.com*

The agent calls `add_credential` and a macOS dialog pops up showing the host the token will be
locked to. You type the token there. It goes straight to the keychain, is verified against Bamboo,
and the agent only gets back "saved, verified as you". Then: *"check my bamboo pipeline"*.

Prefer the terminal? `agentkey add bamboo` does the same with a hidden prompt.

`agentkey init --dry-run` shows what it would change first. Your `~/.claude/settings.json` is backed
up to `settings.json.agentkey-bak`.

---

## 🛠 Commands

| Command | What it does |
|---|---|
| `agentkey add [service]` | Store a credential. Prompts for URL, username (if needed) and the secret |
| `agentkey list` | Show stored credentials (never secrets) |
| `agentkey edit <name> --url/--username/--service/--description` | Change metadata. A new host needs terminal confirmation |
| `agentkey rotate <name>` | Replace the secret after rotating it |
| `agentkey test <name>` | Check the credential still works |
| `agentkey rm <name>` | Delete it (needs terminal confirmation) |
| `agentkey presets` | List service presets |
| `agentkey init` | Set up Claude Code |
| `agentkey mcp` | The MCP server (started by your agent) |
| `agentkey hook prompt` | The Claude Code `UserPromptSubmit` hook |

### MCP tools

| Tool | |
|---|---|
| `list_credentials` | Names, service, base URL, allowed hosts, API hints. No secrets |
| `http_request` | `credential`, `method`, `path`, `query`, `headers`, `body`, `max_bytes`, `keep_tail` |
| `add_credential` | Opens a secure dialog for the user to type the secret; verifies and stores it |
| `rotate_credential` | Same dialog, for a new token on an existing credential |
| `update_credential` | Change URL, username or description; a new host needs the user's approval in a dialog |

The secret never travels through the MCP connection. There is no tool that takes or returns one.

### Other agents

`agentkey init` prints config snippets for Codex and Cursor. The MCP server works there; the
paste-to-save hook is Claude Code only for now.

---

## 🧱 How it's stored

| What | Where |
|---|---|
| Secrets | OS keychain (macOS Keychain, Windows Credential Manager, Secret Service), service `agentkey` |
| Metadata | `~/.config/agentkey/credentials.toml` (no secrets) |
| Audit log | `~/.config/agentkey/audit.log` |

On macOS, the keychain only hands agentkey's items to agentkey itself. Another process (say, an
agent running `security find-generic-password`) triggers a system prompt you have to approve.

---

## ⚠️ Limits (v0.1)

- Paste detection is reliable for prefixed tokens (`ghp_`, `github_pat_`, `glpat-`, `sk-`, `AKIA`…). Unprefixed tokens such as Bamboo's are caught when the message gives context ("my bamboo token is …").
- HTTP APIs only. Database, SSH and cloud CLI credentials are on the roadmap.
- The secure dialog is macOS-only for now; elsewhere use `agentkey add` in a terminal.
- The agent picks the URL in `add_credential`, so read the host in the dialog before typing your token.
- Unsigned builds may make macOS ask for keychain access again after each rebuild.

## 🗺 Roadmap

- **0.2**: `agentkey doctor` (find leaked secrets in MCP configs, `.env` files and agent transcripts), move plain-text tokens in MCP configs into agentkey, more presets
- **0.3**: database auth proxy, SSH agent, git/docker/AWS credential helpers
- **0.4**: Touch ID approval for sensitive credentials, short-lived credentials (AWS STS, GitHub App tokens)

## 📄 License

MIT OR Apache-2.0
