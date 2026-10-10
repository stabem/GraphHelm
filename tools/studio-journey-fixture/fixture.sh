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

# Resolve live listeners, never a port alone: a crashed fixture's port may have been reused.
# Vite's command names this checkout and its exact strict port; serve names the fixture dir.
fixture_listeners() {
  local action=$1; shift
  if [ -r /proc/$$/winpid ]; then
    GH_FIXTURE_DIR="$(cygpath -am "$dir")" GH_FIXTURE_REPO="$(cygpath -am "$repo")" \
      GH_FIXTURE_PORTS="$*" GH_FIXTURE_ACTION="$action" powershell.exe -NoProfile -NonInteractive -Command "$(cat <<'PS'
$ErrorActionPreference = 'Stop'
$fixtureDir = $env:GH_FIXTURE_DIR.Replace('\','/').ToLowerInvariant().TrimEnd('/') + '/'
$vite = $env:GH_FIXTURE_REPO.Replace('\','/').ToLowerInvariant().TrimEnd('/') + '/apps/studio/'
$ports = @($env:GH_FIXTURE_PORTS -split ' ' | Where-Object { $_ -match '^\d+$' -and $_ -notin @('5196','8793') })
$processes = @{}
Get-CimInstance Win32_Process | ForEach-Object { $processes[[string]$_.ProcessId] = $_.CommandLine }
$listeners = netstat -ano | Where-Object { $_ -match 'LISTENING' }
foreach ($line in $listeners) {
    $fields = $line.Trim() -split '\s+'
    if ($fields[1] -notmatch '^127\.0\.0\.1:(\d+)$' -or $Matches[1] -notin $ports) { continue }
    $port = $Matches[1]
    $listenerId = $fields[-1]
    $commandLine = $processes[$listenerId]
    if (-not $commandLine) { continue }
    $commandLine = $commandLine.Replace('\','/').ToLowerInvariant()
    $owned = $commandLine.Contains($fixtureDir) -or ($commandLine.Contains($vite) -and $commandLine.Contains('/vite/bin/vite.js') -and $commandLine -match (('--port\s+{0}\s+--strictport(?:\s|$)' -f $port)))
    if (-not $owned) { continue }
    if ($env:GH_FIXTURE_ACTION -eq 'kill') {
        & taskkill.exe /PID $listenerId /T /F 2>$null | Out-Null
    } else {
        Write-Output $listenerId
    }
}
PS
)" | tr -d '\r'
  elif command -v lsof > /dev/null; then
    local port pid line
    for port in "$@"; do
      case "$port" in 5196|8793) continue ;; esac
      for pid in $(lsof -t -iTCP@127.0.0.1:"$port" -sTCP:LISTEN 2>/dev/null | sort -u); do
        line=$(ps -p "$pid" -o args= || true)
        if [[ "$line" == *"$dir/"* ]] || [[ "$line" == *"$repo/apps/studio/"* && ( "$line" == *"/vite/bin/vite.js"* || "$line" == *"/node_modules/.bin/vite "* ) && "$line " == *"--port $port --strictPort "* ]]; then
          if [ "$action" = kill ]; then kill "$pid" 2>/dev/null || true; else echo "$pid"; fi
        fi
      done
    done
  fi
}

case "$cmd" in
up)
  # Record the launcher first: down must stop it before it can spawn any more children.
  mkdir -p "$dir/.graphhelm"
  echo "up $$ $(cat /proc/$$/winpid 2> /dev/null || true)" > "$pids"
  echo "ports $rport $sport" >> "$pids"
  # #560 review: a missing dependency fails here, named, instead of hanging a later step.
  command -v node > /dev/null || { echo "fixture: node is not on PATH" >&2; exit 3; }
  command -v npm > /dev/null || { echo "fixture: npm is not on PATH" >&2; exit 3; }
  [ -d "$repo/apps/studio/node_modules" ] || { echo "fixture: $repo/apps/studio has no node_modules (npm --prefix apps/studio ci)" >&2; exit 3; }
  if [ ! -d "$dir/.git" ]; then
    git -C "$dir" init -q
    printf '# demo\n' > "$dir/README.md"
    git -C "$dir" add -A && git -C "$dir" -c user.email=demo@example.com -c user.name=demo commit -qm init
  fi
  "$bin" --json init --project "$dir" --bind "127.0.0.1:$rport" --harness codex > "$dir/.graphhelm/init.json"
  export GRAPHHELM_EVENTS_KEY="$(cat "$dir/.graphhelm/serve.key")"
  # The Runtime reads journeys and delivered documents from a disposable project. Copy the
  # journeys byte-for-byte; never let a document save reach the source repository (#585).
  mkdir -p "$dir/project/.graphhelm" "$dir/project/docs"
  cp -R "$repo/.graphhelm/journeys" "$dir/project/.graphhelm/"
  printf '# Release notes: version 0.1\n' > "$dir/project/docs/RELEASE_NOTES.md"
  git -C "$dir/project" init -q
  # #585: a route manifest of the fixture's own, listed and written by the Studio's Models panel
  # (Add model). Listing only: `--gateway-manifest` wires no executor, so nodes stay on fixtures.
  cp "$repo/examples/gateway/astra-routes.json" "$dir/.graphhelm/routes.json"
  nohup "$bin" serve --events "$dir/.graphhelm/events" --bind "127.0.0.1:$rport" --project "$dir/project" \
    --keyring "$dir/.graphhelm/keyring" --key-id studio --gateway-manifest "$dir/.graphhelm/routes.json" \
    > "$dir/.graphhelm/serve.out" 2> "$dir/.graphhelm/serve.err" &
  echo "serve $! $(cat /proc/$!/winpid 2> /dev/null)" >> "$pids"
  for _ in $(seq 1 30); do curl -sf --max-time 10 "http://127.0.0.1:$rport/health" > /dev/null && break; sleep 1; done
  curl -sf --max-time 10 "http://127.0.0.1:$rport/health" > /dev/null || { echo "runtime did not answer on $rport"; cat "$dir/.graphhelm/serve.err"; exit 1; }
  for pid in $(fixture_listeners record "$rport"); do
    if [ -r /proc/$$/winpid ]; then echo "serve-listener 0 $pid"; else echo "serve-listener $pid"; fi >> "$pids"
  done
  # The seeded run: the manual-override example, one blocked node, one ready node.
  if ! grep -q '"executionId":"demo"' "$dir/.graphhelm/seed.json" 2>/dev/null; then
    echo '{"nodeOutcomes":{"implementation":"failure"}}' > "$dir/.graphhelm/fixtures.json"
    "$bin" --json execution start --file "$repo/examples/graphs/manual-override-deploy.yaml" \
      --events "$dir/.graphhelm/events" --fixtures "$dir/.graphhelm/fixtures.json" --mode supervised --execution demo \
      > "$dir/.graphhelm/seed.json"
    printf '%s\n' '{"version":1,"summary":"Release notes ready for review.","reason":"Fixture document for the owner to edit.","documents":[{"path":"docs/RELEASE_NOTES.md","title":"Release notes","kind":"file","action":"created"}]}' > "$dir/.graphhelm/delivery.json"
    "$bin" --json execution delivery --events "$dir/.graphhelm/events" --execution demo --node deploy \
      --delivery "$dir/.graphhelm/delivery.json" --project-directory "$dir/project" \
      --keyring "$dir/.graphhelm/keyring" --key-id studio > "$dir/.graphhelm/delivery.out"
    # One unanswered question from the bot `planner` to the owner (studio-answer-question, studio-name-bot).
    token=$(head -1 "$dir/.graphhelm/events.token"); id=$(node -e 'console.log(require("crypto").randomUUID())')
    now=$(date -u +%Y-%m-%dT%H:%M:%SZ)
    printf '{"signal":{"id":"planner-question-%s","source":{"type":"tool","id":"planner"},"type":"operator_note","severity":"low","description":"Should the deploy go to staging first, or straight to production?","recommendations":["Staging first","Straight to production"],"evidence":["demo"],"emittedAt":"%s","to":"studio-operator"}}' "$id" "$now" > "$dir/.graphhelm/question.json"
    curl -sf --max-time 10 -X POST "http://127.0.0.1:$rport/v1/executions/demo/signal" -H "Authorization: Bearer $token" \
      -H "Content-Type: application/json" -H "Idempotency-Key: $id" -H "X-GraphHelm-Actor: planner" -H "X-GraphHelm-Actor-Type: agent" \
      --data-binary @"$dir/.graphhelm/question.json" > "$dir/.graphhelm/question.out"
    # #585: studio-handover shows "While you were away" after 20+ events since the owner was last
    # here; the flow sets that last-seen time in the browser, these are the events after it.
    for n in $(seq 1 22); do
      nid=$(node -e 'console.log(require("crypto").randomUUID())')
      printf '{"signal":{"id":"planner-progress-%s","source":{"type":"tool","id":"planner"},"type":"operator_note","severity":"low","description":"Progress note %s while you were away.","evidence":["demo"],"emittedAt":"%s"}}' "$nid" "$n" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$dir/.graphhelm/note.json"
      curl -sf --max-time 10 -X POST "http://127.0.0.1:$rport/v1/executions/demo/signal" -H "Authorization: Bearer $token" \
        -H "Content-Type: application/json" -H "Idempotency-Key: $nid" -H "X-GraphHelm-Actor: planner" -H "X-GraphHelm-Actor-Type: agent" \
        --data-binary @"$dir/.graphhelm/note.json" > /dev/null
    done
  fi
  # Seed a governed wait for the answer-node flow only.
  flow=${GRAPHHELM_JOURNEY_FLOW:-}
  if [ "$flow" = studio-answer-node ]; then
    printf '%s\n' '{"nodeOutcomes":{"implementation":"success","deploy":"unknown"}}' > "$dir/.graphhelm/waiting-fixtures.json"
    # Governed genesis starts at version 1 without a predecessor, with a matching run id.
    sed -e 's/^  executionId: exec_override[[:space:]]*$/  executionId: waiting/' \
      -e 's/^  version: 13[[:space:]]*$/  version: 1/' -e '/^  basedOn:/d' \
      "$repo/examples/graphs/manual-override-deploy.yaml" > "$dir/.graphhelm/waiting-graph.yaml"
    "$bin" --json execution start --file "$dir/.graphhelm/waiting-graph.yaml" \
      --events "$dir/.graphhelm/events" --fixtures "$dir/.graphhelm/waiting-fixtures.json" \
      --mode supervised --execution waiting --keyring "$dir/.graphhelm/keyring" --key-id studio \
      > "$dir/.graphhelm/waiting-seed.json"
  fi
  export GRAPHHELM_EVENTS="$dir/.graphhelm/events" GRAPHHELM_RUNTIME_URL="http://127.0.0.1:$rport" \
    GRAPHHELM_STUDIO_SESSION_NONCE=studio-fixture GRAPHHELM_PROJECT=demo
  nohup npm --prefix "$repo/apps/studio" run dev -- --port "$sport" --strictPort > "$dir/.graphhelm/studio.out" 2>&1 &
  echo "studio $! $(cat /proc/$!/winpid 2> /dev/null)" >> "$pids"
  for _ in $(seq 1 60); do curl -sf --max-time 10 -o /dev/null "http://127.0.0.1:$sport/__studio/session?nonce=studio-fixture" && break; sleep 1; done
  curl -sf --max-time 10 -o /dev/null "http://127.0.0.1:$sport/__studio/session?nonce=studio-fixture" || { echo "studio did not answer on $sport"; tail -20 "$dir/.graphhelm/studio.out"; exit 1; }
  for pid in $(fixture_listeners record "$sport"); do
    if [ -r /proc/$$/winpid ]; then echo "studio-listener 0 $pid"; else echo "studio-listener $pid"; fi >> "$pids"
  done
  echo "fixture up: runtime http://127.0.0.1:$rport, studio http://127.0.0.1:$sport/?session=studio-fixture"
  echo "replay secret: export GRAPHHELM_SECRET_STUDIO_TOKEN=\"\$(head -1 '$dir/.graphhelm/events.token')\""
  ;;
down)
  # Each record is `<name> <shell pid> [<OS pid>]`. On Windows (Git Bash) the shell pid is an MSYS
  # pid the OS does not know, so the OS pid taken from /proc/<pid>/winpid at start is used.
  [ -f "$pids" ] || { echo "nothing recorded in $pids"; exit 0; }
  ports=$(sed -n 's/^ports //p' "$pids")
  if [ "$(uname -s | cut -c1-5)" = "MINGW" ] || [ "$(uname -s | cut -c1-6)" = "CYGWIN" ]; then
    # The first record is up: kill its tree before the children, so no new child escapes.
    winpids=$(awk '$1 != "ports" && $3 != "" {print $3}' "$pids")
    for pid in $winpids; do
      taskkill //PID "$pid" //T //F > /dev/null 2>&1 && echo "stopped pid $pid" || true
    done
  else
    while read -r name pid _; do
      [ "$name" = ports ] && continue
      if [ "$name" = up ]; then
        # Freeze the launcher before ending its children: it cannot start Vite after down.
        kill -STOP "$pid" 2>/dev/null || true
        pkill -TERM -P "$pid" 2>/dev/null || true
        kill -KILL "$pid" 2>/dev/null || true
      else
        kill "$pid" 2>/dev/null && echo "stopped $name ($pid)" || true
      fi
    done < "$pids"
  fi
  # npm/nohup's recorded OS pid may already have exited while its Vite child still listens.
  # Always check both ports after the recorded kills; the command-line check protects neighbours.
  fixture_listeners kill $ports
  rm -f "$pids"
  ;;
*) echo "unknown command: $cmd"; exit 2 ;;
esac
