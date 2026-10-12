#!/bin/sh
# Journey launcher: `up <dir> <runtime port> <app port>` starts the app, `down <dir>` stops it.
case "$1" in
up)
  mkdir -p "$2"
  nohup python3 server.py "$4" >/dev/null 2>&1 &
  echo $! > "$2/pid"
  ;;
down)
  [ -f "$2/pid" ] && kill "$(cat "$2/pid")" 2>/dev/null
  ;;
esac
exit 0
