#!/bin/bash
# Publishes dist/Asides-<version>.dmg (from `make release`) behind the website's download button:
# a Cloudflare Worker that only serves static files, at https://asides.<subdomain>.workers.dev.
# Needs Wrangler, logged in to the Cloudflare account that owns the Worker (`wrangler login`).
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
DMG="dist/Asides-$VERSION.dmg"
if [ ! -f "$DMG" ]; then
    echo "No $DMG; run \`make release\` first." >&2
    exit 1
fi

# The file name stays the same across versions, so the website's link never changes.
rm -rf dist/download
mkdir -p dist/download
cp "$DMG" dist/download/Asides.dmg
cp packaging/download/_redirects dist/download/
wrangler deploy --config packaging/download/wrangler.jsonc
