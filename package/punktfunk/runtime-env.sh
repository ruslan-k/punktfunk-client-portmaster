#!/bin/bash
# Sourced by both PortMaster launchers.

GAMEDIR=${GAMEDIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)}
STATE="$GAMEDIR/state"
RUNTIME="$GAMEDIR/runtime"
CONFIG="$GAMEDIR/config.env"

mkdir -p "$STATE/home" "$STATE/config" "$STATE/cache" "$STATE/data" "$RUNTIME"

if [ -f "$CONFIG" ]; then
  # shellcheck disable=SC1090
  source "$CONFIG"
fi

export HOME="$STATE/home"
export XDG_CONFIG_HOME="$STATE/config"
export XDG_CACHE_HOME="$STATE/cache"
export XDG_DATA_HOME="$STATE/data"

if [ -z "${XDG_RUNTIME_DIR:-}" ]; then
  export XDG_RUNTIME_DIR="/tmp/punktfunk-${UID:-0}"
  mkdir -p "$XDG_RUNTIME_DIR"
  chmod 700 "$XDG_RUNTIME_DIR" 2>/dev/null || true
fi

export PATH="$GAMEDIR/bin:$PATH"
export LD_LIBRARY_PATH="$RUNTIME:$GAMEDIR/libs:${LD_LIBRARY_PATH:-}"

# SDL3's upstream spelling is SDL_VIDEO_DRIVER. Keep SDL_VIDEODRIVER too for
# older firmware wrappers that still key off the SDL2-compatible variable.
case "${PUNKTFUNK_VIDEO_DRIVER:-auto}" in
  x11)
    export SDL_VIDEO_DRIVER=x11
    export SDL_VIDEODRIVER=x11
    ;;
  wayland)
    export SDL_VIDEO_DRIVER=wayland
    export SDL_VIDEODRIVER=wayland
    ;;
  auto|*)
    if [ -n "${DISPLAY:-}" ]; then
      export SDL_VIDEO_DRIVER=x11
      export SDL_VIDEODRIVER=x11
    elif [ -n "${WAYLAND_DISPLAY:-}" ]; then
      export SDL_VIDEO_DRIVER=wayland
      export SDL_VIDEODRIVER=wayland
    fi
    ;;
esac

# TSPS images have shipped Mali Vulkan entry points without the conventional
# libvulkan.so.1 loader name. Prefer a real Vulkan loader when present; otherwise
# expose the Mali library under the name ash/SDL3 dlopen.
if ! (
  [ -e /usr/lib/libvulkan.so.1 ] ||
  [ -e /usr/lib64/libvulkan.so.1 ] ||
  [ -e /lib/libvulkan.so.1 ] ||
  [ -e /lib64/libvulkan.so.1 ]
); then
  for mali in \
    /usr/lib/libmali.so.0.32.0 \
    /usr/lib/libmali.so.0 \
    /usr/lib/libmali.so \
    /usr/lib64/libmali.so.0 \
    /usr/lib64/libmali.so
  do
    if [ -e "$mali" ]; then
      ln -sfn "$mali" "$RUNTIME/libvulkan.so.1"
      break
    fi
  done
fi

export PUNKTFUNK_DECODER=${PUNKTFUNK_DECODER:-software}
export RUST_LOG=${RUST_LOG:-info}

if [ -n "${sdl_controllerconfig:-}" ]; then
  export SDL_GAMECONTROLLERCONFIG="$sdl_controllerconfig"
fi
