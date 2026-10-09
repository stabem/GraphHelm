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
  # #560 review: a missing dependency fails here, named, instead of hanging a later step.
  command -v node > /dev/null || { echo "fixture: node is not on PATH" >&2; exit 3; }
  command -v npm > /dev/null || { echo "fixture: npm is not on PATH" >&2; exit 3; }
  [ -d "$repo/apps/studio/node_modules" ] || { echo "fixture: $repo/apps/studio has no node_modules (npm --prefix apps/studio ci)" >&2; exit 3; }
  if [ ! -d "$dir/.git" ]; then
    git -C "$dir" init -q
    printf '# demo\n' > "$dir/README.md"
    git -C "$dir" add -A && git -C "$dir" -c user.email=demo@example.com -c user.name=demo commit -qm init
  fi
  mkdir -p "$dir/.graphhelm"
  "$bin" --json init --project "$dir" --bind "127.0.0.1:$rport" --harness codex > "$dir/.graphhelm/init.json"
  export GRAPHHELM_EVENTS_KEY="$(cat "$dir/.graphhelm/serve.key")"
  # `--project` is the repository: the Runtime's GET /v1/journeys and /v1/journeys/flows read
  # <project>/.graphhelm/journeys, where the studio-* flows and contracts live. Events, token,
  # key and keyring stay under the fixture directory.
  # #585: a route manifest of the fixture's own, listed and written by the Studio's Models panel
  # (Add model). Listing only: `--gateway-manifest` wires no executor, so nodes stay on fixtures.
  cp "$repo/examples/gateway/astra-routes.json" "$dir/.graphhelm/routes.json"
  nohup "$bin" serve --events "$dir/.graphhelm/events" --bind "127.0.0.1:$rport" --project "$repo" \
    --keyring "$dir/.graphhelm/keyring" --key-id studio --gateway-manifest "$dir/.graphhelm/routes.json" \
    > "$dir/.graphhelm/serve.out" 2> "$dir/.graphhelm/serve.err" &
  echo "ports $rport $sport" > "$pids"
  echo "serve $!" >> "$pids"
  for _ in $(seq 1 30); do curl -sf --max-time 10 "http://127.0.0.1:$rport/health" > /dev/null && break; sleep 1; done
  curl -sf --max-time 10 "http://127.0.0.1:$rport/health" > /dev/null || { echo "runtime did not answer on $rport"; cat "$dir/.graphhelm/serve.err"; exit 1; }
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
    curl -sf --max-time 10 -X POST "http://127.0.0.1:$rport/v1/executions/demo/signal" -H "Authorization: Bearer $token" \
      -H "Content-Type: application/json" -H "Idempotency-Key: $id" -H "X-GraphHelm-Actor: planner" -H "X-GraphHelm-Actor-Type: agent" \
      --data-binary @"$dir/.graphhelm/question.json" > "$dir/.graphhelm/question.out"
  fi
  export GRAPHHELM_EVENTS="$dir/.graphhelm/events" GRAPHHELM_RUNTIME_URL="http://127.0.0.1:$rport" \
    GRAPHHELM_STUDIO_SESSION_NONCE=studio-fixture GRAPHHELM_PROJECT=demo
  nohup npm --prefix "$repo/apps/studio" run dev -- --port "$sport" --strictPort > "$dir/.graphhelm/studio.out" 2>&1 &
  echo "studio $!" >> "$pids"
  for _ in $(seq 1 60); do curl -sf --max-time 10 -o /dev/null "http://127.0.0.1:$sport/__studio/session?nonce=studio-fixture" && break; sleep 1; done
  curl -sf --max-time 10 -o /dev/null "http://127.0.0.1:$sport/__studio/session?nonce=studio-fixture" || { echo "studio did not answer on $sport"; tail -20 "$dir/.graphhelm/studio.out"; exit 1; }
  echo "fixture up: runtime http://127.0.0.1:$rport, studio http://127.0.0.1:$sport/?session=studio-fixture"
  echo "replay secret: export GRAPHHELM_SECRET_STUDIO_TOKEN=\"\$(head -1 '$dir/.graphhelm/events.token')\""
  ;;
down)
  # The recorded pids are the shell's; on Windows (Git Bash) they are MSYS pids, not the ones the
  # OS knows, so the listeners are found by port instead. Only the fixture's two ports are touched.
  [ -f "$pids" ] || { echo "nothing recorded in $pids"; exit 0; }
  ports=$(sed -n 's/^ports //p' "$pids")
  if [ "$(uname -s | cut -c1-5)" = "MINGW" ] || [ "$(uname -s | cut -c1-6)" = "CYGWIN" ]; then
    for port in $ports; do
      for pid in $(netstat -ano | grep LISTENING | grep -E "127\.0\.0\.1:$port " | awk '{print $NF}' | sort -u); do
        taskkill //PID "$pid" //T //F > /dev/null 2>&1 && echo "stopped port $port (pid $pid)"
      done
    done
  else
    while read -r name pid; do [ "$name" = ports ] && continue; kill "$pid" 2> /dev/null && echo "stopped $name ($pid)"; done < "$pids"
  fi
  rm -f "$pids"
  ;;
*) echo "unknown command: $cmd"; exit 2 ;;
esac
