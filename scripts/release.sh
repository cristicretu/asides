#!/bin/bash
# Builds ExplainIt.app (with the CLI inside), signs it with a Developer ID certificate and a secure
# timestamp, has Apple notarize it, staples the ticket and zips it for a GitHub release.
#
# One-time setup: store notarytool credentials under a profile name (an app-specific password from
# account.apple.com), then run `make release`:
#   xcrun notarytool store-credentials explainit-notary --apple-id <Apple ID> --team-id <Team ID>
set -euo pipefail
cd "$(dirname "$0")/.."

PROFILE="${NOTARY_PROFILE:-explainit-notary}"
IDENTITY="$(scripts/signing-identity.sh)"
case "$IDENTITY" in
    "Developer ID Application: "*) ;;
    *) echo "Notarization needs a \"Developer ID Application\" certificate; found: $IDENTITY" >&2; exit 1 ;;
esac
TEAM_ID=$(sed -n 's/.*(\([A-Z0-9]*\))$/\1/p' <<<"$IDENTITY")
if ! xcrun notarytool history --keychain-profile "$PROFILE" >/dev/null 2>&1; then
    echo "No notarytool credentials named \"$PROFILE\". Store them once with:" >&2
    echo "  xcrun notarytool store-credentials $PROFILE --apple-id <your Apple ID> --team-id $TEAM_ID" >&2
    echo "or set NOTARY_PROFILE to an existing profile." >&2
    exit 1
fi

make bundle TIMESTAMP=--timestamp SIGN_ID="$IDENTITY"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
APP=dist/ExplainIt.app
ZIP="dist/ExplainIt-$VERSION.zip"

rm -f "$ZIP"
ditto -c -k --keepParent "$APP" "$ZIP"
echo "Submitting $ZIP to Apple's notary service (usually a few minutes)…"
RESULT=$(xcrun notarytool submit "$ZIP" --keychain-profile "$PROFILE" --wait --output-format json)
STATUS=$(plutil -extract status raw -o - - <<<"$RESULT")
if [ "$STATUS" != "Accepted" ]; then
    ID=$(plutil -extract id raw -o - - <<<"$RESULT")
    echo "Notarization finished with status \"$STATUS\". Apple's log:" >&2
    xcrun notarytool log "$ID" --keychain-profile "$PROFILE" >&2
    exit 1
fi

# Attach the ticket so Gatekeeper can verify the app offline, then zip the stapled app.
xcrun stapler staple "$APP"
rm -f "$ZIP"
ditto -c -k --keepParent "$APP" "$ZIP"
spctl --assess --type execute --verbose=2 "$APP"
echo "SHA-256: $(shasum -a 256 "$ZIP" | cut -d' ' -f1)"
echo "Ready: $ZIP"
