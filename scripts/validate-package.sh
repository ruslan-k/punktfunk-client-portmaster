#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
STAGE=${1:-$ROOT/.build/stage}

test -f "$STAGE/port.json"
test -x "$STAGE/Punktfunk.sh"
test -x "$STAGE/Punktfunk Setup.sh"
test -x "$STAGE/punktfunk/bin/punktfunk"
test -x "$STAGE/punktfunk/bin/punktfunk-session"
test -f "$STAGE/punktfunk/libs/libSDL3.so.0"

jq -e '.version == 3' "$STAGE/port.json" >/dev/null
jq -e '.attr.arch | index("aarch64") != null' "$STAGE/port.json" >/dev/null
jq -e '.attr.exp == true' "$STAGE/port.json" >/dev/null

file "$STAGE/punktfunk/bin/punktfunk" | grep -qi 'ARM aarch64'
file "$STAGE/punktfunk/bin/punktfunk-session" | grep -qi 'ARM aarch64'

aarch64-linux-gnu-readelf -h "$STAGE/punktfunk/bin/punktfunk-session" | grep -q 'AArch64'

echo "PortMaster package validation passed."
