#!/bin/sh
# Isolated launcher: `up <dir> <runtime port> <app port>` serves site/ on the app port.
case "$1" in
up)
  mkdir -p "$2"
  nohup python3 -m http.server "$4" --bind 127.0.0.1 --directory site >/dev/null 2>&1 &
  echo $! > "$2/pid"
  ;;
down)
  [ -f "$2/pid" ] && kill "$(cat "$2/pid")" 2>/dev/null
  ;;
esac
exit 0
