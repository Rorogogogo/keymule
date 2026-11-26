# keymule's page

The page `keymule ui` serves (and the macOS menu-bar window shows): React 19 + TypeScript,
built with Vite into `src/app/ui/dist/`, which the Rust binary embeds with `include_str!`.
`dist/` is committed so `cargo build` and `cargo install` never need Node; CI rebuilds it and
fails if it doesn't match the source.

The page never sees a secret. It shows names and metadata, and its buttons run keymule
commands that ask in keymule's own dialogs. In page mode (Linux, dev containers), a typed
secret is encrypted in the browser to the keymule process that asked, and a revealed value is
encrypted to a key only that tab holds (see `src/lib/api.ts`).

## Change it

```sh
cd ui
npm ci
# after editing src/:
npm run build           # typecheck + build into ../src/app/ui/dist
cd .. && cargo build    # the binary picks up the new dist
```

Commit `src/app/ui/dist/` with your change.

### With hot reload

```sh
# terminal 1: keymule with demo data (debug build; nothing real in it)
cargo build
python3 -I ui/scripts/demo.py /tmp/keymule-demo
KEYMULE_HOME=/tmp/keymule-demo KEYMULE_TEST_VAULT=/tmp/keymule-demo/vault.json \
  KEYMULE_NO_UPDATE_CHECK=1 target/debug/keymule ui --no-open
# terminal 2: the dev server, which forwards /api to it
cd ui && npm run dev
```

Open `http://localhost:5173/#t=<token>` with the token from terminal 1's link. Set
`KEYMULE_UI_PORT` if `keymule ui` isn't on 7420. Add `KEYMULE_APPROVALS=page` to terminal 1
to work on the questions asked in the page.

## Test it

```sh
cargo build && cd ui && npm run e2e                                  # 29 checks in Chrome
npm run e2e -- ../target/debug/keymule /tmp/keymule-shots            # and save screenshots
```

It drives your installed Chrome (`CHROME=` to pick another) against a real `keymule ui` with
demo data: every view in light and dark, row menus, notes, the add and move forms, the usage
chart, and in page mode pairing, a question, a secret typed in the page and a revealed value.

## Layout

- `src/lib/store.ts`: a tiny store (one state object, `useSyncExternalStore`).
- `src/lib/app.ts`: the state and everything that changes it: loading, polling, actions,
  the scan, questions, notifications.
- `src/lib/api.ts`: the token, signing in with a paired browser, and the browser's crypto.
- `src/components/`: one file per area (credentials, servers, activity, leaks, usage,
  banners, questions, sidebar). `src/app.css` is the whole stylesheet.
