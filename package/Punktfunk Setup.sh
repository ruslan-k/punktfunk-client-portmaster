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
LOG="$LOGDIR/setup-$(date +%Y%m%d-%H%M%S).log"

cd "$GAMEDIR" || exit 1
# shellcheck disable=SC1091
source "$GAMEDIR/runtime-env.sh"

PF="$GAMEDIR/bin/punktfunk"

dlg() {
  dialog "$@" 3>&1 1>&2 2>&3
}

message() {
  if command -v dialog >/dev/null 2>&1; then
    dialog --title "Punktfunk" --msgbox "$1" 12 68
  else
    echo "$1"
  fi
}

if ! command -v dialog >/dev/null 2>&1; then
  {
    echo "The 'dialog' command is not available on this firmware."
    echo "Pair from SSH instead:"
    echo "  cd $GAMEDIR"
    echo "  source ./runtime-env.sh"
    echo "  ./bin/punktfunk discover"
    echo "  ./bin/punktfunk pair HOST"
    echo "  ./bin/punktfunk default-host HOST"
  } | tee -a "$LOG"
  exit 3
fi

DISCOVERY=$("$PF" discover --timeout 4 2>>"$LOG" || true)

hosts=()
labels=()
while IFS=$'\t' read -r name addr saved paired rest; do
  [ -n "$addr" ] || continue
  hosts+=("$addr")
  labels+=("$name — $addr [$paired]")
done <<< "$DISCOVERY"

menu_args=()
for ((i=0; i<${#hosts[@]}; i++)); do
  menu_args+=("$((i+1))" "${labels[$i]}")
done
menu_args+=("M" "Enter host address manually")
menu_args+=("R" "Reset Punktfunk client state")
menu_args+=("Q" "Quit")

choice=$(dlg --backtitle "Punktfunk / TrimUI Smart Pro S" \
  --title "Host setup" \
  --menu "Select a host to pair or make default:" 18 76 10 \
  "${menu_args[@]}") || exit 0

case "$choice" in
  Q)
    exit 0
    ;;
  R)
    if dlg --title "Reset" --yesno "Forget all paired hosts and client settings?" 9 58; then
      "$PF" reset >>"$LOG" 2>&1 || true
      message "Punktfunk client state was reset."
    fi
    exit 0
    ;;
  M)
    host=$(dlg --title "Host" --inputbox "Host IP/name, optionally :port" 9 62 "") || exit 0
    ;;
  *)
    idx=$((choice-1))
    host=${hosts[$idx]}
    ;;
esac

[ -n "$host" ] || exit 0

# If this record is already paired, setting it as default is enough.
if "$PF" hosts list --probe 2>>"$LOG" | grep -F "$host" | grep -q "paired"; then
  if "$PF" default-host "$host" >>"$LOG" 2>&1; then
    message "Default host set to:\n\n$host"
    exit 0
  fi
fi

pin=$(dlg --title "Pair Punktfunk" \
  --inputbox "Enter the pairing PIN shown by the Punktfunk host/web console." \
  10 68 "") || exit 0

if [ -z "$pin" ]; then
  message "Pairing cancelled: empty PIN."
  exit 0
fi

if printf '%s\n' "$pin" | "$PF" pair "$host" --pin - --name "TrimUI Smart Pro S" >>"$LOG" 2>&1; then
  "$PF" default-host "$host" >>"$LOG" 2>&1 || true
  message "Paired successfully.\n\nDefault host: $host\n\nLaunch Punktfunk from Ports."
  exit 0
fi

message "Pairing failed. See:\n\n$LOG"
exit 4
