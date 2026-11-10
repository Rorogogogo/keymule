# 🫏 keymule

> Let your AI agents use your API tokens and passwords without ever seeing them. keymule carries the keys; the agent never opens the pack.

[![CI](https://img.shields.io/github/actions/workflow/status/Rorogogogo/keymule/ci.yml?branch=main&label=CI)](https://github.com/Rorogogogo/keymule/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#-license)

---

## 🤔 Why?

You ask Claude Code to check a Bamboo pipeline, so you paste your token into the chat. That token
now sits in the model provider's logs and in your transcript files, so you should rotate it. Next
week you paste a new one, and the cycle repeats.

keymule breaks the cycle. Tokens live in your OS keychain. The agent asks keymule to make the
call, keymule attaches the token itself, and the agent only ever sees the response.

```text
You:    check why my bamboo pipeline failed
Agent:  → list_credentials()            ← names and API hints only
        → http_request(credential="bamboo", path="/rest/api/latest/result/PROJ-PLAN")
                                        ← keymule adds the token, strips it from the reply
Agent:  Build #482 failed in "Integration tests"…
```

---

## ✨ Features

- 🙈 **The agent never sees the secret.** It refers to credentials by name; the secret is attached inside keymule and redacted from every response.
- 🔒 **Locked to hosts.** A Bamboo token only goes to your Bamboo host. Requests elsewhere are refused, and redirects to other hosts are followed *without* the token. The allowed hosts are sealed in the keychain with the token, so editing keymule's files doesn't change them.
- ✋ **Paste-to-save.** Paste a token into Claude Code by habit and a hook catches it *before* it is sent, saves it to the keychain and blocks the message.
- 🧑‍💻 **A human approves the dangerous bits.** Pointing a credential at a new host or deleting it needs you at a real terminal, so a prompt-injected agent can't redirect your token.
- 🚫 **No reveal command.** There is deliberately no `keymule get`; an agent with a shell could just run it.
- 📒 **Audit log.** Every request is logged to `~/.config/keymule/audit.log` (without secrets).
- 🧩 **Service presets.** Bamboo, GitHub (incl. Actions), GitLab, Jenkins, or any HTTP API. Presets know the auth scheme, how to verify a token, and give the agent an API cheat-sheet.
- 🩺 **Leak doctor.** `keymule doctor` finds tokens that already leaked into agent transcripts, sit in plain text in agent configs, or are readable in `.env` files and shell history, so you know what to rotate.
- 📦 **Adopt MCP secrets.** `keymule adopt` moves the API keys your MCP server configs keep in plain text into the keychain. The servers start the same way, through `keymule exec`.

---

## 🩺 Find what already leaked

Example output (made-up values):

```text
$ keymule doctor
keymule doctor: scanned 48 agent transcripts (390 MB), 5 agent configs, 2 .env files, 1 shell histories

🔴 Sent to an AI model: rotate these (2)
  sk_l…7Qa     Stripe live key
      seen in       Codex session in ~/code/shop (2026-03-02)
  ac_4…k9P     ACME_API_KEY
      seen in       Claude Code session in ~/code/api ×3 (latest 2026-04-11)
      plain text in ~/.claude.json · acme.env.ACME_API_KEY

🟠 Plain text in agent configs: move these into keymule (1)
  ghp_…T4x     GitHub token
      plain text in ~/.cursor/mcp.json · github.env.GITHUB_TOKEN
```

It reads `~/.claude/projects`, `~/.codex/sessions`, the Claude Code, Codex, Cursor and Claude
Desktop configs, `.env` files in the current project and your home folder, and shell history. A
few hundred MB takes a few seconds. Secrets are always masked and nothing leaves your machine.
`--all` also lists unsure matches, and `--json` gives machine-readable output. Agents can run the
same scan with the `scan_for_leaks` MCP tool: *"did any of my tokens leak?"*

---

## 📦 Get secrets out of MCP configs

Most MCP servers want their API key in an `env` block of `~/.claude.json`, Cursor's `mcp.json`
or Codex's `config.toml`, where every agent can read it. `keymule adopt` moves them into the
keychain (example output, made-up values):

```text
$ keymule adopt --dry-run
2 MCP server entries in 2 config files keep secrets in plain text:

  acme   ACME_API_KEY=ak_4…k9P
      ~/.claude.json · project ~/code/api: acme
      ~/.codex/config.toml · acme

Each entry will start the same command through keymule, which adds the secrets. For example:
  before: npx -y acme-mcp
  after:  ~/.cargo/bin/keymule exec acme -- npx -y acme-mcp
```

Run it without `--dry-run` to apply (it asks first and backs up each file it rewrites). It handles
`env` blocks, inline `KEY=value npx …` commands and keys passed as arguments. Non-secret settings
and `${VAR}` references stay where they are.

The secrets and the exact command lines allowed to receive them are sealed together in one
keychain item. `keymule exec acme -- env` is refused, and editing the config to point at another
program doesn't get the key either. `keymule rotate acme` stores a new value.

---

## 🚀 Quick start

```sh
cargo install --path .        # crates.io release coming
keymule init                 # registers the MCP server, paste hook and CLAUDE.md note
```

Restart Claude Code, then just ask:

> *connect my bamboo, it's at https://bamboo.yourcompany.com*

The agent calls `add_credential` and a macOS dialog pops up showing the host the token will be
locked to. You type the token there. It goes straight to the keychain, is verified against Bamboo,
and the agent only gets back "saved, verified as you". Then: *"check my bamboo pipeline"*.

Prefer the terminal? `keymule add bamboo` does the same with a hidden prompt.

`keymule init --dry-run` shows what it would change first. Your `~/.claude/settings.json` is backed
up to `settings.json.keymule-bak`.

---

## 🛠 Commands

| Command | What it does |
|---|---|
| `keymule add [service] [--dialog]` | Store a credential. Prompts for URL, username (if needed) and the secret; `--dialog` asks for the secret in the macOS dialog |
| `keymule list [--json]` | Show stored credentials and adopted MCP servers (never secrets) |
| `keymule edit <name> --url/--username/--service/--description` | Change metadata. A new host needs terminal confirmation |
| `keymule rotate <name> [--dialog]` | Replace the secret after rotating it |
| `keymule test <name>` | Check the credential still works |
| `keymule rm <name>` | Delete it (needs terminal confirmation) |
| `keymule presets` | List service presets |
| `keymule doctor [--all] [--json] [--dir D]` | Find leaked and plain-text secrets |
| `keymule adopt [--dry-run] [--yes]` | Move plain-text MCP server secrets into the keychain |
| `keymule exec <name> -- <command…>` | Start an adopted MCP server with its secrets (used in configs) |
| `keymule init` | Set up Claude Code |
| `keymule mcp` | The MCP server (started by your agent) |
| `keymule hook prompt` | The Claude Code `UserPromptSubmit` hook |

### MCP tools

| Tool | |
|---|---|
| `list_credentials` | Names, service, base URL, allowed hosts, API hints. No secrets |
| `http_request` | `credential`, `method`, `path`, `query`, `headers`, `body`, `max_bytes`, `keep_tail` |
| `add_credential` | Opens a secure dialog for the user to type the secret; verifies and stores it |
| `rotate_credential` | Same dialog, for a new token on an existing credential |
| `update_credential` | Change URL, username or description; a new host needs the user's approval in a dialog |
| `scan_for_leaks` | Run the doctor and return masked results |

The secret never travels through the MCP connection. There is no tool that takes or returns one.

### Other agents

`keymule init` prints config snippets for Codex and Cursor. The MCP server works there; the
paste-to-save hook is Claude Code only for now.

---

## 🧱 How it's stored

| What | Where |
|---|---|
| Secrets, with their allowed hosts, auth scheme and (for MCP servers) allowed commands | OS keychain (macOS Keychain, Windows Credential Manager, Secret Service), service `keymule` |
| Readable index | `~/.config/keymule/credentials.toml`: names, URLs, descriptions. No secrets, and not trusted: if it disagrees with the keychain, the keychain wins and `keymule list` warns you |
| Audit log | `~/.config/keymule/audit.log`: one line per request, and one when an adopted MCP server starts |

On macOS, the keychain only hands keymule's items to keymule itself. Another process (say, an
agent running `security find-generic-password`) triggers a system prompt you have to approve.

---

## ⚠️ Limits

- Paste detection is reliable for prefixed tokens (`ghp_`, `github_pat_`, `glpat-`, `sk-`, `AKIA`…). Unprefixed tokens such as Bamboo's are caught when the message gives context ("my bamboo token is …").
- HTTP APIs only. Database, SSH and cloud CLI credentials are on the roadmap.
- The secure dialog is macOS-only for now; elsewhere use `keymule add` in a terminal.
- The agent picks the URL in `add_credential`, so read the host in the dialog before typing your token.
- Unsigned builds may make macOS ask for keychain access again after each rebuild.
- `keymule adopt` skips HTTP MCP servers (secret headers), project `.mcp.json` files (usually shared; use `${VAR}` there) and command lines with shell syntax.
- `keymule exec` stops casual reveals and keeps keys out of files, but an agent that can run arbitrary code as you could still get at a server's environment (for example by changing the server's own code).

## 🗺 Roadmap

- **0.2**: ✅ `keymule doctor`
- **0.3**: ✅ `keymule adopt` and `keymule exec`. Next: secret headers for HTTP MCP servers, more presets
- **0.4**: database auth proxy, SSH agent, git/docker/AWS credential helpers
- **0.5**: Touch ID approval for sensitive credentials, short-lived credentials (AWS STS, GitHub App tokens)

## 📄 License

MIT OR Apache-2.0
