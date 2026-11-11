#!/bin/sh
# Build and install keymule, then sign it with a fixed identity so macOS
# Keychain recognizes every rebuild as the same app. Without this, each
# rebuild is a new ad-hoc signature and "Always Allow" has to be clicked
# again for every secret.
#
# One-time setup: Keychain Access → Certificate Assistant → Create a
# Certificate… → Name "keymule local", Identity Type "Self-Signed Root",
# Certificate Type "Code Signing". Then open the certificate, expand Trust,
# and set "Code Signing" to "Always Trust".
set -e
IDENTITY="${KEYMULE_SIGN_IDENTITY:-keymule local}"
cd "$(dirname "$0")/.."
cargo install --path . --locked
BIN="$(command -v keymule)"
if codesign --force --sign "$IDENTITY" --identifier dev.keymule "$BIN" 2>/dev/null; then
  echo "Signed $BIN as \"$IDENTITY\". Click Always Allow once per secret; it sticks across rebuilds now."
else
  echo "Installed $BIN, but couldn't sign it with \"$IDENTITY\" (see the setup steps in this script)." >&2
  echo "macOS will ask for Keychain access again after each rebuild." >&2
fi
