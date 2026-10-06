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

# Firmware may ship no monospace fonts. Skia's console overlay requires one
# even though the main UI embeds Geist. Use only the port's font closure.
export FONTCONFIG_FILE="$GAMEDIR/fonts/fonts.conf"
export FONTCONFIG_PATH="$GAMEDIR/fonts"

if [ -z "${XDG_RUNTIME_DIR:-}" ]; then
  export XDG_RUNTIME_DIR="/tmp/punktfunk-${UID:-0}"
  mkdir -p "$XDG_RUNTIME_DIR"
  chmod 700 "$XDG_RUNTIME_DIR" 2>/dev/null || true
fi

export PATH="$GAMEDIR/bin:$PATH"
export LD_LIBRARY_PATH="$RUNTIME:$GAMEDIR/libs:${LD_LIBRARY_PATH:-}"

# SpruceOS owns audio routing (speaker vs Bluetooth). Its helper writes the
# .asoundrc into the HOME we just selected, so Punktfunk's ALSA "default" PCM
# follows the same route and volume plumbing as native emulators.
if [ -x /mnt/SDCARD/spruce/scripts/asound-setup.sh ]; then
  /mnt/SDCARD/spruce/scripts/asound-setup.sh "$HOME" >/dev/null 2>&1 || true
fi
export PUNKTFUNK_ALSA_DEVICE=${PUNKTFUNK_ALSA_DEVICE:-default}

# SDL3's upstream spelling is SDL_VIDEO_DRIVER. Keep SDL_VIDEODRIVER too for
# older firmware wrappers that still key off the SDL2-compatible variable.
case "${PUNKTFUNK_VIDEO_DRIVER:-auto}" in
  kmsdrm)
    export SDL_VIDEO_DRIVER=kmsdrm
    export SDL_VIDEODRIVER=kmsdrm
    ;;
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
    else
      # Menu-launched Spruce has no compositor. Its Mali ICD exposes
      # VK_KHR_display, not X11/Wayland WSI; use SDL's direct-display backend.
      export SDL_VIDEO_DRIVER=kmsdrm
      export SDL_VIDEODRIVER=kmsdrm
    fi
    ;;
esac

# Some TrimUI images expose the vendor Mali Vulkan entry points without a
# conventional loader soname. Prefer a real system loader. The libmali fallback
# is retained for device testing; if the nightly already provides a loader this
# block is a no-op.
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
