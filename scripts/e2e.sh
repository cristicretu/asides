#!/bin/bash
# End-to-end test: the installed Asides.app (built with dev hooks) driven through the `asides`
# CLI in a throwaway git repo. WAV fixtures stand in for the microphone (`asides simulate`).
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
FIX="$ROOT/tests/fixtures"
CLI="${ASIDES_CLI:-$HOME/.local/bin/asides}"
WORK=$(mktemp -d)
trap '"$CLI" quit >/dev/null 2>&1 || true; rm -rf "$WORK"' EXIT

pass() { printf '  \033[32m✓\033[0m %s\n' "$1"; }
fail() { printf '  \033[31m✗\033[0m %s\n' "$1"; exit 1; }
check() { # description, command...
    local what=$1; shift
    if "$@" >/dev/null 2>&1; then pass "$what"; else fail "$what"; fi
}
contains() { grep -qF -- "$2" "$1"; }
wait_for() { # file, text
    for _ in $(seq 1 120); do
        [ -f "$1" ] && grep -qF -- "$2" "$1" && return 0
        sleep 0.5
    done
    echo "--- $1"; cat "$1" 2>/dev/null || true
    fail "timed out waiting for \"$2\""
}

REPO="$WORK/shop"
mkdir -p "$REPO/src"
git -C "$REPO" init -q -b feat/checkout
git -C "$REPO" config user.email e2e@example.com
git -C "$REPO" config user.name E2E
echo "app" > "$REPO/src/app.js"
git -C "$REPO" add .
git -C "$REPO" commit -q -m "Fix cart total rounding"
git -C "$REPO" remote add origin git@github.com:acme/shop.git
cd "$REPO/src" # binding from a subdirectory must bind the repo root

echo "Bind"
out=$("$CLI")
echo "$out" | sed 's/^/    /'
check "app started and bound to the repo root" grep -q "Asides ready in .*/shop$" <<<"$out"
check "no notes yet" grep -q "first recording creates a note" <<<"$out"
check ".asides/ excluded locally" grep -qx ".asides/" "$REPO/.git/info/exclude"

echo "New titled note: English recording + screenshot"
check "asides new acknowledges the title" grep -q 'new note "Checkout session"' <<<"$("$CLI" new "Checkout session")"
"$CLI" simulate "$FIX/en_bug.wav" --image "$FIX/screenshot.png" --at 6.5 >/dev/null
for _ in $(seq 1 120); do NOTE=$("$CLI" path 2>/dev/null) && break; sleep 0.5; done
wait_for "$NOTE" "## 1 ·"
check "note folder named after the title" grep -q "/.asides/[0-9-]* Checkout session/notes.md$" <<<"$NOTE"
check "front matter: title" contains "$NOTE" "title: Checkout session"
check "front matter: git branch" contains "$NOTE" "branch: feat/checkout"
check "front matter: git remote" contains "$NOTE" 'remote: "git@github.com:acme/shop.git"'
check "front matter: commit subject" contains "$NOTE" "commit_subject: Fix cart total rounding"
check "front matter: macOS version" grep -q "^  macos: " "$NOTE"
check "front matter: display" grep -q "^  display: [0-9]*x[0-9]* @" "$NOTE"
check "front matter: transcription" contains "$NOTE" "whisper large-v3-turbo (local)"
check "English transcript" contains "$NOTE" "padding on the summary card"
check "screenshot linked" grep -q '^!\[[0-9:]*\](images/01-[0-9]*\.png)$' "$NOTE"
check "screenshot saved unchanged" cmp "$FIX/screenshot.png" "$(dirname "$NOTE")/$(sed -n 's/^!\[.*\](\(images\/.*\))$/\1/p' "$NOTE" | head -1)"

echo "Append a Romanian recording to the latest note"
check "bind reports the latest note" grep -q 'appending to "Checkout session" (1 entry)' <<<"$("$CLI")"
"$CLI" simulate "$FIX/ro_bug.wav" >/dev/null
wait_for "$NOTE" "## 2 ·"
check "Romanian transcript with diacritics" contains "$NOTE" "Butonul de salvare nu face nimic după ce schimb parola."
check "metadata written only once" test "$(grep -c '^title:' "$NOTE")" = 1
check "same note" test "$("$CLI" path)" = "$NOTE"

echo "New untitled note: titled after the first sentence"
"$CLI" new >/dev/null
"$CLI" simulate "$FIX/ro_bug.wav" >/dev/null
for _ in $(seq 1 120); do NOTE2=$("$CLI" path) && [ "$NOTE2" != "$NOTE" ] && break; sleep 0.5; done
wait_for "$NOTE2" "## 1 ·"
check "title from the first sentence" contains "$NOTE2" "title: Butonul de salvare nu face nimic după ce schimb parola"
check "first note untouched" test "$(grep -c '^## ' "$NOTE")" = 2

echo "Status, export, git hygiene"
check "status shows the binding" grep -q "bound to .*/shop" <<<"$("$CLI" status)"
"$CLI" export --out "$WORK/note.zip" >/dev/null
zip_names() { /usr/bin/python3 -I -c 'import sys, zipfile; print("\n".join(i.filename + ("" if i.flag_bits & 0x800 or i.filename.isascii() else " !utf8") for i in zipfile.ZipFile(sys.argv[1]).infolist()))' "$1"; }
check "export zips notes.md" grep -q "/notes.md$" <<<"$(zip_names "$WORK/note.zip")"
check "zip names are UTF-8 flagged" test -z "$(grep "!utf8" <<<"$(zip_names "$WORK/note.zip")")"
check "zip has no macOS metadata files" test -z "$(grep -E "/\._|DS_Store" <<<"$(zip_names "$WORK/note.zip")")"
check "notes never show up in git status" test -z "$(git -C "$REPO" status --porcelain)"

echo "Switch between repos (the menu's Directory list)"
OTHER="$WORK/admin"
mkdir -p "$OTHER"
git -C "$OTHER" init -q -b main
recent() { /usr/bin/python3 -I -c 'import json, os, sys; print("\n".join(os.path.basename(d) for d in json.load(open(sys.argv[1]))["recent_dirs"][:2]))' "$HOME/Library/Application Support/Asides/state.json"; }
"$CLI" -C "$OTHER" >/dev/null
check "binding another repo puts it first" test "$(recent | tr '\n' ' ')" = "admin shop "
check "the new repo has no notes yet" grep -q "No notes in" <<<"$("$CLI" -C "$OTHER" status)"
"$CLI" >/dev/null
check "switching back reorders the list" test "$(recent | tr '\n' ' ')" = "shop admin "
check "and appends to the same note again" grep -q 'appending to "Butonul' <<<"$("$CLI")"

echo "Quit"
"$CLI" quit >/dev/null
sleep 1
check "app quit" grep -q "not running" <<<"$("$CLI" status)"

echo
echo "Note 1:"
sed 's/^/    /' "$NOTE"
echo "All end-to-end checks passed."
