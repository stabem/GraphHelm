#!/bin/sh
# Isolated launcher for #1784: `up <dir> <runtime port> <app port>` seeds the state named by
# GRAPHHELM_JOURNEY_STATE into <dir>/site/status.json and serves <dir>/site on the app port. It
# confirms the state in <dir>/.graphhelm/state, except `lost`, which it cannot seed. Every call's
# flow and state are appended to $STATES_LOG so the test sees what each run was told.
case "$1" in
up)
  mkdir -p "$2/site" "$2/.graphhelm"
  printf '%s %s\n' "${GRAPHHELM_JOURNEY_FLOW:-}" "${GRAPHHELM_JOURNEY_STATE:--}" >> "$STATES_LOG"
  printf '{"state":"%s"}' "${GRAPHHELM_JOURNEY_STATE:-default}" > "$2/site/status.json"
  if [ -n "${GRAPHHELM_JOURNEY_STATE:-}" ] && [ "$GRAPHHELM_JOURNEY_STATE" != lost ]; then
    printf '%s' "$GRAPHHELM_JOURNEY_STATE" > "$2/.graphhelm/state"
  fi
  nohup python3 -m http.server "$4" --bind 127.0.0.1 --directory "$2/site" >/dev/null 2>&1 &
  echo $! > "$2/pid"
  ;;
down)
  [ -f "$2/pid" ] && kill "$(cat "$2/pid")" 2>/dev/null
  ;;
esac
exit 0
