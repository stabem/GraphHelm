#!/usr/bin/env bash
# The fixture the `studio-*` journey flows are written against (#381): a disposable Runtime with
# one seeded run and one unanswered agent question, and this repository's Studio dev server
# pointed at it. `graphhelm journey replay studio-<id>` runs against what this script starts.
#
#   tools/studio-journey-fixture/fixture.sh up   <fixture dir> [runtime port] [studio port]
#   tools/studio-journey-fixture/fixture.sh down <fixture dir>
#
# Defaults: Runtime 127.0.0.1:8797, Studio 127.0.0.1:5184 (the flows' `base`). Both must be free.
# Needs: `graphhelm` on PATH (or GRAPHHELM_BIN), node and npm, the Studio's dependencies installed
# (`npm --prefix apps/studio ci`). Secrets stay in files under <fixture dir>/.graphhelm; the
# script never prints them. The Studio's session nonce is fixed (`studio-fixture`) so the page
# opens connected: http://127.0.0.1:<studio port>/?session=studio-fixture
set -euo pipefail
repo=$(cd "$(dirname "$0")/../.." && pwd)
bin=${GRAPHHELM_BIN:-graphhelm}
cmd=${1:-}; dir=${2:-}; rport=${3:-8797}; sport=${4:-5184}
[ -n "$cmd" ] && [ -n "$dir" ] || { sed -n 2,12p "$0"; exit 2; }
mkdir -p "$dir"; dir=$(cd "$dir" && pwd)
pids="$dir/.graphhelm/fixture.pids"

case "$cmd" in
up)
  if [ ! -d "$dir/.git" ]; then
    git -C "$dir" init -q
    printf '# demo\n' > "$dir/README.md"
    git -C "$dir" add -A && git -C "$dir" -c user.email=demo@example.com -c user.name=demo commit -qm init
  fi
  "$bin" --json init --project "$dir" --bind "127.0.0.1:$rport" --harness codex > "$dir/.graphhelm/init.json"
  export GRAPHHELM_EVENTS_KEY="$(cat "$dir/.graphhelm/serve.key")"
  nohup "$bin" serve --events "$dir/.graphhelm/events" --bind "127.0.0.1:$rport" --project "$dir" \
    --keyring "$dir/.graphhelm/keyring" --key-id studio > "$dir/.graphhelm/serve.out" 2> "$dir/.graphhelm/serve.err" &
  echo "serve $!" > "$pids"
  for _ in $(seq 1 30); do curl -sf "http://127.0.0.1:$rport/health" > /dev/null && break; sleep 1; done
  curl -sf "http://127.0.0.1:$rport/health" > /dev/null || { echo "runtime did not answer on $rport"; cat "$dir/.graphhelm/serve.err"; exit 1; }
  # The seeded run: the manual-override example, one blocked node, one ready node.
  if ! grep -q '"executionId":"demo"' "$dir/.graphhelm/seed.json" 2>/dev/null; then
    echo '{"nodeOutcomes":{"implementation":"failure"}}' > "$dir/.graphhelm/fixtures.json"
    "$bin" --json execution start --file "$repo/examples/graphs/manual-override-deploy.yaml" \
      --events "$dir/.graphhelm/events" --fixtures "$dir/.graphhelm/fixtures.json" --mode supervised --execution demo \
      > "$dir/.graphhelm/seed.json"
    # One unanswered question from the bot `planner` to the owner (studio-answer-question, studio-name-bot).
    token=$(head -1 "$dir/.graphhelm/events.token"); id=$(node -e 'console.log(require("crypto").randomUUID())')
    now=$(date -u +%Y-%m-%dT%H:%M:%SZ)
    printf '{"signal":{"id":"planner-question-%s","source":{"type":"tool","id":"planner"},"type":"operator_note","severity":"low","description":"Should the deploy go to staging first, or straight to production?","recommendations":["Staging first","Straight to production"],"evidence":["demo"],"emittedAt":"%s","to":"studio-operator"}}' "$id" "$now" > "$dir/.graphhelm/question.json"
    curl -sf -X POST "http://127.0.0.1:$rport/v1/executions/demo/signal" -H "Authorization: Bearer $token" \
      -H "Content-Type: application/json" -H "Idempotency-Key: $id" -H "X-GraphHelm-Actor: planner" -H "X-GraphHelm-Actor-Type: agent" \
      --data-binary @"$dir/.graphhelm/question.json" > "$dir/.graphhelm/question.out"
  fi
  export GRAPHHELM_EVENTS="$dir/.graphhelm/events" GRAPHHELM_RUNTIME_URL="http://127.0.0.1:$rport" \
    GRAPHHELM_STUDIO_SESSION_NONCE=studio-fixture GRAPHHELM_PROJECT=demo
  nohup npm --prefix "$repo/apps/studio" run dev -- --port "$sport" --strictPort > "$dir/.graphhelm/studio.out" 2>&1 &
  echo "studio $!" >> "$pids"
  for _ in $(seq 1 60); do curl -sf -o /dev/null "http://127.0.0.1:$sport/__studio/session?nonce=studio-fixture" && break; sleep 1; done
  curl -sf -o /dev/null "http://127.0.0.1:$sport/__studio/session?nonce=studio-fixture" || { echo "studio did not answer on $sport"; tail -20 "$dir/.graphhelm/studio.out"; exit 1; }
  echo "fixture up: runtime http://127.0.0.1:$rport, studio http://127.0.0.1:$sport/?session=studio-fixture"
  echo "replay secret: export GRAPHHELM_SECRET_STUDIO_TOKEN=\"\$(head -1 '$dir/.graphhelm/events.token')\""
  ;;
down)
  [ -f "$pids" ] || { echo "nothing recorded in $pids"; exit 0; }
  while read -r name pid; do
    if [ "$(uname -s | cut -c1-5)" = "MINGW" ] || [ "$(uname -s | cut -c1-6)" = "CYGWIN" ]; then taskkill //PID "$pid" //T //F > /dev/null 2>&1 || true
    else kill "$pid" 2> /dev/null || true; fi
    echo "stopped $name ($pid)"
  done < "$pids"
  rm -f "$pids"
  ;;
*) echo "unknown command: $cmd"; exit 2 ;;
esac
