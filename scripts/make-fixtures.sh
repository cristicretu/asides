#!/bin/bash
# Generates the speech fixtures (macOS system voices) and a small PNG used by the tests.
# They are not committed: the macOS license only allows system-voice audio for personal use.
set -euo pipefail
mkdir -p "$(dirname "$0")/../tests/fixtures"
cd "$(dirname "$0")/../tests/fixtures"

if ! say -v '?' | grep -q '^Ioana '; then
    echo "The Romanian voice \"Ioana\" is missing. Add it in System Settings → Accessibility →" >&2
    echo "Spoken Content → System Voice → Manage Voices…, then run \`make fixtures\` again." >&2
    exit 1
fi

wav() { say ${3:+-v "$3"} -o "$1" --file-format=WAVE --data-format=LEI16@16000 "$2"; }

wav en_bug.wav "Okay, so on the checkout page the padding on the summary card should be bigger, like twenty four pixels, to compensate for the sticky header. Also the error toast disappears too quickly."
wav ro_bug.wav "Butonul de salvare nu face nimic după ce schimb parola. Mesajul de eroare apare sub formular, dar textul e tăiat pe ecrane mici." Ioana

/usr/bin/python3 -I - <<'EOF'
import struct, zlib
w, h = 64, 40
raw = b"".join(b"\x00" + b"".join(bytes((x * 4 % 256, y * 6 % 256, 200)) for x in range(w)) for y in range(h))
def chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")
open("screenshot.png", "wb").write(png)
EOF
ls -la
