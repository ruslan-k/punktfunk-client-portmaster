#!/bin/bash
# Shared initialization. Resolve the payload relative to the selected menu entry,
# not HOME (Spruce changes HOME before invoking a port).
GAMEDIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd) || exit 1
LOGDIR="$GAMEDIR/logs"
mkdir -p "$LOGDIR" || exit 1
LOG="$LOGDIR/${PUNKTFUNK_LOG_PREFIX:-punktfunk}-$(date +%Y%m%d-%H%M%S)-$$.log"
exec >>"$LOG" 2>&1
trap 'rc=$?; printf "launcher_exit=%s\n" "$rc"' EXIT

printf '=== Punktfunk PortMaster (%s) ===\n' "${PUNKTFUNK_LOG_PREFIX:-punktfunk}"
date
uname -a

if [ -n "${PUNKTFUNK_CONTROL_FOLDER:-}" ]; then
  controlfolder="$PUNKTFUNK_CONTROL_FOLDER"
elif [ -f /mnt/SDCARD/App/PortMaster/control.txt ]; then
  controlfolder=/mnt/SDCARD/App/PortMaster
elif [ -d /opt/system/Tools/PortMaster ]; then
  controlfolder=/opt/system/Tools/PortMaster
elif [ -d /opt/tools/PortMaster ]; then
  controlfolder=/opt/tools/PortMaster
elif [ -d "${XDG_DATA_HOME:-$HOME/.local/share}/PortMaster" ]; then
  controlfolder="${XDG_DATA_HOME:-$HOME/.local/share}/PortMaster"
else
  controlfolder=/roms/ports/PortMaster
fi

if [ ! -f "$controlfolder/control.txt" ]; then
  echo "Missing PortMaster control.txt: $controlfolder"
  exit 1
fi
source "$controlfolder/control.txt"
[ -f "$controlfolder/device_info.txt" ] && source "$controlfolder/device_info.txt"
[ -f "$controlfolder/mod_${CFW_NAME:-}.txt" ] && source "$controlfolder/mod_${CFW_NAME:-}.txt"
get_controls
cd "$GAMEDIR" || exit 1
source "$GAMEDIR/runtime-env.sh"

printf 'GAMEDIR=%s DISPLAY=%s WAYLAND_DISPLAY=%s SDL_VIDEO_DRIVER=%s\n' \
  "$GAMEDIR" "${DISPLAY:-}" "${WAYLAND_DISPLAY:-}" "${SDL_VIDEO_DRIVER:-auto}"

run_session() {
  "$GAMEDIR/bin/punktfunk-session" "$@"
  local rc=$?
  printf 'session_exit=%s\n' "$rc"
  return "$rc"
}
