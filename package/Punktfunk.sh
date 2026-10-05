#!/bin/bash

XDG_DATA_HOME=${XDG_DATA_HOME:-$HOME/.local/share}

if [ -d "/opt/system/Tools/PortMaster/" ]; then
  controlfolder="/opt/system/Tools/PortMaster"
elif [ -d "/opt/tools/PortMaster/" ]; then
  controlfolder="/opt/tools/PortMaster"
elif [ -d "$XDG_DATA_HOME/PortMaster/" ]; then
  controlfolder="$XDG_DATA_HOME/PortMaster"
else
  controlfolder="/roms/ports/PortMaster"
fi

# shellcheck disable=SC1090
source "$controlfolder/control.txt"
# shellcheck disable=SC1090
source "$controlfolder/device_info.txt"
[ -f "$controlfolder/mod_$CFW_NAME.txt" ] && source "$controlfolder/mod_$CFW_NAME.txt"

get_controls

GAMEDIR="/$directory/ports/punktfunk"
LOGDIR="$GAMEDIR/logs"
mkdir -p "$LOGDIR"
LOG="$LOGDIR/punktfunk-$(date +%Y%m%d-%H%M%S).log"

cd "$GAMEDIR" || exit 1

# shellcheck disable=SC1091
source "$GAMEDIR/runtime-env.sh"

exec > >(tee -a "$LOG") 2>&1

echo "=== Punktfunk PortMaster ==="
date
uname -a
printf 'DISPLAY=%s WAYLAND_DISPLAY=%s SDL_VIDEO_DRIVER=%s\n' \
  "${DISPLAY:-}" "${WAYLAND_DISPLAY:-}" "${SDL_VIDEO_DRIVER:-auto}"
printf 'decoder=%s start_mode=%s\n' \
  "${PUNKTFUNK_DECODER:-auto}" "${PUNKTFUNK_START_MODE:-browse}"

HOST_LINE=$("$GAMEDIR/bin/punktfunk" default-host 2>>"$LOG" || true)
HOST=$(printf '%s\n' "$HOST_LINE" | awk -F '\t' 'NF >= 2 && $1 != "none" { print $2; exit }')

if [ -z "$HOST" ]; then
  echo "No paired/default Punktfunk host. Starting setup."
  "$GAMEDIR/../Punktfunk Setup.sh"
  HOST_LINE=$("$GAMEDIR/bin/punktfunk" default-host 2>>"$LOG" || true)
  HOST=$(printf '%s\n' "$HOST_LINE" | awk -F '\t' 'NF >= 2 && $1 != "none" { print $2; exit }')
fi

if [ -z "$HOST" ]; then
  echo "No host configured. Run Punktfunk Setup from Ports."
  exit 2
fi

echo "Using host: $HOST"

case "${PUNKTFUNK_START_MODE:-browse}" in
  desktop)
    exec "$GAMEDIR/bin/punktfunk" launch "$HOST" --exec --fullscreen
    ;;
  browse|*)
    exec "$GAMEDIR/bin/punktfunk-session" --browse "$HOST" --fullscreen --stats
    ;;
esac
