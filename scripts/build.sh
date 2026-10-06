#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck disable=SC1091
source "$ROOT/sources.env"

BUILD=$ROOT/.build
SRC=$BUILD/src
PREFIX=/opt/target
TARGET_DIR=$BUILD/target
STAGE=$BUILD/stage
DIST=$ROOT/dist

rm -rf "$BUILD" "$DIST" "$PREFIX"
mkdir -p "$SRC" "$PREFIX" "$TARGET_DIR" "$STAGE" "$DIST"

echo "==> source: Punktfunk $PUNKTFUNK_REF"
git clone --filter=blob:none "$PUNKTFUNK_REPO" "$SRC/punktfunk"
git -C "$SRC/punktfunk" checkout --detach "$PUNKTFUNK_REF"
PUNKTFUNK_ACTUAL=$(git -C "$SRC/punktfunk" rev-parse HEAD)
test "$PUNKTFUNK_ACTUAL" = "$PUNKTFUNK_REF"

echo "==> source: SDL $SDL_TAG"
git clone --depth 1 --branch "$SDL_TAG" "$SDL_REPO" "$SRC/SDL"
SDL_ACTUAL=$(git -C "$SRC/SDL" rev-parse HEAD)

echo "==> build SDL3 for aarch64"
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_LIBDIR="$PREFIX/lib/pkgconfig:/usr/lib/aarch64-linux-gnu/pkgconfig:/usr/share/pkgconfig"
export PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig"
export CC=aarch64-linux-gnu-gcc
export CXX=aarch64-linux-gnu-g++
export AR=aarch64-linux-gnu-ar
export RANLIB=aarch64-linux-gnu-ranlib

cmake -S "$SRC/SDL" -B "$BUILD/sdl-build" -G Ninja \
  -DCMAKE_SYSTEM_NAME=Linux \
  -DCMAKE_SYSTEM_PROCESSOR=aarch64 \
  -DCMAKE_C_COMPILER=aarch64-linux-gnu-gcc \
  -DCMAKE_CXX_COMPILER=aarch64-linux-gnu-g++ \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_INSTALL_PREFIX="$PREFIX" \
  -DSDL_SHARED=ON \
  -DSDL_STATIC=OFF \
  -DSDL_TESTS=OFF \
  -DSDL_TEST_LIBRARY=OFF \
  -DSDL_X11=ON \
  -DSDL_X11_XSCRNSAVER=OFF \
  -DSDL_X11_XTEST=OFF \
  -DSDL_WAYLAND=ON \
  -DSDL_KMSDRM=ON \
  -DSDL_PIPEWIRE=OFF \
  -DSDL_ALSA=ON

cmake --build "$BUILD/sdl-build" --parallel "$(nproc)"
cmake --install "$BUILD/sdl-build"

test -f "$PREFIX/lib/libSDL3.so.0"

echo "==> install SpruceOS embedded audio backend"
python3 - "$SRC/punktfunk" "$ROOT" <<'PY'
from pathlib import Path
import re
import shutil
import sys

root = Path(sys.argv[1])
port = Path(sys.argv[2])
cargo = root / "crates/pf-client-core/Cargo.toml"
text = cargo.read_text(encoding="utf-8")

# The handheld backend is ALSA. Keep the optional PipeWire dependency in the
# manifest/lockfile for a byte-stable upstream Cargo.lock, but do not activate
# it from the desktop feature. That prevents libspa bindgen from compiling.
text, n_feature = re.subn(r'"dep:pipewire",\s*', "", text, count=1)
if n_feature != 1:
    raise SystemExit(
        f"unexpected upstream Cargo.toml shape: pipewire feature refs={n_feature}"
    )
cargo.write_text(text, encoding="utf-8")

# The console's presenter dependency otherwise re-enables PyroWave transitively,
# despite the session's --no-default-features. Keep the H.264-only port feature
# closure explicit instead of building an unrelated desktop codec toolchain.
console_cargo = root / "crates/pf-console-ui/Cargo.toml"
console_text = console_cargo.read_text(encoding="utf-8")
old = 'pf-presenter = { path = "../pf-presenter", optional = true }'
new = 'pf-presenter = { path = "../pf-presenter", optional = true, default-features = false }'
if console_text.count(old) != 1:
    raise SystemExit("unexpected console presenter dependency")
console_cargo.write_text(console_text.replace(old, new), encoding="utf-8")

shutil.copyfile(
    port / "patches/audio-alsa.rs",
    root / "crates/pf-client-core/src/audio.rs",
)
shutil.copyfile(
    port / "patches/pad_audio-embedded.rs",
    root / "crates/pf-client-core/src/pad_audio.rs",
)
print("installed ALSA playback backend and embedded pad-audio stub")
PY

echo "==> cross-build Punktfunk CLI + gamepad console session"
export PATH="/root/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$TARGET_DIR"
# GitHub runner/container HTTP2 occasionally fails while fetching the sparse
# crates.io index. Use HTTP/1.1 and bounded Cargo retries, retaining --locked.
export CARGO_HTTP_MULTIPLEXING=false
export CARGO_NET_RETRY=5
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
export CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++
export AR_aarch64_unknown_linux_gnu=aarch64-linux-gnu-ar
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=-Wl,--as-needed"
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_LIBDIR="$PREFIX/lib/pkgconfig:/usr/lib/aarch64-linux-gnu/pkgconfig:/usr/share/pkgconfig"
export PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I/usr/include/aarch64-linux-gnu -I/usr/aarch64-linux-gnu/include"

cd "$SRC/punktfunk"
rustup override set "$RUST_TOOLCHAIN"
rustup target add "$TARGET" --toolchain "$RUST_TOOLCHAIN"
rustup component add rustfmt --toolchain "$RUST_TOOLCHAIN"

# Make the copied backend pass upstream formatting before compiling it.
rustfmt --edition 2024 \
  crates/pf-client-core/src/audio.rs \
  crates/pf-client-core/src/pad_audio.rs

cargo build --locked --release --target "$TARGET" \
  -p punktfunk-cli \
  -p punktfunk-client-session \
  --no-default-features \
  --features punktfunk-client-session/ui
echo "==> stage PortMaster package"
cp -a "$ROOT/package/." "$STAGE/"
mkdir -p "$STAGE/punktfunk/bin" "$STAGE/punktfunk/libs" "$STAGE/punktfunk/licenses"

cp "$TARGET_DIR/$TARGET/release/punktfunk" "$STAGE/punktfunk/bin/"
cp "$TARGET_DIR/$TARGET/release/punktfunk-session" "$STAGE/punktfunk/bin/"
cp -L "$PREFIX/lib/libSDL3.so.0" "$STAGE/punktfunk/libs/libSDL3.so.0"

bash "$ROOT/scripts/collect-libs.sh" \
  "$STAGE/punktfunk/libs" \
  "$STAGE/punktfunk/bin/punktfunk" \
  "$STAGE/punktfunk/bin/punktfunk-session" \
  "$STAGE/punktfunk/libs/libSDL3.so.0"

for license in LICENSE-MIT LICENSE-APACHE; do
  if [ -f "$SRC/punktfunk/$license" ]; then
    cp "$SRC/punktfunk/$license" "$STAGE/punktfunk/licenses/"
  fi
done
if [ -f "$SRC/punktfunk/clients/linux/THIRD-PARTY-NOTICES.txt" ]; then
  cp "$SRC/punktfunk/clients/linux/THIRD-PARTY-NOTICES.txt" \
    "$STAGE/punktfunk/licenses/THIRD-PARTY-NOTICES.txt"
fi

cat > "$STAGE/punktfunk/SOURCES.txt" <<EOF
Punktfunk repository: $PUNKTFUNK_REPO
Punktfunk commit:     $PUNKTFUNK_ACTUAL
SDL repository:       $SDL_REPO
SDL tag:              $SDL_TAG
SDL commit:           $SDL_ACTUAL
Rust toolchain:       $RUST_TOOLCHAIN
Target:               $TARGET
Build base:           $BUILD_BASE
Audio backend:        ALSA (SpruceOS embedded patch)
SpruceOS nightly:     $SPRUCEOS_NIGHTLY_TAG
SpruceOS commit:      $SPRUCEOS_NIGHTLY_COMMIT
Port repository SHA:  ${GITHUB_SHA:-local}
EOF

chmod +x "$STAGE/Punktfunk.sh" "$STAGE/Punktfunk Setup.sh" \
  "$STAGE/punktfunk/runtime-env.sh" \
  "$STAGE/punktfunk/bin/punktfunk" "$STAGE/punktfunk/bin/punktfunk-session"

bash "$ROOT/scripts/validate-package.sh" "$STAGE"

echo "==> archive"
(
  cd "$STAGE"
  zip -9 -r "$DIST/punktfunk.zip" .
)

sha256sum "$DIST/punktfunk.zip" > "$DIST/SHA256SUMS"

python3 - "$DIST/build-info.json" <<PY
import json, os, sys
out = {
    "punktfunk_repo": "$PUNKTFUNK_REPO",
    "punktfunk_commit": "$PUNKTFUNK_ACTUAL",
    "sdl_repo": "$SDL_REPO",
    "sdl_tag": "$SDL_TAG",
    "sdl_commit": "$SDL_ACTUAL",
    "rust_toolchain": "$RUST_TOOLCHAIN",
    "target": "$TARGET",
    "build_base": "$BUILD_BASE",
    "audio_backend": "alsa-spruceos",
    "spruceos_nightly_tag": "$SPRUCEOS_NIGHTLY_TAG",
    "spruceos_nightly_commit": "$SPRUCEOS_NIGHTLY_COMMIT",
    "github_sha": os.environ.get("GITHUB_SHA", "local"),
}
with open(sys.argv[1], "w", encoding="utf-8") as f:
    json.dump(out, f, indent=2)
    f.write("\n")
PY

echo "==> done"
cat "$DIST/SHA256SUMS"
