#!/bin/bash
# Prints the code-signing identity to use: a Developer ID Application certificate, else an
# Apple Development one, else the self-signed "ExplainIt Dev" (`make cert`), else "-" (ad hoc).
# Any stable identity keeps the microphone permission across rebuilds; ad hoc does not.
identities=$(security find-identity -v -p codesigning 2>/dev/null)
for kind in "Developer ID Application: " "Apple Development: " "ExplainIt Dev"; do
    match=$(sed -n "s/.*\"\(${kind}[^\"]*\)\".*/\1/p" <<<"$identities" | head -1)
    if [ -n "$match" ]; then
        echo "$match"
        exit 0
    fi
done
echo "-"
