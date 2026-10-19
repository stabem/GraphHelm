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
if [ "$cmd" = up ]; then
  [[ "$rport" =~ ^[0-9]+$ ]] && (( rport > 0 && rport < 65536 )) && [ "$rport" != 8793 ] || { echo 'fixture: unsafe Runtime port' >&2; exit 2; }
  [[ "$sport" =~ ^[0-9]+$ ]] && (( sport > 0 && sport < 65536 )) && [ "$sport" != "$rport" ] || { echo 'fixture: unsafe Studio port' >&2; exit 2; }
fi
mkdir -p "$dir"; dir=$(cd "$dir" && pwd)
. "$(dirname "$0")/wait.sh"
pids="$dir/.graphhelm/fixture.pids"
rt="$dir/project/.graphhelm"

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
  # A healthy pre-existing listener is never a fixture owned by this invocation.
  if curl -fsS --max-time 1 "http://127.0.0.1:$rport/health" >/dev/null 2>&1; then echo 'fixture: Runtime port already occupied' >&2; exit 2; fi
  # Record the launcher first: down must stop it before it can spawn any more children.
  mkdir -p "$dir/.graphhelm"
  echo "up $$ $(cat /proc/$$/winpid 2> /dev/null || true)" > "$pids"
  echo "ports $rport $sport" >> "$pids"
  # #560 review: a missing dependency fails here, named, instead of hanging a later step.
  command -v node > /dev/null || { echo "fixture: node is not on PATH" >&2; exit 3; }
  command -v npm > /dev/null || { echo "fixture: npm is not on PATH" >&2; exit 3; }
  [ -d "$repo/apps/studio/node_modules" ] || { echo "fixture: $repo/apps/studio has no node_modules (npm --prefix apps/studio ci)" >&2; exit 3; }
  # The Runtime reads journeys and delivered documents from a disposable project. Copy the
  # journeys byte-for-byte; never let a document save reach the source repository (#585).
  # Its events, keys and tokens live in that project's own .graphhelm: `serve` refuses events
  # that sit in another folder's .graphhelm (#1298). <dir>/.graphhelm keeps the fixture's files.
  mkdir -p "$dir/project/.graphhelm" "$dir/project/docs"
  if [ ! -d "$dir/project/.git" ]; then
    git -C "$dir/project" init -q
    printf '# demo\n' > "$dir/project/README.md"
    git -C "$dir/project" add -A && git -C "$dir/project" -c user.email=demo@example.com -c user.name=demo commit -qm init
  fi
  "$bin" --json init --project "$dir/project" --bind "127.0.0.1:$rport" --harness codex > "$dir/.graphhelm/init.json"
  export GRAPHHELM_FIXTURE_RUNTIME_PORT="$rport" GRAPHHELM_EVENTS_KEY="$(cat "$rt/serve.key")"
  cp -R "$repo/.graphhelm/journeys" "$dir/project/.graphhelm/"
  # #1041: the files the flows name in `scope`, so a fixture preview is not refused flow.scope_path_missing.
  node "$(dirname "$0")/copy_scope.mjs" "$repo" "$dir/project" > /dev/null
  # #1635: an owner approval needs a preview recorded at the flow's scope commit, the last commit
  # touching the flow and its scope; untracked copies have none, so commit them (never the keys).
  git -C "$dir/project" add -f -- .graphhelm/journeys
  git -C "$dir/project" add -A -- apps docs
  git -C "$dir/project" diff --cached --quiet \
    || git -C "$dir/project" -c user.email=demo@example.com -c user.name=demo commit -qm 'fixture: journeys and their scope'
  # #1448: the browser observer the fixture's own previews (the Studio's Run again) need, set up as
  # `setup --install-observer playwright` does: the driver and judge from this checkout, and the
  # pinned @playwright/test resolvable from the project (the runner clears NODE_PATH). Offline cache
  # first; when npm cannot install it, previews still end OBSERVER_MISSING rather than failing up.
  mkdir -p "$dir/project/.graphhelm/observers"
  cp "$repo/tools/journey-driver/driver.mjs" "$dir/project/.graphhelm/observers/journey_driver.mjs"
  cp "$repo/tools/journey-driver/judge.mjs" "$dir/project/.graphhelm/observers/judge.mjs"
  [ -f "$dir/project/package.json" ] || printf '{"private":true}\n' > "$dir/project/package.json"
  npm --prefix "$dir/project" install --save-dev --prefer-offline --no-audit --no-fund \
    @playwright/test@1.64.0 > "$dir/.graphhelm/observer-install.log" 2>&1 \
    || echo "fixture: @playwright/test not installed (see $dir/.graphhelm/observer-install.log); previews end OBSERVER_MISSING" >&2
  printf '# Release notes: version 0.1\n' > "$dir/project/docs/RELEASE_NOTES.md"
  # #585: a route manifest of the fixture's own, listed and written by the Studio's Models panel
  # (Add model). Listing only: `--gateway-manifest` wires no executor, so nodes stay on fixtures.
  cp "$repo/examples/gateway/astra-routes.json" "$rt/routes.json"
  # #1448: the studio-* flows sign in with this token; a preview the Runtime starts hands its
  # runner every GRAPHHELM_SECRET_* it holds. Set for serve only, never printed.
  GRAPHHELM_SECRET_STUDIO_TOKEN="$(head -1 "$rt/events.token")" \
  nohup "$bin" serve --events "$rt/events" --bind "127.0.0.1:$rport" --project "$dir/project" \
    --keyring "$rt/keyring" --key-id studio --gateway-manifest "$rt/routes.json" \
    > "$dir/.graphhelm/serve.out" 2> "$dir/.graphhelm/serve.err" &
  echo "serve $! $(cat /proc/$!/winpid 2> /dev/null)" >> "$pids"
  for _ in $(seq 1 30); do curl -sf --max-time 10 "http://127.0.0.1:$rport/health" > /dev/null && break; sleep 1; done
  curl -sf --max-time 10 "http://127.0.0.1:$rport/health" > /dev/null || { echo "runtime did not answer on $rport"; cat "$dir/.graphhelm/serve.err"; exit 1; }
  [ -n "$(fixture_listeners record "$rport")" ] || { echo 'fixture: Runtime listener is not owned by this fixture' >&2; exit 1; }
  for pid in $(fixture_listeners record "$rport"); do
    if [ -r /proc/$$/winpid ]; then echo "serve-listener 0 $pid"; else echo "serve-listener $pid"; fi >> "$pids"
  done
  # The seeded run: the manual-override example, one blocked node, one ready node.
  if ! grep -q '"executionId":"demo"' "$dir/.graphhelm/seed.json" 2>/dev/null; then
    echo '{"nodeOutcomes":{"implementation":"failure"}}' > "$dir/.graphhelm/fixtures.json"
    "$bin" --json execution start --file "$repo/examples/graphs/manual-override-deploy.yaml" \
      --events "$rt/events" --fixtures "$dir/.graphhelm/fixtures.json" --mode supervised --execution demo \
      > "$dir/.graphhelm/seed.json"
    printf '%s\n' '{"version":1,"summary":"Release notes ready for review.","reason":"Fixture document for the owner to edit.","documents":[{"path":"docs/RELEASE_NOTES.md","title":"Release notes","kind":"file","action":"created"}]}' > "$dir/.graphhelm/delivery.json"
    "$bin" --json execution delivery --events "$rt/events" --execution demo --node deploy \
      --delivery "$dir/.graphhelm/delivery.json" --project-directory "$dir/project" \
      --keyring "$rt/keyring" --key-id studio > "$dir/.graphhelm/delivery.out"
    # One unanswered question from the bot `planner` to the owner (studio-answer-question).
    token=$(head -1 "$rt/events.token"); id=$(node -e 'console.log(require("crypto").randomUUID())')
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
  flow=${GRAPHHELM_JOURNEY_FLOW:-}
  if [ "$flow" = studio-work-graph ]; then
    # #721 task records use the existing planner actor, so these rows do not create fake agents.
    # Every refusal stops the seed; the fixture token's permissions are never widened.
    node - "$dir" "$now" "http://127.0.0.1:$rport" "$rt/events.token" <<'NODE'
const fs = require('fs');
const [dir, at, runtimeUrl, tokenFile] = process.argv.slice(2);
const runtime = new URL(runtimeUrl);
if (runtime.protocol !== 'http:' || runtime.hostname !== '127.0.0.1' || !runtime.port || runtime.port === '8793' || runtime.pathname !== '/') {
  throw new Error('fixture: unsafe Runtime URL');
}
const common = { schema: 'graphhelm-task-event-v1', revision: 1, at };
const journey = [process.env.GRAPHHELM_JOURNEY_FLOW || 'studio-work-graph'];
const records = [
  ['open-claimed', 'task.claimed', { taskId: 'issue-721-open', issue: 721, lane: 'planner', branch: 'issue-721-open-fixture', journeys: journey, title: 'Observe issue graph and work kanban' }],
  ['open-planned', 'task.planned', { taskId: 'issue-721-open', lane: 'planner', classes: ['user_visible'], reviews: 1, proof: 'journey', critic: { mode: 'none', passScore: 7, maxRounds: 1 }, summary: 'Fixture plan' }],
  ['open-pr', 'task.pr_opened', { taskId: 'issue-721-open', pr: 721, lane: 'planner', headSha: '721abcde', journeys: journey, title: 'Observe issue graph and work kanban' }],
  ['merged-claimed', 'task.claimed', { taskId: 'issue-721-merged', issue: 721, lane: 'planner', branch: 'issue-721-merged-fixture', journeys: journey, title: 'Merged fixture row' }],
  ['merged-pr', 'task.pr_opened', { taskId: 'issue-721-merged', pr: 722, lane: 'planner', headSha: '722abcde', journeys: journey, title: 'Merged fixture row' }],
  ['merged-review', 'task.review_verdict', { taskId: 'issue-721-merged', pr: 722, headSha: '722abcde', reviewer: 'planner', verdict: 'APPROVE', commentUrl: 'https://github.com/stabem/GraphHelm/pull/722#review' }],
  ['merged-done', 'task.merged', { taskId: 'issue-721-merged', pr: 722, mergeSha: '722fedcb', closes: [], merger: 'planner' }],
];
const token = fs.readFileSync(tokenFile, 'utf8').trim();
(async () => {
  for (const [id, type, fields] of records) {
    const document = { ...common, ...fields };
    const signal = { id: `fixture-721-${id}`, type, source: { type: 'user', id: 'planner' }, severity: 'low',
      emittedAt: at, evidence: ['demo'], description: JSON.stringify(document) };
    const response = await fetch(new URL('/v1/executions/demo/signal', runtime), {
      method: 'POST', headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json',
        'Idempotency-Key': `fixture-721-${id}`, 'X-GraphHelm-Actor': 'planner', 'X-GraphHelm-Actor-Type': 'agent' },
      body: JSON.stringify({ signal }), signal: AbortSignal.timeout(10000),
    });
    const result = await response.json();
    fs.writeFileSync(`${dir}/.graphhelm/task-${id}.out`, JSON.stringify(result));
    if (!response.ok || result.ok !== true) throw new Error(`task seed refused: ${id}`);
  }
})().catch(() => { console.error('fixture: task record refused; stop without widening permissions'); process.exit(1); });
NODE
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
const token = fs.readFileSync(`${dir}/project/.graphhelm/events.token`, 'utf8').trim();
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
  export GRAPHHELM_EVENTS="$rt/events" GRAPHHELM_RUNTIME_URL="http://127.0.0.1:$rport" \
    GRAPHHELM_STUDIO_SESSION_NONCE=studio-fixture GRAPHHELM_PROJECT=demo
  nohup npm --prefix "$repo/apps/studio" run dev -- --port "$sport" --strictPort > "$dir/.graphhelm/studio.out" 2>&1 &
  echo "studio $! $(cat /proc/$!/winpid 2> /dev/null)" >> "$pids"
  wait_answers "http://127.0.0.1:$sport/__studio/session?nonce=studio-fixture" "$!" 60 \
    || { echo "studio did not answer on $sport"; tail -20 "$dir/.graphhelm/studio.out"; exit 1; }
  for pid in $(fixture_listeners record "$sport"); do
    if [ -r /proc/$$/winpid ]; then echo "studio-listener 0 $pid"; else echo "studio-listener $pid"; fi >> "$pids"
  done
  # R6: the launcher reads this private data file after up; never echo the minted token.
  (umask 077; printf 'GRAPHHELM_SECRET_STUDIO_TOKEN=%s\n' "$(head -1 "$rt/events.token")" > "$dir/.graphhelm/secrets.env")
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
