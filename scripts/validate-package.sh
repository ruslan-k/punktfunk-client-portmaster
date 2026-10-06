#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
STAGE=${1:-$ROOT/.build/stage}

test -f "$STAGE/port.json"
test -x "$STAGE/Punktfunk.sh"
test ! -e "$STAGE/Punktfunk Setup.sh"
test -x "$STAGE/punktfunk/bin/punktfunk"
test -x "$STAGE/punktfunk/bin/punktfunk-session"
test -f "$STAGE/punktfunk/libs/libSDL3.so.0"
test -s "$STAGE/punktfunk/fonts/fonts.conf"
test -s "$STAGE/punktfunk/fonts/DejaVuSansMono.ttf"
test -s "$STAGE/punktfunk/fonts/DejaVuSans.ttf"
test -s "$STAGE/punktfunk/licenses/DejaVu-copyright.txt"
# A successful build without KMSDRM still cannot open a Spruce menu session.
strings "$STAGE/punktfunk/libs/libSDL3.so.0" > "$STAGE/.sdl-strings"
grep -qx 'kmsdrm' "$STAGE/.sdl-strings"
rm "$STAGE/.sdl-strings"
strings "$STAGE/punktfunk/bin/punktfunk-session" > "$STAGE/.session-strings"
if grep -q -- '--browse needs the console UI' "$STAGE/.session-strings"; then
  echo 'Invalid package: session was built without the ui feature' >&2
  exit 1
fi
# The native Cedar rung must be compiled in and addressable by its pin name:
# config.env defaults to `native-cedar`, so a build that lost the patch would
# ship a client whose decoder default names a rung it does not contain.
grep -q 'native-cedar' "$STAGE/.session-strings"
grep -q 'AddVDPlugin' "$STAGE/.session-strings"
rm "$STAGE/.session-strings"
bash -n "$STAGE/Punktfunk.sh" \
  "$STAGE/punktfunk/launcher-common.sh" "$STAGE/punktfunk/runtime-env.sh"

jq -e '.version == 3' "$STAGE/port.json" >/dev/null
jq -e '.attr.arch | index("aarch64") != null' "$STAGE/port.json" >/dev/null
jq -e '.attr.exp == true' "$STAGE/port.json" >/dev/null

file "$STAGE/punktfunk/bin/punktfunk" | grep -qi 'ARM aarch64'
file "$STAGE/punktfunk/bin/punktfunk-session" | grep -qi 'ARM aarch64'

aarch64-linux-gnu-readelf -h "$STAGE/punktfunk/bin/punktfunk-session" | grep -q 'AArch64'

echo "PortMaster package validation passed."
