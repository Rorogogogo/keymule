# Approvals, grants and guided setup

Status: design. Stage 1 shipped in v0.11 (`gh-secret`: batch, existing gh login, verified
account). The rest is proposed.

## Decided after review (2026-11-14)

- **Allow all, per project, shipped first.** `keymule allow` shows one dialog (*Cancel /
  Today / Always*) for the current git repo. After that, keymule
  asks nothing there except before deleting: not for programs (agent-built ones included),
  `gh-secret`, git installs, MCP commands and URLs, new hosts, reveals, or saving a pasted
  secret. Shells, interpreters and the read-back guard are let through as well. Exec under allow-all isn't remembered, so `keymule allow --off` brings every
  question back. Each skipped dialog is an `AUTO-ALLOW` line in the activity log. The
  finer-grained grants below are kept as a later option, not the plan.
- **The trade-off, said once in the dialog:** with allow-all on, keymule still keeps
  secrets out of its own output and transcripts, so they don't leak by accident. But an
  agent that sets out to read one can.
- **keymule stays a vault.** No OAuth flows and no token refresh: it stores secrets, hands
  them to approved tools and copies them where you send them. Section 4 (OAuth capture),
  stage 2 and the "+ OAuth capture" column below are dropped. For the Chrome Web Store, CI
  can use a service account with GitHub signing in to Google, so there are no secrets to
  store.
- **Codex and Cursor ask with a choice, not a text stop.** Their prompt hooks can only let a
  message through or stop it; neither can swap a secret out of it. So keymule shows a
  native dialog: *Save* (default), *Send Anyway* (goes straight through) or
  *Don't Send*. After saving, Codex still needs `y` to send the rest, because the hook
  can't send a changed message for you. Under allow-all, keymule saves without the dialog.

## Where this comes from

On 2026-11-14 an agent set up Chrome Web Store publishing through GitHub Actions for
`Rorogogogo/aiyifan-ads-free`. The workflow needs three Actions secrets: `CWS_CLIENT_ID`,
`CWS_CLIENT_SECRET` and `CWS_REFRESH_TOKEN`. `gh` was already signed in as the repo owner,
and the Google OAuth client was already stored in keymule. Even so:

| What went wrong | Root cause in keymule |
|---|---|
| A dialog for every credential × program pair | Approvals are per `(credential, program, folder)`, with no way to approve a set |
| `gh-secret` wanted a GitHub token stored in keymule | It could only upload with a stored credential, not the gh login the user already had |
| The one stored GitHub token was for another account (`launchstory`) | keymule showed the *description* ("…our own github account which is Rorogogogo"), never the verified account |
| The user revealed the client secret, typed it into the OAuth helper, then copied the refresh token from the terminal into another dialog | keymule had no OAuth flow, so a token it could have caught passed through the user's hands twice |
| Client ID, client secret, authorization code and refresh token got mixed up (the user pasted the redirect URL, code included, into the chat) | Nothing explained which value is which, or kept the code out of sight |
| The agent compiled a Swift helper to pipe values from `keymule exec` into `gh secret set` | The supported path didn't fit, so the agent built its own |
| The agent said keymule "can't" do things it could | Errors said what failed, not what would work instead |

## Principles

1. **Use, create and reveal are separate.** Approving a use never creates or reveals
   anything. Creating a secret (paste, import, OAuth) is its own step. Revealing stays a
   human-only window.
2. **Grants bind to facts keymule checked, not to labels.** That means resolved program
   paths and hashes, canonical project roots, the account GitHub reports, and the parent
   process. Never an agent name the agent supplied.
3. **One approval per intent, not per step.** The dialog previews everything a grant covers.
4. **Reuse logins the user already has** (gh, and later glab and gcloud) instead of asking
   for another token. Borrow the token inside keymule, use it for that call, never export it.
5. **Every refusal names the supported alternative.**
6. **Be honest about prompts keymule doesn't own.** Google's consent screen, GitHub's own
   checks and the agent harness's permission prompts stay. keymule only removes its own.

## 1. Grants

A grant is a sealed record in the vault, like today's `exec_grants`, so an agent can't add
or widen one by editing a file:

```
Grant {
  id, created, expires?            // optional time limit
  mode: once | session | project | auto
  credentials: [name…]             // exactly these
  operation: gh-secret | exec | http | oauth
  destination:                     // operation-specific, exact
    gh-secret: github.com/<owner>/<repo> + secret names
    exec:      program path + sha256 (or code-signing team ID)
    http:      method + host + path prefix
  project: canonical git root + origin URL
  harness: verified agent identity (below)
  uses, last_used
}
```

**Modes**, as dialog buttons. macOS dialogs allow three buttons, so *Once* and *This
session* share a pop-up:

| Mode | Lasts | Matches |
|---|---|---|
| Ask every time | this call | — |
| This session | until the agent process that asked exits (max 12 h) | the same operation |
| This project | until revoked or expired (default 30 days) | the same operation, same project |
| Auto (opt-in per project, set in `keymule ui`) | until revoked | any operation already covered by one of the project's grants, plus read-only checks, plus re-sync after a rotation |

**Auto mode re-asks when anything widens**: a new destination (another repo, host or secret
name), a credential outside the grant, a changed program hash or path, a different
operation, or a different harness. Creating or revealing a secret always asks, in any
mode.

**Session and harness identity.** keymule walks its parent processes
(`proc_pidpath`/`sysctl KERN_PROC_PID`) up to the first known harness: Claude Code, Codex,
Cursor or Claude Desktop. It records that process's path, code-signing team ID (where it
has one) and pid plus start time. A *session* grant matches only that pid and start time.
A *project* grant matches the harness binary identity. An npm-installed CLI shows up as
`node` running a script, which is weak evidence; then keymule records the script path and
hash and offers only *Once* or *This session*. Self-reported names (MCP `clientInfo`, env
vars) are shown, never trusted.

**Remembering needs a human.** The session that prompted this used computer use, and
computer use can click a native dialog. So *This project* and *Auto* also need Touch ID or
the login password (LocalAuthentication). Taking a once or session grant stays a plain
click.

## 2. Batch approval

- **Done (v0.11):** `keymule gh-secret --repo o/r A=cred-a B=cred-b C=cred-c` shows one
  dialog listing every secret, which ones replace existing values, the verified uploading
  account, and where the values could go before the copy.
- **Next:** an MCP tool `request_grant({project, purpose, operations:[…]})`. It shows one
  preview, such as *"Allow Claude Code in ~/…/kubb-helper to configure Chrome Web Store
  publishing for Rorogogogo/aiyifan-ads-free using aierfan-cws-client and
  aierfan-cws-refresh-token: copy 3 Actions secrets, run 1 verification"*, and returns a
  grant id. Later calls that fit the grant run without a dialog. Long previews open in
  `keymule ui`, with the approval still done natively.

## 3. Existing CLI logins (done for gh)

`gh-secret` tries the account `gh` is signed in as (`gh auth token --hostname github.com
[--user X]`, run by keymule with `GH_TOKEN`/`GH_HOST`/`NODE_OPTIONS`-style variables
removed, stdout kept in memory). keymule then calls api.github.com **itself**. It doesn't
pipe values into `gh secret set`: a gh config or proxy the agent changed could swap in its
own "repo key" and decrypt the values. A fake `gh` on PATH only yields a token for some
other account, and the dialog names that account. The same pattern fits `glab`, `gcloud
auth print-access-token` and `vercel`.

## 4. OAuth capture (dropped: keymule stays a vault)

```
keymule oauth google --client aierfan-cws-client \
  --scope https://www.googleapis.com/auth/chromewebstore --save-as aierfan-cws-refresh-token
```

- Loopback redirect on 127.0.0.1, PKCE S256 and a random `state`, browser opened by
  keymule. The authorization code never leaves keymule. The page tells the user to close
  the tab and paste nothing.
- The code exchange sends the client secret only to a token URL inside that credential's
  sealed hosts (`oauth2.googleapis.com`).
- keymule stores the refresh token sealed to the same hosts. With `openid email` added, it
  also records which Google account approved. The agent gets back `stored
  aierfan-cws-refresh-token (chromewebstore, approved by you@gmail.com)` and nothing else.
- No keymule dialog: Google's consent screen is the user's approval, and keymule shows a
  notification afterwards.
- **Client import** fixes the ID-versus-secret mix-up: `import_oauth_client(path)` reads the
  `client_secret_….json` that Google offers on the "OAuth client created" screen. One
  confirmation stores both values as one credential with labeled fields, then offers to
  delete the file. The dialogs explain each value once: client ID (public, names the app),
  client secret (the app's password), authorization code (one-time, keymule handles it),
  refresh token (long-lived, what CI needs).

## 5. Verified identity

Each preset gets an identity probe: GitHub `GET /user` plus the `X-OAuth-Scopes` header,
Google `tokeninfo`, Stripe `GET /v1/account`. The result (account, scopes, time checked)
is stored with the credential. `list_credentials` and the UI show that next to the
description, and warn when they disagree (*"description mentions Rorogogogo; token is
launchstory"*). `gh-secret` already checks the account and its repo access before asking
(v0.11).

## 6. Controls

- `keymule ui` → **Grants**: every remembered grant with its scope, mode, expiry, use count
  and last use. Revoke it, or set an expiry.
- **Activity**: `audit.log` lines grouped by grant (already written: `GH-SECRET`, `EXEC`,
  `GET`…).
- **Pause**: a sealed `paused` flag. Every operation then refuses with *"keymule is paused
  by the user; nothing was sent"*. Any caller can pause. Resuming needs the UI plus Touch
  ID, or a TTY.

## 7. Precise refusals

Every error says what was refused, why, and the command that would work. From v0.11:

```
keymule: nothing keymule can use may set Actions secrets on Rorogogogo/aiyifan-ads-free:
  github-launchstory: signed in as launchstory, can't see Rorogogogo/aiyifan-ads-free (GitHub says 404)
  gh: signed in as Rorogogogo, admin on Rorogogogo/aiyifan-ads-free

gh can: use --via gh.
```

Next, apply the same rule to `exec` refusals, for example *"for Actions secrets use
`keymule gh-secret`; a helper program isn't needed"*, and to the MCP tool descriptions.

## Prompts keymule can't remove

| Prompt | Owner |
|---|---|
| Google consent, including "Google hasn't verified this app → Advanced → Go to …" | Google, once per refresh token |
| Creating the OAuth client and consent screen, enabling the API | Google Cloud console |
| Bash or tool permission prompts, auto-mode classifier | Agent harness (Claude Code, Codex) |
| Keychain "Always Allow" after an unsigned rebuild | macOS (fixed by signing) |

## Walkthrough: Chrome Web Store setup, before and after

Interactions counted here: keymule dialogs and windows; secret values the user moved by
hand; commands the user had to run themselves; chat turns the user spent on keymule
confusion or detours. Google's consent clicks (about 4) happen in every column.

**Before (the real session):**

1. Add the client secret: dialog, copied from Google's modal.
2. Add the client ID: dialog, copied too.
3. "Which one is the OAuth client secret?" The user opens `keymule ui` and **reveals** it,
   because the helper needs it.
4. The user runs `node scripts/cws-get-refresh-token.mjs` in their own terminal and types
   the client ID and the revealed secret. They consent at Google, then paste the redirect
   URL, code included, into the chat.
5. Add the refresh token: dialog, copied from the terminal ("which one is it?", "can you
   add it for me?", "it starts with 1//, is it the one?").
6. `gh-secret` wants a stored GitHub token. The only one belongs to `launchstory`. Then:
   "why can't we use the CLI?", "what are we trying to do?", "then why not gh?"
7. The agent sets `CWS_CLIENT_ID` with `gh secret set --body '<id>'`, so the value lands in
   a command line and the transcript.
8. "You can run it for me." The agent compiles a Swift helper. Two `exec` dialogs follow:
   client secret, then refresh token.
9. The verification workflow runs, with no prompts.

**After v0.11 (this change).** Steps 1 to 5 are unchanged. Steps 6 to 8 become:

```
keymule gh-secret --repo Rorogogogo/aiyifan-ads-free \
  CWS_CLIENT_ID=aierfan-cws-client-id CWS_CLIENT_SECRET=aierfan-cws-client-secret \
  CWS_REFRESH_TOKEN=aierfan-cws-refresh-token
```

That is one dialog: *"Copy 3 secrets from keymule into Rorogogogo/aiyifan-ads-free's
GitHub Actions secrets? … Uploaded as Rorogogogo (admin there), using your gh login
(/opt/homebrew/bin/gh)."* Then *"created/updated … GitHub now lists all of them."*

**After OAuth capture and client import (stage 2):**

1. Click "Download JSON" on Google's client-created screen (Google). The agent calls
   `import_oauth_client(~/Downloads/client_secret_….json)`: one keymule confirmation.
2. The agent runs `keymule oauth google …`. The browser opens, the user consents (Google),
   and keymule stores the refresh token.
3. The agent runs `gh-secret` (one dialog), then the verification workflow.

**After grants (stage 3):** steps 1 and 2 stay as they are, because creating secrets stays
separate. Step 3's dialog becomes the `request_grant` preview, taken with *This project*.
After the next refresh-token rotation, Auto re-syncs the Actions secret with no prompt.

| | Before | v0.11 | + OAuth capture | + project grant, on a re-run |
|---|---|---|---|---|
| keymule dialogs / windows | 6 (3 add, 1 reveal, 2 exec) | 5 | 2 | 0 |
| Secret values moved by hand | 5 | 5 | 0 | 0 |
| Commands the user ran | 1 | 1 | 0 | 0 |
| Detour / confusion chat turns | ~10 | ~6 | ~0 | 0 |
| Helper programs the agent wrote | 1 | 0 | 0 | 0 |
| Secret values put on a command line | 1 | 0 | 0 | 0 |

## Stages

1. **v0.11 (done):** `gh-secret` batch with one dialog, the existing gh login, verified
   account and access before asking, `--dry-run`, verification after upload, precise
   errors.
2. `keymule oauth google` and `import_oauth_client`. Identity probes stored on credentials.
3. Sealed grants with modes. The parent-process harness check. Touch ID for remembered
   grants. `request_grant`. Grants, Activity and Pause in the UI.
4. Auto mode: re-sync after rotation, and the other borrowed CLI logins.
