#!/bin/sh
# Build keymule, sign it with a fixed identity, and only then install it,
# so macOS Keychain recognizes every rebuild as the same app. Without this,
# each rebuild is a new ad-hoc signature and "Always Allow" has to be
# clicked again for every secret.
#
# One-time setup: Keychain Access → Certificate Assistant → Create a
# Certificate… → Name "keymule local", Identity Type "Self-Signed Root",
# Certificate Type "Code Signing". Then open the certificate, expand Trust,
# and set "Code Signing" to "Always Trust".
set -e
IDENTITY="${KEYMULE_SIGN_IDENTITY:-keymule local}"
DEST="${CARGO_HOME:-$HOME/.cargo}/bin/keymule"
cd "$(dirname "$0")/.."
cargo build --release --locked
BUILT="target/release/keymule"
if ! codesign --force --sign "$IDENTITY" --identifier dev.keymule "$BUILT"; then
  echo "Couldn't sign keymule with \"$IDENTITY\", so nothing was installed." >&2
  echo "Create the certificate first (see the steps at the top of this script)." >&2
  exit 1
fi
mkdir -p "$(dirname "$DEST")"
# Copy to a temp name and rename, so a running keymule is never half-written.
cp "$BUILT" "$DEST.new"
mv -f "$DEST.new" "$DEST"
echo "Installed $DEST, signed as \"$IDENTITY\"."
echo "macOS asks for Keychain access once per secret. Click Always Allow; it sticks across rebuilds now."
