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
    # #86: recorded handoffs, using the same signal envelope and actor headers as task_record.py.
    # Refusal is fatal: never widen the fixture token's permissions to seed task records.
    node - "$dir" "$now" <<'NODE'
const fs = require('fs');
const [dir, at] = process.argv.slice(2);
const records = [
  ['901-claimed', 'lane-a', 'task.claimed', { taskId: 'issue-901', issue: 901, lane: 'lane-a', branch: 'issue-901-fixture', assignedBy: 'lead-901' }],
  ['901-review', 'lane-a', 'task.review_assigned', { taskId: 'issue-901', pr: 903, headSha: 'aaaaaaaa', reviewer: 'lane-b', ordinal: 1 }],
  ['902-claimed', 'lane-c', 'task.claimed', { taskId: 'issue-902', issue: 902, lane: 'lane-c', branch: 'issue-902-fixture' }],
];
for (const [id, lane, type, fields] of records) {
  const document = { schema: 'graphhelm-task-event-v1', revision: 1, at, ...fields };
  const signal = { id: `fixture-${id}`, type, source: { type: 'user', id: lane }, severity: 'low',
    emittedAt: at, evidence: ['demo'], description: JSON.stringify(document) };
  fs.writeFileSync(`${dir}/.graphhelm/task-${id}.json`, JSON.stringify({ signal }));
}
NODE
    for record in 901-claimed 901-review 902-claimed; do
      actor=lane-a; [ "$record" != 902-claimed ] || actor=lane-c
      curl -sf --max-time 10 -X POST "http://127.0.0.1:$rport/v1/executions/demo/signal" -H "Authorization: Bearer $token" \
        -H "Content-Type: application/json" -H "Idempotency-Key: fixture-$record" -H "X-GraphHelm-Actor: $actor" -H "X-GraphHelm-Actor-Type: agent" \
        --data-binary @"$dir/.graphhelm/task-$record.json" > "$dir/.graphhelm/task-$record.out" \
        || { echo "fixture: task record $record refused; stop without widening permissions" >&2; exit 1; }
      node -e 'const r=JSON.parse(require("fs").readFileSync(process.argv[1],"utf8")); if(r.ok!==true) { console.error("fixture: task record refused; stop without widening permissions"); process.exit(1); }' "$dir/.graphhelm/task-$record.out"
    done
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
  # #137: the async Runtime driver records delegation; no model executor is wired.
  if [ "$flow" = studio-delegation-label ]; then
    node - "$repo" "$dir" <<'NODE'
const fs = require('fs');
const [repo, dir] = process.argv.slice(2);
let graph = fs.readFileSync(`${repo}/examples/graphs/manual-override-deploy.yaml`, 'utf8').replace(/\r\n/g, '\n');
graph = graph.replace(/exec_override_graph_v13/g, 'routing-fixture-v1')
  .replace('executionId: exec_override', 'executionId: routing-fixture')
  .replace('version: 13', 'version: 1').replace(/^  basedOn:.*\n/m, '')
  .replace(/implementation/g, 'routed-worker-137')
  .replace('      type: agent', '      delegation: {kind: implementer}\n      type: agent')
  .replace(/      name: .*\n/, '      name: Routed worker\n')
  // A deploy node selects the legacy synchronous driver, which does not record delegation.
  .replace(/    deploy:\n[\s\S]*?  budgets:/, '  edges: []\n  budgets:')
  .replace('      - deploy', '      - routed-worker-137');
fs.writeFileSync(`${dir}/.graphhelm/routing-graph.yaml`, graph);
NODE
    printf '%s\n' '{"nodeOutcomes":{"routed-worker-137":"failure"}}' > "$dir/.graphhelm/routing-fixtures.json"
    node - "$dir" "$rport" <<'NODE'
const fs = require('fs');
const path = require('path');
const [dir, port] = process.argv.slice(2);
const token = fs.readFileSync(`${dir}/.graphhelm/events.token`, 'utf8').trim();
fetch(`http://127.0.0.1:${port}/v1/executions/routing-fixture/start`, {
  method: 'POST',
  headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json',
    'Idempotency-Key': 'routing-fixture-start', 'X-GraphHelm-Actor': 'studio-operator', 'X-GraphHelm-Actor-Type': 'owner' },
  body: JSON.stringify({ file: path.resolve(dir, '.graphhelm/routing-graph.yaml'),
    fixtures: path.resolve(dir, '.graphhelm/routing-fixtures.json'), mode: 'supervised' }),
  signal: AbortSignal.timeout(30000),
}).then(async response => {
  const result = await response.json();
  fs.writeFileSync(`${dir}/.graphhelm/routing-seed.json`, JSON.stringify(result));
  if (!response.ok || result.ok !== true) throw new Error('delegation fixture start refused');
}).catch(() => { console.error('fixture: delegation seed failed'); process.exitCode = 1; });
NODE
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
  # R6: the launcher reads this private data file after up; never echo the minted token.
  (umask 077; printf 'GRAPHHELM_SECRET_STUDIO_TOKEN=%s\n' "$(head -1 "$dir/.graphhelm/events.token")" > "$dir/.graphhelm/secrets.env")
  chmod 600 "$dir/.graphhelm/secrets.env"
  echo "fixture up: runtime http://127.0.0.1:$rport, studio http://127.0.0.1:$sport/?session=studio-fixture"
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
