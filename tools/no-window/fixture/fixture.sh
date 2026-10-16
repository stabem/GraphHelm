#!/bin/sh
# No-window launcher fixture (#1290): `up <dir> <runtime port> <app port>` starts one local Node
# HTTP child, `down <dir>` stops only that child. No install, database, Docker or network.
# When NOWINDOW_MARKS names a file, each reached step appends its name there for the harness.
mark() { [ -n "$NOWINDOW_MARKS" ] && echo "$1" >> "$NOWINDOW_MARKS"; return 0; }
here=$(cd "$(dirname "$0")" && pwd)
case "$1" in
up)
  mkdir -p "$2" || exit 1
  nohup node "$here/server.mjs" "$4" >"$2/server.log" 2>&1 &
  echo $! > "$2/pid"
  i=0
  while [ $i -lt 50 ]; do
    grep -q '^listening' "$2/server.log" 2>/dev/null && { mark "up $(cat "$2/pid")"; exit 0; }
    i=$((i + 1))
    sleep 0.1
  done
  echo "fixture server did not start" >&2
  exit 1
  ;;
down)
  mark down
  [ -f "$2/pid" ] || exit 0
  kill "$(cat "$2/pid")" 2>/dev/null
  rm -f "$2/pid"
  ;;
*)
  echo "usage: fixture.sh up <dir> <runtime port> <app port> | down <dir>" >&2
  exit 2
  ;;
esac
exit 0
