# 🫏 keymule

> Let your AI agents use your API tokens and passwords without ever seeing them. keymule carries the keys; the agent never opens the pack.

[![CI](https://img.shields.io/github/actions/workflow/status/Rorogogogo/keymule/ci.yml?branch=main&label=CI)](https://github.com/Rorogogogo/keymule/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#-license)

https://github.com/user-attachments/assets/7a568e4a-df78-4e66-bde9-eceb996ad39d

## 🚀 Get started

**1. Install and set up** (macOS or Linux, no Rust needed):

```sh
curl -fsSL https://raw.githubusercontent.com/Rorogogogo/keymule/main/install.sh | sh
```

It installs keymule, then runs `keymule init`, which finds Claude Code, Codex, Cursor, Claude
Desktop and Antigravity and gives each one keymule's MCP server, the paste-to-save hook and a short
note on how to use keymule. Every file it edits is backed up first. On a Mac it also puts keymule
in the menu bar; when macOS asks whether keymule may use your keychain, enter your password and
click **Always Allow**. In a Linux container without a keychain, it tells you to set
`KEYMULE_PASSPHRASE` first ([details](#-linux-and-dev-containers)).

With Homebrew: `brew install rorogogogo/tap/keymule && keymule init`. To install without setting
anything up (CI, images), add `KEYMULE_NO_INIT=1`: `curl … | KEYMULE_NO_INIT=1 sh`.

**2. Restart your agent and ask:**

> *connect my github*

A keymule window opens and you type the token there, not in the chat. The agent only hears
"saved, verified as you", and from then on it can use the token: *"why did my last CI run fail?"*

Pasted a token into the chat out of habit? keymule catches it before it's sent and offers to save
it instead.

Building from source, checking downloads, or doing it all from the terminal:
[setup details](#-setup-details).

---

## 🤔 Sound familiar?

**"I notice you shared an API key. You should rotate it right away."**
You pasted a token so the agent could check a pipeline. Now it's in the model provider's logs and
in a transcript file on your disk, so the agent tells you to rotate it. You make a new one, paste
it next week, and get the same warning.

**"I can't read your `.env` file" / "please don't share secrets with me".**
The agent won't touch the credential, so you end up running the command yourself, or exporting
`GH_TOKEN` in a terminal and pasting the output back. Or the opposite happens: it reads `.env`
without asking, and your keys land in the transcript.

**The same token, over and over.**
New session, new agent (Claude Code today, Codex tomorrow, Cursor for that one repo), a new dev
container: each one wants the token again, so it goes back through the clipboard and the chat.

**Keys sitting in plain text.**
Your MCP servers keep their API keys in `~/.claude.json` or `~/.cursor/mcp.json`. Every agent can
read them, and they show up in screenshots and dotfile backups.

**"Add it to GitHub Actions secrets."**
CI needs the token, so you copy it from somewhere, paste it into GitHub's web page, and try to
remember where else it lives when you rotate it.

## 🫏 With keymule

Tokens live in keymule's encrypted vault (its key is in your OS keychain). The agent asks keymule
to use them by name; keymule attaches the secret itself and strips it from anything that comes
back. The agent never sees the value, so there's nothing to leak and nothing to rotate.

```text
You:    check why my bamboo pipeline failed
Agent:  → list_credentials()            ← names and hints only
        → http_request(credential="bamboo", path="/rest/api/latest/result/PROJ-PLAN")
                                        ← keymule adds the token and blanks it out of the reply
Agent:  Build #482 failed in "Integration tests"…
```

| The problem | With keymule |
|---|---|
| Pasting a token into chat | The agent opens a keymule dialog and you type the token there, not in the chat. Paste one by habit and keymule catches it before it's sent and offers to save it instead. |
| The agent won't, or shouldn't, read the credential | It doesn't need to. `http_request` makes the call for it, and `keymule exec <name> -- gh …` runs CLI tools with the token in their environment and out of their output. |
| Giving the same token to every agent | Save it once. Claude Code, Codex, Cursor, Claude Desktop and Antigravity all use the same vault (`keymule init` sets them up). |
| Keys in plain text in MCP configs | `keymule adopt` moves them into the vault; the servers start the same way through `keymule exec`. |
| Copying secrets into GitHub Actions | `keymule gh-secret --repo you/app API_KEY=stripe`: one dialog, encrypted with the repo's key, uploaded with your `gh` login. |
| Not knowing what already leaked | `keymule doctor` scans agent transcripts, configs, `.env` files and shell history, and says what to rotate. |
| Approving every little thing | One **Allow all** for a project (Today, or until you turn it off), and keymule stops asking there. Deleting still asks. |
| No screen in a dev container | Questions wait in the keymule page; answer them from your Mac's browser. |

---

## ✨ Features

- 🙈 **The agent never sees the secret.** It refers to credentials by name; keymule attaches the secret and redacts it from every response, command output and log line.
- 🔒 **Locked to hosts.** A Bamboo token only goes to your Bamboo host. Requests elsewhere are refused, and redirects to other hosts are followed *without* the token. The allowed hosts are sealed in the vault with the token, so editing keymule's files doesn't change them.
- ✋ **Paste-to-save.** Paste a token into Claude Code, Codex or Cursor by habit and keymule catches it *before* it's sent and offers to save it, named from your message (`github-launchstory` for "the PAT for our launchstory account") or, when the message says nothing, from the conversation around it (`github-homebrew-tap` when the agent just asked for `HOMEBREW_TAP_TOKEN`).
- 🧑‍💻 **You approve the risky parts.** A new program, a new host, copying to GitHub or deleting asks you in a keymule dialog the agent can't click. **Allow all** turns the questions off for one project; deleting always asks.
- 🧰 **CLI tools too.** `keymule exec github --env GH_TOKEN -- gh pr create …` gives the token to that program only, and hides it from its output.
- 🔑 **Passwords, private keys and AWS keys.** Not just API tokens: `keymule exec db-pass --env PGPASSWORD -- psql …`, `keymule exec deploy-key --file KEY -- ssh -i {KEY} host` (the key goes in a temporary file only you can read, removed afterwards), `keymule exec aws-prod -- aws s3 ls` (gets `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`). Pasted ones are caught before they're sent, and the popup takes several lines for a key.
- 🐙 **GitHub Actions secrets.** `keymule gh-secret` copies several secrets into a repo in one dialog.
- 🖥 **Menu bar and page.** On macOS keymule lives in the menu bar and opens its page in its own window: credentials, MCP servers, live activity, leak scan. **Reveal** shows a value in a keymule window when *you* need to see it, never on the page or in a terminal.
- 🐧 **Linux and dev containers.** Without a screen, keymule's questions wait in the page, answered from a browser you've paired.
- 🩺 **Leak doctor.** `keymule doctor` finds tokens that already leaked into agent transcripts, configs, `.env` files or shell history.
- 📦 **Adopt MCP secrets.** `keymule adopt` moves API keys out of MCP configs; remote (HTTP) servers connect through `keymule remote`, which adds the key header itself.
- 🧩 **Service presets.** Bamboo, GitHub, GitLab, Jenkins, Stripe, OpenAI, or any HTTP API: the auth scheme, how to verify a token, and an API cheat sheet for the agent.
- 📒 **Activity log and usage.** Every use is logged to `~/.config/keymule/audit.log`, without secrets. The page's **Usage** view adds it up: uses per day by kind (API calls, commands, MCP server starts, reveals) over 7, 30 or 90 days, each key's trend and last use, and the keys nobody has used lately.

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

## 🔧 Setup details

The [two steps at the top](#-get-started) are all most people need. The rest:

The Linux builds are static, so they run in any container (Alpine too). Each download comes with a
checksum and a GitHub attestation of the workflow run that built it:
`gh attestation verify <file> --repo Rorogogogo/keymule`.

<details>
<summary>Build from source instead</summary>

```sh
cargo install --git https://github.com/Rorogogogo/keymule
```

On a Mac, `scripts/install-macos.sh` builds and signs it with a local certificate, so the
keychain's "Always Allow" sticks across your own rebuilds (setup steps are at the top of the
script). Downloaded builds ask once per update instead.
</details>

For a self-hosted service, give the agent its address:

> *connect my bamboo, it's at https://bamboo.yourcompany.com*

The agent calls `add_credential` and a keymule dialog pops up showing the host the token will be
locked to. You type the token there. It goes straight into the vault, is verified against Bamboo,
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
| `keymule add ssh --host H --username U [--port P] [--key \| --key-file PATH] [--dialog]` | Store an SSH login for one server: a password (typed in the terminal, or the dialog), a private key (pasted in the dialog), or a key file already on this machine (used where it is) |
| `keymule ssh <name> [-- <command…>]` | Log in to that server and run a command (or open a session). keymule answers ssh's password prompt itself, or hands it the key as a private file; you approve once per folder, and the secret is scrubbed from the output |
| `keymule convert <name> --to ssh\|password [--host H --username U --port P]` | Turn an MCP server secret that no MCP server runs into an SSH login or a password, after a yes in a dialog. The value moves inside keymule; you don't type it again |
| `keymule edit <name> --url/--username/--service/--description` | Change metadata (an SSH login: `--host/--port/--username/--key-file`; an MCP server secret: `--description`). A new host needs terminal confirmation |
| `keymule rename <name> <new-name> [--description D]` | Give a credential a better name |
| `keymule rotate <name> [--dialog]` | Replace the secret after rotating it |
| `keymule reveal <name>` | Show a secret in a macOS window after you agree (never printed, so agents running it can't read it); Copy clears the clipboard after 30 s |
| `keymule test <name>` | Check the credential still works |
| `keymule migrate` | Move secrets from an older keymule's keychain items into the encrypted vault |
| `keymule rm <name> [--dialog]` | Delete it (asks you first, on the terminal or in a dialog; the web page's Delete uses the dialog) |
| `keymule presets` | List service presets |
| `keymule doctor [--all] [--json] [--dir D]` | Find leaked and plain-text secrets |
| `keymule adopt [--dry-run] [--yes] [--server NAME]` | Move plain-text MCP server secrets into the keychain |
| `keymule exec <name> [--env VAR] [--file VAR] -- <command…>` | Start an MCP server with its secrets (used in configs), or run a program with a stored credential: in an env var, or (`--file`, for a private key) as a temporary file whose path goes in VAR and wherever `{VAR}` is in the command. AWS keys go in their two variables. New commands need your approval once |
| `keymule install <name> --host H --username U [--dir D]` | Copy a credential into git's credential store, so git uses it for `https://U@H`; the same dialog lets the service's CLI (gh) use it in the repo folder |
| `keymule uninstall <name> [--host H] [--username U]` | Remove those copies again (the credential stays in keymule) |
| `keymule gh-secret --repo O/R SECRET=name… [--via gh\|gh:LOGIN\|CRED] [--dry-run]` | Copy credentials into GitHub Actions secrets for CI, all in one dialog: encrypted with the repo's key, uploaded as the account `gh` is signed in as (or a stored GitHub credential) after keymule checks it has admin on the repo |
| `keymule allow [--dir D] [--off [--everywhere]] [--status]` | Allow all: one dialog (Cancel / Today / Always), then keymule stops asking in this repo (or `--dir ~` for every folder) (any program including shells, gh-secret, new hosts, reveal, the read-back guard, pasted secrets saved without asking). Deleting still asks; every skipped dialog is logged |
| `keymule remote <name> <url> [--header "Name: value"]` | Connect to a remote (HTTP) MCP server, adding the headers keymule holds (used in configs). New URLs need your approval once |
| `keymule ui [--no-open] [--port N]` | Open the local web page (127.0.0.1 only, port 7420 by default, token in the link it opens). Without a screen, keymule's questions wait there for a paired browser |
| `keymule menubar [--background] [--login on\|off]` | keymule in the macOS menu bar: its page in its own window (no browser), with a Dock icon while the window is open. `keymule ui` opens that window when it's running |
| `keymule init [--dry-run]` | Set up every agent found (see [Agents](#agents)). On macOS it also puts keymule in the menu bar and makes it start at login (first setup only). The first time, it also offers allow-all for your home folder, with Always preselected |
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
| `update_credential` | Change URL, username or description (an SSH login's host, port or username); a new host or server needs the user's approval in a dialog |
| `install_credential` | Copy a credential into git's credential store for a host and account, after the user approves in a dialog |
| `add_ssh_credential` | `name`, `host`, `port`, `username`, `auth` (`password` or `key`) or `key_file`, `description`: an SSH login, typed in the dialog, used with `keymule ssh` |
| `convert_mcp_secret` | Move an MCP server secret no server runs to Credentials as an SSH login or a password, after the user confirms; nobody sees the value |
| `add_mcp_secret` | Only for a secret an MCP server reads (database URL, API key, header), via the dialog, for `keymule exec` or `keymule remote`. Keeps its description |
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

### SSH logins

```sh
keymule add ssh --name vultr-ssh --host 203.0.113.7 --username root   # or ask your agent
keymule ssh vultr-ssh -- uptime
```

The server, port and account are sealed with the password, so editing `credentials.toml` can't
send it to another machine, and the agent can't add ssh options. A password reaches ssh through
`SSH_ASKPASS`: ssh runs keymule itself, which answers only a password prompt, once, from a file
only you can read that it deletes right away. A key goes in as `-i` with a temporary private
file. A server that only takes keys can use a key file you already have
(`keymule edit vultr-ssh --key-file ~/.ssh/id_ed25519`): ssh uses it where it is, and the
password stays in the same entry (for the provider's console, say). The first connection to a server accepts its host key; a changed key is refused. A
password stored the old way with `add_mcp_secret` moves over with `keymule convert`, or the
**Move to Credentials** button on the page.

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

## 🐧 Linux and dev containers

Where there's no screen for keymule's popups (Linux, a dev container, a machine over SSH), its
questions wait in the keymule page instead, and you answer them from your browser:

```sh
export KEYMULE_PASSPHRASE=…        # no system keychain in a container: unlock the vault with a passphrase
keymule ui --no-open                # in your own terminal; prints the page link and a pairing code
```

Many containers have no D-Bus, so there's no system keychain for the vault's key. `keymule init`,
`keymule list` and the page say so when that's the case. Set `KEYMULE_PASSPHRASE` where your agents
run: in a dev container, `"remoteEnv": { "KEYMULE_PASSPHRASE": "${localEnv:KEYMULE_PASSPHRASE}" }`
in `devcontainer.json` with the value set on your own machine (so it never sits in the repo), or an
`export` in your shell profile. Use the same passphrase every time: it's what unlocks the vault.

Forward port 7420 to your Mac (VS Code does it from the Ports panel), open the link it printed,
and click **Pair this browser** with the code. From then on, when an agent needs your OK or a new
secret, the question appears on the page: Allow, Cancel, Today / Always for allow-all, or a field
to type the secret into.

- **You hear about it.** The tab title shows "(1) keymule asks", and the page offers a browser
  notification, so a question doesn't time out while you're watching the terminal.
- **Paired once, the address is enough.** A paired browser opens `http://127.0.0.1:7420` without
  the link's token: it signs in with its pairing key, and reconnects by itself when `keymule ui`
  restarts. (No cookie: a browser would send it to every local port.)
- **Never stuck.** A browser that isn't paired can still say no, hide the question, or pair right
  in the question with the code. 127.0.0.1 and localhost count as different browsers.
- **Only your browser can answer.** Pairing makes a key pair inside the browser (its private half
  can't be read out); every answer is signed with it, and keymule checks the signature. Something
  that can call the page's server or write keymule's files, an agent included, can't answer for
  you. A second browser pairs only after an already paired one allows it, or with the code from
  `keymule ui` run in your own terminal (so losing your only paired browser doesn't lock you out).
- **Typed secrets are encrypted** to the keymule process that asked, so they never sit in a file
  in the clear.
- **Reveal works here too.** After you click Show, keymule encrypts the value to a one-off key
  that only that browser tab holds. The page shows it for 30 seconds; the server and its files
  only ever see ciphertext.
- **The limit:** in a container the vault's key is the passphrase in the environment, which the
  agent can read too. So an agent that deliberately decrypts the vault itself can get at the
  secrets. Page approvals guard keymule's own ways of using them, the same trade-off as allow-all.
- On a Mac, `KEYMULE_APPROVALS=page` sends questions to the page instead of popups.

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
- `http_request` is for HTTP API tokens; passwords, private keys and AWS keys are only handed to programs through `keymule exec`, and SSH logins only to `ssh` through `keymule ssh`. Detection catches labelled passwords ("password: …", `DB_PASSWORD=…`); one dropped into a sentence with no label isn't caught, so ask your agent to open keymule's popup for passwords instead of pasting them.
- Popups are macOS-only; elsewhere questions wait in the keymule page (see [Linux and dev containers](#-linux-and-dev-containers)). Revealing a value needs a Mac screen.
- The agent picks the URL in `add_credential`, so read the host in the dialog before typing your token.
- After each update of a downloaded build, macOS asks once for access to the vault key (not once per secret), because the builds aren't signed with an Apple Developer ID yet. Building from source with `scripts/install-macos.sh` avoids it.
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
- **0.8–0.10**: ✅ `keymule init` for Codex, Cursor, Claude Desktop and Antigravity; `keymule exec` for any credential; git installs; the read-back guard; `keymule gh-secret`
- **0.11**: ✅ Allow all; `gh-secret` with your `gh` login; a choice dialog for secrets pasted into Codex and Cursor
- **0.12**: ✅ cleaner dialogs with short text and simple buttons
- **0.13**: ✅ the menu bar app
- **0.14**: ✅ questions in the page for Linux and dev containers
- **0.15**: ✅ ready-made downloads, a one-line installer and Homebrew
- **Next**: more presets, database auth proxy, SSH agent, git/docker credential helpers
- **Later**: Touch ID approval for sensitive credentials, short-lived credentials (AWS STS, GitHub App tokens)

## 🗂 Project structure

```text
src/
  main.rs        starts keymule
  cli.rs         every command and its options
  commands/      what each command does (credentials, setup)
  storage/       the vault and what's in it: store, encrypted vault, service presets, redaction
  consent/       asking you: popups (dialog.html), page approvals, allow-all, terminal prompts
  agents/        the MCP server, the paste hook (hook/), the read-back guard, init, adopt
  usage/         using a secret without showing it: HTTP, exec, MCP servers, remote, git, GitHub secrets
  scan/          finding secrets: before a message is sent, and leaks after the fact (doctor)
  app/           the local page (ui/) and the macOS menu bar app
tests/e2e.py     end-to-end tests against a built binary
```

## 📄 License

MIT OR Apache-2.0
