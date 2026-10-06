#!/bin/bash

source "$(dirname -- "${BASH_SOURCE[0]}")/punktfunk/launcher-common.sh"

HOST_LINE=$("$GAMEDIR/bin/punktfunk" default-host || true)
HOST=$(printf '%s\n' "$HOST_LINE" | awk -F '\t' 'NF >= 2 && $1 != "none" { print $2; exit }')

if [ -z "$HOST" ]; then
  echo "No default host: opening the native gamepad host/pairing console."
  run_session --browse --fullscreen
  exit $?
fi

case "${PUNKTFUNK_START_MODE:-browse}" in
  desktop)
    "$GAMEDIR/bin/punktfunk" launch "$HOST" --exec --fullscreen
    rc=$?
    printf 'session_exit=%s\n' "$rc"
    exit "$rc"
    ;;
  browse|*)
    run_session --browse "$HOST" --fullscreen --stats
    exit $?
    ;;
esac
