# 🫏 keymule

> Let your AI agents use your API tokens and passwords without ever seeing them. keymule carries the keys; the agent never opens the pack.

[![CI](https://img.shields.io/github/actions/workflow/status/Rorogogogo/keymule/ci.yml?branch=main&label=CI)](https://github.com/Rorogogogo/keymule/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#-license)

---

## 🤔 Why?

You ask Claude Code to check a Bamboo pipeline, so you paste your token into the chat. That token
now sits in the model provider's logs and in your transcript files, so you should rotate it. Next
week you paste a new one, and the cycle repeats.

keymule breaks the cycle. Tokens live in an encrypted vault whose key is in your OS keychain. The agent asks keymule to make the
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
- ✋ **Paste-to-save.** Paste a token into Claude Code by habit and a hook catches it *before* it is sent and asks, right in the terminal: type `y` to save it under a name taken from your message (`github-launchstory` for "the PAT for our launchstory account"; a bare Stripe key pasted in your `LaunchStory` repo becomes `stripe-launchstory`), with your words as its description so the agent picks the right one later, or `y <name>` and your message goes on with the token swapped for its keymule name, or `n` to send it as it was.
- 🧑‍💻 **A human approves the dangerous bits.** Pointing a credential at a new host or deleting it needs you at a real terminal, so a prompt-injected agent can't redirect your token.
- 🚫 **No reveal command.** There is deliberately no `keymule get`; an agent with a shell could just run it.
- 📒 **Audit log.** Every request is logged to `~/.config/keymule/audit.log` (without secrets).
- 🧩 **Service presets.** Bamboo, GitHub (incl. Actions), GitLab, Jenkins, Stripe, OpenAI, or any HTTP API. Presets know the auth scheme, how to verify a token, and give the agent an API cheat-sheet.
- 🩺 **Leak doctor.** `keymule doctor` finds tokens that already leaked into agent transcripts, sit in plain text in agent configs, or are readable in `.env` files and shell history, so you know what to rotate.
- 🖥 **Local web page.** `keymule ui` opens a page on your machine with your credentials, MCP servers, live activity and leak scan, with buttons that open keymule's dialogs. **Reveal** shows a stored value in a keymule window when you need to see it yourself, never on the page or in a terminal.
- 📦 **Adopt MCP secrets.** `keymule adopt` moves the API keys your MCP server configs keep in plain text into the keychain. The servers start the same way, through `keymule exec`. Remote (HTTP) servers whose API key sits in a header connect through `keymule remote`, which adds the header itself.

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
and `${VAR}` references stay where they are. Default-looking passwords (a local database's
`postgres`, say) are skipped unless you name the server: `keymule adopt --server my-db`.

The secrets and the exact command lines allowed to receive them are sealed together in one
keychain item. A command keymule hasn't seen (`keymule exec acme -- env`, or a config edited to
point at another program) only gets the key after you approve it in a dialog. `keymule rotate acme`
stores a new value.

Remote (HTTP) MCP servers with a key in their `headers` become a small local bridge:

```text
  before: {"type": "http", "url": "https://mcp.acme.dev/mcp", "headers": {"x-api-key": "ak_…"}}
  after:  {"type": "stdio", "command": "~/.cargo/bin/keymule", "args": ["remote", "acme", "https://mcp.acme.dev/mcp"]}
```

`keymule remote` speaks MCP over stdio to your agent and Streamable HTTP to the server, adding the
header to each request. It only sends it to the URLs you approved (https, or http on localhost),
never follows redirects with it, and blanks the key out of anything the server sends back.
Non-secret headers stay in the config as `--header="Name: value"`.

### New MCP servers, without the password ever reaching the chat

> *add a postgres MCP for my dev database*

The agent calls `add_mcp_secret(name="pg-dev", env="DATABASE_URI")`, you type the URL into
keymule's dialog, and the agent configures the server as
`keymule exec pg-dev -- postgres-mcp --access-mode=restricted`. When it first starts, keymule shows
you that command and asks whether it may have the secret. If you paste a database URL into chat
by habit, the paste hook holds it back and, after you type `y`, saves it the same way (`keymule exec postgres --env
DATABASE_URI -- …`). For a remote server the agent calls
`add_mcp_secret(name="acme", header="x-api-key")` and sets it up as `keymule remote acme <url>`;
you approve the URL once, the first time it connects.

---

## 🚀 Quick start

```sh
cargo install --path .        # crates.io release coming
keymule init                 # sets up every agent it finds: MCP server, paste hook, instructions
```

Restart your agent (Claude Code, Codex, Cursor, Claude Desktop or Antigravity), then just ask:

> *connect my bamboo, it's at https://bamboo.yourcompany.com*

The agent calls `add_credential` and a macOS dialog pops up showing the host the token will be
locked to. You type the token there. It goes straight to the keychain, is verified against Bamboo,
and the agent only gets back "saved, verified as you". Then: *"check my bamboo pipeline"*.

Prefer the terminal? `keymule add bamboo` does the same with a hidden prompt.

`keymule init --dry-run` shows what it would change first. Every config file it edits is backed
up next to itself as `<name>.keymule-bak`.

---

## 🛠 Commands

| Command | What it does |
|---|---|
| `keymule add [service] [--dialog]` | Store a credential. Prompts for URL, username (if needed) and the secret; `--dialog` asks for the secret in the macOS dialog |
| `keymule list [--json]` | Show stored credentials and adopted MCP servers (never secrets) |
| `keymule edit <name> --url/--username/--service/--description` | Change metadata. A new host needs terminal confirmation |
| `keymule rotate <name> [--dialog]` | Replace the secret after rotating it |
| `keymule reveal <name>` | Show a secret in a macOS window after you agree (never printed, so agents running it can't read it); Copy clears the clipboard after 30 s |
| `keymule test <name>` | Check the credential still works |
| `keymule migrate` | Move secrets from an older keymule's keychain items into the encrypted vault |
| `keymule rm <name> [--dialog]` | Delete it (asks you first, on the terminal or in a dialog; the web page's Delete uses the dialog) |
| `keymule presets` | List service presets |
| `keymule doctor [--all] [--json] [--dir D]` | Find leaked and plain-text secrets |
| `keymule adopt [--dry-run] [--yes] [--server NAME]` | Move plain-text MCP server secrets into the keychain |
| `keymule exec <name> [--env VAR] -- <command…>` | Start an MCP server with its secrets (used in configs), or run a CLI tool with a stored credential in an env var. New commands need your approval once |
| `keymule install <name> --host H --username U [--dir D]` | Copy a credential into git's credential store, so git uses it for `https://U@H`; the same dialog lets the service's CLI (gh) use it in the repo folder |
| `keymule uninstall <name> [--host H] [--username U]` | Remove those copies again (the credential stays in keymule) |
| `keymule gh-secret --repo O/R SECRET=name… [--via gh\|gh:LOGIN\|CRED] [--dry-run]` | Copy credentials into GitHub Actions secrets for CI, all in one dialog: encrypted with the repo's key, uploaded as the account `gh` is signed in as (or a stored GitHub credential) after keymule checks it has admin on the repo |
| `keymule allow [--off [--everywhere]] [--status]` | Allow all: one dialog, then keymule stops asking in this repo (any program including shells, gh-secret, new hosts, reveal, the read-back guard, pasted secrets saved without asking). Deleting still asks; every skipped dialog is logged |
| `keymule remote <name> <url> [--header "Name: value"]` | Connect to a remote (HTTP) MCP server, adding the headers keymule holds (used in configs). New URLs need your approval once |
| `keymule ui [--no-open] [--port N]` | Open the local web page (127.0.0.1 only, token in the link it opens) |
| `keymule init [--dry-run]` | Set up every agent found (see [Agents](#agents)) |
| `keymule mcp` | The MCP server (started by your agent) |
| `keymule hook prompt [--agent claude\|codex\|cursor]` | The paste-to-save prompt hook |
| `keymule hook bash [--agent claude\|codex\|cursor]` | The guard that stops agents reading tokens back out of git, gh or the keychain |

### MCP tools

| Tool | |
|---|---|
| `list_credentials` | Names, service, base URL, allowed hosts, API hints. No secrets |
| `http_request` | `credential`, `method`, `path`, `query`, `headers`, `body`, `max_bytes`, `keep_tail` |
| `add_credential` | Opens a secure dialog for the user to type the secret; verifies and stores it |
| `rotate_credential` | Same dialog, for a new token on an existing credential |
| `update_credential` | Change URL, username or description; a new host needs the user's approval in a dialog |
| `install_credential` | Copy a credential into git's credential store for a host and account, after the user approves in a dialog |
| `add_mcp_secret` | Store a secret an MCP server needs (database URL, API key, header) via the dialog, for `keymule exec` or `keymule remote` |
| `scan_for_leaks` | Run the doctor and return masked results |

The secret never travels through the MCP connection. There is no tool that takes or returns one.

### CLI tools and git

Some jobs aren't one HTTP call: `gh pr create`, `git push`. keymule covers them without the
token passing through the chat or your clipboard.

```sh
# gh, glab and other CLIs: the token goes into the env var the tool reads
keymule exec github-acme --env GH_TOKEN -- gh pr create --repo acme/shop --fill

# git over HTTPS: copy the token into git's own credential store, once.
# Run in the repo: the same dialog also lets gh use it in this folder.
keymule install github-acme --host github.com --username acme-bot
git remote set-url origin https://acme-bot@github.com/acme/shop.git

# CI: put stored secrets into the repo's Actions secrets (encrypted locally).
# One dialog for all of them; uploads with your existing gh login.
keymule gh-secret --repo acme/ext CWS_CLIENT_ID=cws-id CWS_CLIENT_SECRET=cws-secret \
  CWS_REFRESH_TOKEN=cws-refresh
```

`gh-secret` asks GitHub who each login is before it asks you, so the dialog names the real
account: *"Copy 3 secrets from keymule into acme/ext's GitHub Actions secrets? … Uploaded as
acme (admin there), using your gh login."* A token for the wrong account is caught up front,
with what each login can do on the repo. It borrows gh's token inside keymule for those
api.github.com calls only; it never prints or stores it, and you don't need a second GitHub
token in keymule. `--dry-run` shows who would upload and which secrets would be replaced.

Setting up a repo is one dialog: *"Let git use github-acme for github.com as acme-bot? … gh may
use it in ~/code/shop too."* After that nothing asks again for that repo, including its
subfolders.

- **exec** asks once per program and folder (subfolders included), in a dialog. It refuses
  shells, interpreters and tools that just print their environment (`sh`, `python`, `env`,
  `cat`…), including scripts run by one. It scrubs the token from the tool's output, as is,
  URL-encoded or base64. The tool itself does get the token, so only allow tools you trust.
- **install** runs `git credential approve` itself, with the token on stdin, so it lands in
  whatever helper git uses (osxkeychain, Git Credential Manager…). keymule remembers the copy:
  `rotate` updates it, `rm` and `uninstall` erase it. Installing the same copy again asks
  nothing. Repos whose remote URL names another account keep using that one. To keep your usual
  account the default everywhere else, set
  `git config --global credential.https://github.com.username <you>` once.
- Once the token is in git's store, anything you run could ask git for it. keymule's agent hook
  (`keymule hook bash`) blocks the usual ways to read it back: `git credential fill`, asking a
  helper directly, `security find-…-password -w`, `gh auth token`, and `~/.git-credentials` or gh's
  `hosts.yml`. It always blocks them, installed or not. It matches command text (skipping
  heredocs that only write a file), so it's a guard rail, not a wall.

A GitHub credential at `api.github.com` may also be used for `github.com` and
`uploads.github.com`.

### Agents

`keymule init` sets up each agent it finds on your machine:

| Agent | MCP server | Paste-to-save hook | Instructions |
|---|---|---|---|
| Claude Code | `claude mcp add` (user scope) | `~/.claude/settings.json` (+ read-back guard) | `~/.claude/CLAUDE.md` |
| Codex | `~/.codex/config.toml` | `~/.codex/hooks.json` (+ read-back guard) | `~/.codex/AGENTS.md` |
| Cursor | `~/.cursor/mcp.json` | `~/.cursor/hooks.json` (+ read-back guard) | printed for Customize → Rules |
| Claude Desktop | `claude_desktop_config.json` | none (no prompt hook) | none |
| Antigravity | `~/.gemini/config/mcp_config.json` | none (no prompt hook) | `~/.gemini/GEMINI.md` |

Codex runs a new hook only after you trust it: type `/hooks` in Codex once. Codex `$CODEX_HOME`
and `AGENTS.override.md` are respected.

Agents differ in what a hook can do. Claude Code and Codex hold the message, ask, and on `y` send
it on with the secret swapped for its keymule name. Cursor can only stop a message, so on `y`
keymule saves the secret and hands the message back, ready to send again without it.

---

## 🧱 How it's stored

| What | Where |
|---|---|
| Secrets, with their allowed hosts, auth scheme and (for MCP servers) allowed commands | `~/.config/keymule/vault.enc`, encrypted with XChaCha20-Poly1305, readable only by you |
| The vault's key | One item in the OS keychain (macOS Keychain, Windows Credential Manager, Secret Service): service `keymule`, account `vault-key`. Without a keychain (some Linux, WSL, CI), set `KEYMULE_PASSPHRASE` and the key is derived from it with Argon2id |
| Readable index | `~/.config/keymule/credentials.toml`: names, URLs, descriptions. No secrets, and not trusted: if it disagrees with the vault, the vault wins and `keymule list` warns you |
| Audit log | `~/.config/keymule/audit.log`: one line per request, and one when an adopted MCP server starts |

On macOS, the keychain only hands the vault's key to keymule itself. Another process (say, an
agent running `security find-generic-password`) triggers a system prompt you have to approve, and
the vault file is useless without that key. Because there's one keychain item, not one per
secret, macOS asks about it once. No admin rights are needed: the key sits in your own login
keychain. Secrets an older keymule kept as separate keychain items move into the vault the first
time they're used, or all at once with `keymule migrate`.

---

## ⚠️ Limits

- A Claude Code hook can't edit a message, only stop it, so the paste hook holds the message in the keychain for up to 15 minutes until you answer `y` or `n`, and then hands it to the agent with your answer.
- Paste detection is reliable for prefixed tokens (`ghp_`, `github_pat_`, `glpat-`, `sk-`, `AKIA`…) and database URLs with a password. Unprefixed tokens such as Bamboo's are caught when the message gives context ("my bamboo token is …"). Default passwords like `postgres:postgres@localhost` pass through.
- HTTP APIs only. Database, SSH and cloud CLI credentials are on the roadmap.
- The secure dialog is macOS-only for now; elsewhere use `keymule add` in a terminal.
- The agent picks the URL in `add_credential`, so read the host in the dialog before typing your token.
- Unsigned builds make macOS ask for access to the vault key again after each rebuild (once, not once per secret). Building from source on a Mac, `scripts/install-macos.sh` signs the binary with a local certificate so "Always Allow" sticks (setup steps are in the script).
- `keymule ui` never sees a secret, but anything that can read your terminal output can open it with the link it prints.
- `keymule reveal` asks before it shows anything, but the value is then on your screen: anyone watching it or recording it sees it, and Copy puts it on the clipboard for 30 seconds.
- `keymule remote` speaks Streamable HTTP, not the older SSE-only transport. Keys in a URL's query string aren't adopted.
- `keymule adopt` skips project `.mcp.json` files (usually shared; use `${VAR}` there) and command lines with shell syntax.
- `keymule exec` stops casual reveals and keeps keys out of files, but an agent that can run arbitrary code as you could still get at a server's environment (for example by changing the server's own code).

## 🗺 Roadmap

- **0.2**: ✅ `keymule doctor`
- **0.3**: ✅ `keymule adopt` and `keymule exec`
- **0.4**: ✅ `add_mcp_secret`, approval for new server commands, pasted database URLs
- **0.5**: ✅ `keymule remote` for HTTP MCP servers' key headers
- **0.6**: ✅ `keymule ui`, a local web page; `keymule reveal`
- **0.7**: ✅ one encrypted vault, one keychain item (passphrase where there's no keychain); paste hook asks inline through the Claude Code mod
- **Next**: more presets, database auth proxy, SSH agent, git/docker/AWS credential helpers
- **Later**: Touch ID approval for sensitive credentials, short-lived credentials (AWS STS, GitHub App tokens)

## 📄 License

MIT OR Apache-2.0
