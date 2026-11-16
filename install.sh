#!/bin/sh
# Install keymule and set it up for your AI agents, in one go:
#
#   curl -fsSL https://raw.githubusercontent.com/Rorogogogo/keymule/main/install.sh | sh
#
# It downloads the release build for this machine (macOS or Linux) with the
# installer every release ships, then runs `keymule init`, which connects
# Claude Code, Codex, Cursor, Claude Desktop and Antigravity. Every file
# init edits is backed up next to itself first.
#
#   KEYMULE_NO_INIT=1   only install (CI, containers, images); run `keymule init` later
#   KEYMULE_VERSION=0.17.0   a specific release instead of the latest
set -eu

repo="Rorogogogo/keymule"
version="${KEYMULE_VERSION:-latest}"
if [ "$version" = "latest" ]; then
    url="https://github.com/$repo/releases/latest/download/keymule-installer.sh"
else
    url="https://github.com/$repo/releases/download/v${version#v}/keymule-installer.sh"
fi

say() { printf '%s\n' "$*"; }
fail() { printf 'keymule: %s\n' "$*" >&2; exit 1; }

if command -v curl >/dev/null 2>&1; then
    fetch() { curl --proto '=https' --tlsv1.2 -fsSL "$1"; }
elif command -v wget >/dev/null 2>&1; then
    fetch() { wget --https-only -qO- "$1"; }
else
    fail "needs curl or wget to download the release"
fi

# Download first, then run: a cut-off download never runs half a script.
installer="$(fetch "$url")" || fail "couldn't download $url"
printf '%s\n' "$installer" | sh || fail "the release installer failed (see above)"

# Find what was just installed: it may not be on this shell's PATH yet.
bin=""
for c in \
    "${KEYMULE_INSTALL_DIR:-}/keymule" \
    "${KEYMULE_INSTALL_DIR:-}/bin/keymule" \
    "${CARGO_HOME:-$HOME/.cargo}/bin/keymule" \
    "$(command -v keymule 2>/dev/null || true)"; do
    if [ -n "$c" ] && [ "$c" != "/keymule" ] && [ "$c" != "/bin/keymule" ] && [ -x "$c" ]; then
        bin="$c"
        break
    fi
done
[ -n "$bin" ] || fail "installed, but couldn't find the keymule binary; run: keymule init"

if [ -n "${KEYMULE_NO_INIT:-}" ]; then
    say ""
    say "keymule is installed. Set up your agents when you're ready: keymule init"
    exit 0
fi

say ""
say "Setting keymule up for your agents (keymule init)…"
say ""
# Not reading the rest of this pipe: init gets no stdin.
"$bin" init </dev/null || fail "setup didn't finish; fix what it says above, then run: keymule init"
