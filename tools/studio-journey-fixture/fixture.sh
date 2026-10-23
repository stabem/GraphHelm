#!/usr/bin/env bash
# The fixture the `studio2-*` journey flows are written against (#381, #1808): a disposable
# Runtime with one seeded run and one unanswered agent question, and this repository's Studio dev
# server pointed at it. `graphhelm journey replay studio2-<id>` runs against what this script starts.
# For studio2-two-projects (GRAPHHELM_JOURNEY_FLOW) it also starts a second project, `proj-two`, with
# its own Runtime on <runtime port>+2, and registers it with the Studio.
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
  # #1905: the Run flows press Run inside this fixture's Studio, which plays the copied flow at its
  # `base`. An isolated fixture answers on its own port, not the flows' 5184, so the copy names
  # this fixture's Studio; at 5184 nothing answers and the inner run ends watch.app_down.
  for f in "$dir/project/.graphhelm/journeys"/studio2-proof-run*.journey.yaml; do
    [ -f "$f" ] || continue
    sed "s#^base: http://127.0.0.1:5184\$#base: http://127.0.0.1:$sport#" "$f" > "$f.tmp" && mv "$f.tmp" "$f"
  done
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
  # #1556: the fixture's own build-slot root, so the Lanes board can show a real slot holder.
  mkdir -p "$dir/slots"
  # #1808: studio2-lanes-needs-action needs silent lanes; this Runtime counts a lane silent after
  # one minute instead of thirty, so the fixture comes up within the launcher's budget.
  if [ "${GRAPHHELM_JOURNEY_FLOW:-}" = studio2-lanes-needs-action ]; then export GRAPHHELM_FIXTURE_SILENT_AFTER_SECONDS=60; fi
  GRAPHHELM_SECRET_STUDIO_TOKEN="$(head -1 "$rt/events.token")" \
  nohup "$bin" serve --events "$rt/events" --bind "127.0.0.1:$rport" --project "$dir/project" \
    --keyring "$rt/keyring" --key-id studio --gateway-manifest "$rt/routes.json" --slot-root "$dir/slots" \
    > "$dir/.graphhelm/serve.out" 2> "$dir/.graphhelm/serve.err" &
  echo "serve $! $(cat /proc/$!/winpid 2> /dev/null)" >> "$pids"
  for _ in $(seq 1 30); do curl -sf --max-time 10 "http://127.0.0.1:$rport/health" > /dev/null && break; sleep 1; done
  curl -sf --max-time 10 "http://127.0.0.1:$rport/health" > /dev/null || { echo "runtime did not answer on $rport"; cat "$dir/.graphhelm/serve.err"; exit 1; }
  [ -n "$(fixture_listeners record "$rport")" ] || { echo 'fixture: Runtime listener is not owned by this fixture' >&2; exit 1; }
  # #1672: one recorded passing preview, so the Proof view has a real observer to name. A draft
  # preview plays only read-only acts, so the seed flow reads this fixture's own Runtime health
  # with GET; its runner records itself. FIXTURE-ONLY: it lives only in the fixture project, and
  # it is not evidence that a real project's preview records an observer.
  cat > "$dir/project/.graphhelm/journeys/runtime-health.journey.yaml" <<FLOW
schema: graphhelm.journey-flow/1
id: runtime-health
title: Read the Runtime health
purpose: The fixture's Runtime answers its health check.
status: draft
approved: null
base: http://127.0.0.1:$rport
actors: [owner]
secrets: []
risks: []
screens:
  - id: checked
    title: The health answer read again
    url: /health
    state: success
    expect: [{role: status, name: "200", because: a healthy Runtime answers its health route with HTTP 200}, {role: json, name: ok=true, because: the fixture Runtime is up and says so}, {role: json, name: command=serve.health, because: the answer comes from the Runtime's own health handler}]
    scope: unknown
  - id: start
    title: The Runtime answers its health check
    url: /health
    state: stable
    expect: [{role: status, name: "200", because: a healthy Runtime answers its health route with HTTP 200}, {role: json, name: ok=true, because: the fixture Runtime is up and says so}]
    scope: unknown
edges:
  - id: read
    from: start
    to: checked
    why: Reading the health route again gives the same healthy answer.
    acts:
      - {kind: http, role: GET, name: /health}
paths:
  main: [read]
drift: []
FLOW
  git -C "$dir/project" add -f -- .graphhelm/journeys/runtime-health.journey.yaml
  # #1909: the confirm flows need a draft whose step runs a command, so its preview holds on
  # confirm_needed until the owner confirms. FIXTURE-ONLY, and only for those flows, so no other
  # flow's journey count changes. The commands print Node's version and then,
  # in a second edge, a line after a wait, on two paths: a run long enough to be polled while it
  # plays, shaped like keel-card-accepted (#1912).
  case "${GRAPHHELM_JOURNEY_FLOW:-}" in studio2-proof-confirm)
    cat > "$dir/project/.graphhelm/journeys/cli-hello.journey.yaml" <<'FLOW'
schema: graphhelm.journey-flow/1
id: cli-hello
title: Print the Node version
name: Print the Node version
purpose: "The fixture's draft with one command step, which waits for the owner's confirmation."
status: draft
approved: null
base: http://127.0.0.1:5184
actors: [owner]
secrets: []
risks: []
screens:
  - id: printed
    title: Node printed its version
    url: /
    state: success
    expect: [{role: exit, name: "0", because: node --version succeeds}, {role: stdout, name: v, because: Node prints its version starting with v}]
    scope: unknown
  - id: start
    title: A shell is ready
    url: /
    state: stable
    expect: [{role: exit, name: "0", because: the entry step is only where the commands start}]
    scope: unknown
  - id: waited
    title: Node printed after a wait
    url: /
    state: success
    expect: [{role: exit, name: "0", because: the wait command succeeds}, {role: stdout, name: waited, because: the command prints waited after its four-second wait}]
    scope: unknown
edges:
  - id: printed.wait
    from: printed
    to: waited
    why: A second command that waits keeps the run playing long enough to be watched.
    acts:
      - {kind: cli, role: exec, name: "node -e setTimeout(()=>console.log('waited'),4000)"}
  - id: start.print
    from: start
    to: printed
    why: Node prints its version once the owner confirms the command.
    acts:
      - {kind: cli, role: exec, name: node --version}
paths:
  main: [start.print, printed.wait]
  again: [start.print, printed.wait]
drift: []
FLOW
    git -C "$dir/project" add -f -- .graphhelm/journeys/cli-hello.journey.yaml ;;
  esac
  # #1919: the Send to fix flows need a journey that fails: a draft reading this fixture's Runtime
  # health and expecting ok=false, which it never is. FIXTURE-ONLY, and only for those flows.
  case "${GRAPHHELM_JOURNEY_FLOW:-}" in studio2-proof-send-fix)
    cat > "$dir/project/.graphhelm/journeys/health-broken.journey.yaml" <<FLOW
schema: graphhelm.journey-flow/1
id: health-broken
title: A health check that always fails
name: Broken health check
purpose: "The fixture's failing draft: the Runtime answers ok=true, the flow expects ok=false."
status: draft
approved: null
base: http://127.0.0.1:$rport
actors: [owner]
secrets: []
risks: []
screens:
  - id: checked
    title: The health answer says not ok
    url: /health
    state: success
    expect: [{role: status, name: "200", because: the Runtime answers its health route with HTTP 200}, {role: json, name: ok=false, because: "deliberately wrong: the Runtime says ok=true, so this flow always fails"}]
    scope: unknown
  - id: start
    title: The Runtime answers its health check
    url: /health
    state: stable
    expect: [{role: status, name: "200", because: the Runtime answers its health route with HTTP 200}]
    scope: unknown
edges:
  - id: read
    from: start
    to: checked
    why: "The health check is read and judged against a wrong expectation, so the journey fails."
    acts:
      - {kind: http, role: GET, name: /health}
paths:
  main: [read]
drift: []
FLOW
    git -C "$dir/project" add -f -- .graphhelm/journeys/health-broken.journey.yaml ;;
  esac
  # #1953: the reload flow needs a run that is still playing after a fresh page opens: a draft
  # reading a FIXTURE-ONLY loopback server that answers every request after six seconds. Its pid
  # is recorded, so down stops it. Only for that flow, so no other flow's journey count changes.
  if [ "${GRAPHHELM_JOURNEY_FLOW:-}" = studio2-proof-run-reload ]; then
    rm -f "$dir/.graphhelm/slow.port"
    nohup node -e '
      const fs = require("fs"); const file = process.argv[1];
      const server = require("http").createServer((_, res) => setTimeout(() => { res.setHeader("content-type", "application/json"); res.end("{\"ok\":true}"); }, 6000));
      server.listen(0, "127.0.0.1", () => fs.writeFileSync(file, String(server.address().port)));
      setTimeout(() => process.exit(0), 3600000);
    ' "$dir/.graphhelm/slow.port" > /dev/null 2>&1 &
    echo "slow-server $! $(cat /proc/$!/winpid 2> /dev/null)" >> "$pids"
    for _ in $(seq 1 50); do [ -s "$dir/.graphhelm/slow.port" ] && break; sleep 0.2; done
    slow=$(cat "$dir/.graphhelm/slow.port" 2> /dev/null || true)
    [[ "$slow" =~ ^[0-9]+$ ]] || { echo 'fixture: the slow server did not start' >&2; exit 1; }
    cat > "$dir/project/.graphhelm/journeys/health-slow.journey.yaml" <<FLOW
schema: graphhelm.journey-flow/1
id: health-slow
title: Read a slow health check
name: Read a slow health check
purpose: "The fixture's draft whose run plays for several seconds: its server answers after a wait."
status: draft
approved: null
base: http://127.0.0.1:$slow
actors: [owner]
secrets: []
risks: []
screens:
  - id: checked
    title: The slow server answered ok
    url: /
    state: success
    expect: [{role: status, name: "200", because: the slow server answers every GET with HTTP 200 after its wait}, {role: json, name: ok=true, because: the slow server always answers ok}]
    scope: unknown
  - id: start
    title: The slow server answers
    url: /
    state: stable
    expect: [{role: status, name: "200", because: the slow server answers every GET with HTTP 200 after its wait}]
    scope: unknown
edges:
  - id: read
    from: start
    to: checked
    why: "Reading the slow server takes several seconds, so the run can be seen while it plays."
    acts:
      - {kind: http, role: GET, name: /}
paths:
  main: [read]
drift: []
FLOW
    git -C "$dir/project" add -f -- .graphhelm/journeys/health-slow.journey.yaml
  fi
  git -C "$dir/project" -c user.email=demo@example.com -c user.name=demo commit -qm 'fixture: observer seed flow'
  # #1901: the Studio opens a project's team run by itself, so a project with no run at all needs a
  # fixture that starts none: GRAPHHELM_FIXTURE_EMPTY=1, or the studio2-no-run flow, skips both the
  # observer preview (its runner records into a run of its own) and the seeded demo run.
  empty=0
  if [ "${GRAPHHELM_FIXTURE_EMPTY:-}" = 1 ] || [ "${GRAPHHELM_JOURNEY_FLOW:-}" = studio2-no-run ]; then empty=1; fi
  if [ "$empty" = 0 ]; then
  "$bin" --json journey preview runtime-health --force --project "$dir/project"     > "$dir/.graphhelm/observer-preview.out" 2>&1 || true
  for _ in $(seq 1 60); do
    "$bin" --json journey preview runtime-health --read --project "$dir/project"       > "$dir/.graphhelm/observer-preview.out" 2>&1 || true
    grep -q '"state":"running"' "$dir/.graphhelm/observer-preview.out" || break
    sleep 1
  done
  grep -q '"recorded":true' "$dir/.graphhelm/observer-preview.out"     || echo "fixture: the observer preview did not record (see $dir/.graphhelm/observer-preview.out)" >&2
  fi
  for pid in $(fixture_listeners record "$rport"); do
    if [ -r /proc/$$/winpid ]; then echo "serve-listener 0 $pid"; else echo "serve-listener $pid"; fi >> "$pids"
  done
  # The seeded run: the manual-override example, one blocked node, one ready node.
  if [ "$empty" = 0 ] && ! grep -q '"executionId":"demo"' "$dir/.graphhelm/seed.json" 2>/dev/null; then
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
    # #1808: studio2-lanes-reply answers a question at each viewport against this one Runtime, so
    # it gets a second open question from the same lane: the second viewport answers that one.
    if [ "${GRAPHHELM_JOURNEY_FLOW:-}" = studio2-lanes-reply ]; then
      id2=$(node -e 'console.log(require("crypto").randomUUID())')
      printf '{"signal":{"id":"planner-question-%s","source":{"type":"tool","id":"planner"},"type":"operator_note","severity":"low","description":"Which slot should the rebuild use?","evidence":["demo"],"emittedAt":"%s","to":"studio-operator"}}' "$id2" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$dir/.graphhelm/question2.json"
      curl -sf --max-time 10 -X POST "http://127.0.0.1:$rport/v1/executions/demo/signal" -H "Authorization: Bearer $token"         -H "Content-Type: application/json" -H "Idempotency-Key: $id2" -H "X-GraphHelm-Actor: planner" -H "X-GraphHelm-Actor-Type: agent"         --data-binary @"$dir/.graphhelm/question2.json" > "$dir/.graphhelm/question2.out" || { echo 'fixture: the second question was refused' >&2; exit 1; }
    fi
    # #1041: one Keel card from the node `deploy` (studio-keel-card). Its evidence is the promise,
    # each scope path and the proof command, in that order (docs/keel/RECORDS.md).
    cid=$(node -e 'console.log(require("crypto").randomUUID())')
    printf '{"signal":{"id":"deploy-card-%s","source":{"type":"node","id":"deploy"},"type":"keel.card","severity":"low","description":"Keel card: the deploy goes to staging first.","evidence":["The release reaches staging before production","deploy/staging.yaml","deploy/production.yaml","graphhelm journey preview studio-deploy"],"emittedAt":"%s"}}' "$cid" "$now" > "$dir/.graphhelm/card.json"
    curl -sf --max-time 10 -X POST "http://127.0.0.1:$rport/v1/executions/demo/signal" -H "Authorization: Bearer $token"       -H "Content-Type: application/json" -H "Idempotency-Key: $cid" -H "X-GraphHelm-Actor: planner" -H "X-GraphHelm-Actor-Type: agent"       --data-binary @"$dir/.graphhelm/card.json" > "$dir/.graphhelm/card.out"
    # #585: studio-handover shows "While you were away" after 20+ events since the owner was last
    # here; the flow sets that last-seen time in the browser, these are the events after it.
    for n in $(seq 1 22); do
      nid=$(node -e 'console.log(require("crypto").randomUUID())')
      printf '{"signal":{"id":"planner-progress-%s","source":{"type":"tool","id":"planner"},"type":"operator_note","severity":"low","description":"Progress note %s while you were away.","evidence":["demo"],"emittedAt":"%s"}}' "$nid" "$n" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$dir/.graphhelm/note.json"
      curl -sf --max-time 10 -X POST "http://127.0.0.1:$rport/v1/executions/demo/signal" -H "Authorization: Bearer $token" \
        -H "Content-Type: application/json" -H "Idempotency-Key: $nid" -H "X-GraphHelm-Actor: planner" -H "X-GraphHelm-Actor-Type: agent" \
        --data-binary @"$dir/.graphhelm/note.json" > /dev/null
    done
    # #1712: one shipped line in the same gap, so the details' "Cited evidence" link cites a record
    # read from the event tail itself. Without it the only citation was the open question, which
    # the Studio loads from a separate, slower source, and the link appeared or not by load.
    sid=$(node -e 'console.log(require("crypto").randomUUID())')
    printf '{"signal":{"id":"planner-shipped-%s","source":{"type":"tool","id":"planner"},"type":"agent_task_completed","severity":"low","description":"Fixture task finished while you were away.","evidence":["demo"],"emittedAt":"%s"}}' "$sid" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$dir/.graphhelm/shipped.json"
    curl -sf --max-time 10 -X POST "http://127.0.0.1:$rport/v1/executions/demo/signal" -H "Authorization: Bearer $token"       -H "Content-Type: application/json" -H "Idempotency-Key: $sid" -H "X-GraphHelm-Actor: planner" -H "X-GraphHelm-Actor-Type: agent"       --data-binary @"$dir/.graphhelm/shipped.json" > /dev/null || echo "fixture: the shipped seed was refused" >&2
  fi
  flow=${GRAPHHELM_JOURNEY_FLOW:-}
  # #1808: studio2-lanes-needs-action's quiet lanes are recorded first; `fresh` below waits for
  # them to turn silent (one minute: the budget this Runtime was started with) before the fixture reports up.
  if [ "$flow" = studio2-lanes-needs-action ]; then
    node "$repo/tools/studio-journey-fixture/seed_needs.mjs" silent "$dir" "http://127.0.0.1:$rport" "$rt/events.token"
  fi
  # #1808: the Graph flows read a request's issues and PRs; the old studio-work-graph seed serves them.
  # #1901: studio2-lanes-board reads the same request in its WHAT I ASKED column.
  if [ "$flow" = studio2-graph-requests ] || [ "$flow" = studio2-graph-review-screen ] || [ "$flow" = studio2-lanes-board ]; then
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
const journey = [process.env.GRAPHHELM_JOURNEY_FLOW || 'studio2-graph-requests'];
const records = [
  ['merged-claimed', 'task.claimed', { taskId: 'issue-721-merged', issue: 721, lane: 'planner', branch: 'issue-721-merged-fixture', journeys: journey, title: 'Merged fixture row' }],
  ['merged-pr', 'task.pr_opened', { taskId: 'issue-721-merged', pr: 722, lane: 'planner', headSha: '722abcde', journeys: journey, title: 'Merged fixture row' }],
  ['merged-review', 'task.review_verdict', { taskId: 'issue-721-merged', pr: 722, headSha: '722abcde', reviewer: 'planner', verdict: 'APPROVE', commentUrl: 'https://github.com/stabem/GraphHelm/pull/722#review' }],
  ['merged-done', 'task.merged', { taskId: 'issue-721-merged', pr: 722, mergeSha: '722fedcb', closes: [], merger: 'planner' }],
  // The open slice is recorded last: an issue's Now reads its newest slice that has a PR (#1839).
  ['open-claimed', 'task.claimed', { taskId: 'issue-721-open', issue: 721, lane: 'planner', branch: 'issue-721-open-fixture', journeys: journey, title: 'Observe issue graph and work kanban' }],
  ['open-planned', 'task.planned', { taskId: 'issue-721-open', lane: 'planner', classes: ['user_visible'], reviews: 1, proof: 'journey', critic: { mode: 'none', passScore: 7, maxRounds: 1 }, summary: 'Fixture plan' }],
  ['open-pr', 'task.pr_opened', { taskId: 'issue-721-open', pr: 721, lane: 'planner', headSha: '721abcde', journeys: journey, title: 'Observe issue graph and work kanban' }],
  // #1808: the Graph draws owner requests (#1792); one request linked to the fixture issue.
  ['request', 'task.request_recorded', { taskId: 'request-fixture', requestId: 'fixture', ask: 'Show the work graph for the fixture run', recordedBy: 'planner' }],
  ['request-link', 'task.request_linked', { taskId: 'request-fixture', requestId: 'fixture', issue: 721, linkedBy: 'planner' }],
];
// #1947: a request with no issue yet sits beside the one-issue request: its YOU ASKED card (chip,
// bar, legend) is taller than its row would be, and must not overlap the next request.
if (journey[0] === 'studio2-graph-requests') {
  records.push(['request-empty', 'task.request_recorded', { taskId: 'request-empty', requestId: 'empty', ask: 'Prove a regression test for the fixture run', recordedBy: 'planner' }]);
}
// #1808: the review's Now is the PR head's captured frame (#1839); one for the first screen.
if (journey[0] === 'studio2-graph-review-screen') {
  const png = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==';
  records.push(['now-frames', 'task.journey_frames', { taskId: 'issue-721-open', pr: 721, headSha: '721abcde', journey: journey[0],
    result: 'pass', ranAt: at, screens: ['board'], lane: 'planner' }, [{ mediaType: 'image/png', base64: png }]]);
}
const token = fs.readFileSync(tokenFile, 'utf8').trim();
(async () => {
  for (const [id, type, fields, attachments] of records) {
    const document = { ...common, ...fields };
    const signal = { id: `fixture-721-${id}`, type, source: { type: 'user', id: 'planner' }, severity: 'low',
      emittedAt: at, evidence: ['demo'], description: JSON.stringify(document) };
    const response = await fetch(new URL('/v1/executions/demo/signal', runtime), {
      method: 'POST', headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json',
        'Idempotency-Key': `fixture-721-${id}`, 'X-GraphHelm-Actor': 'planner', 'X-GraphHelm-Actor-Type': 'agent' },
      body: JSON.stringify(attachments ? { signal, attachments } : { signal }), signal: AbortSignal.timeout(10000),
    });
    const result = await response.json();
    fs.writeFileSync(`${dir}/.graphhelm/task-${id}.out`, JSON.stringify(result));
    if (!response.ok || result.ok !== true) throw new Error(`task seed refused: ${id}`);
  }
})().catch(() => { console.error('fixture: task record refused; stop without widening permissions'); process.exit(1); });
NODE
  fi
  # #1808: the Proof compare and the review's Before read a finished run of the
  # flow kept in the project; seed_run.mjs writes one (current + one earlier), bound to the flow's
  # digest and commit as the preview route reports them.
  if [ "$flow" = studio2-graph-review-screen ] || [ "$flow" = studio2-proof-compare ]; then
    "$bin" --json journey preview "$flow" --project "$dir/project" --read > "$dir/.graphhelm/preview-key.json" 2>/dev/null || true
    key=$(node -e 'const d=JSON.parse(require("fs").readFileSync(process.argv[1],"utf8")).data||{}; if(d.digest&&d.commit) console.log(d.digest+" "+d.commit)' "$dir/.graphhelm/preview-key.json")
    [ -n "$key" ] || { echo "fixture: no preview key for $flow" >&2; exit 1; }
    node "$repo/tools/studio-journey-fixture/seed_run.mjs" "$dir/project" "$flow" $key > /dev/null
  fi
  # #1956: studio2-proof-stale needs a journey edited after its run: its run is seeded under the
  # flow's key, then the flow's purpose is edited and committed, so its digest moves and the
  # preview read answers `none` while that run stays the last one.
  if [ "$flow" = studio2-proof-stale ]; then
    "$bin" --json journey preview "$flow" --project "$dir/project" --read > "$dir/.graphhelm/preview-key.json" 2>/dev/null || true
    key=$(node -e 'const d=JSON.parse(require("fs").readFileSync(process.argv[1],"utf8")).data||{}; if(d.digest&&d.commit) console.log(d.digest+" "+d.commit)' "$dir/.graphhelm/preview-key.json")
    [ -n "$key" ] || { echo "fixture: no preview key for $flow" >&2; exit 1; }
    node "$repo/tools/studio-journey-fixture/seed_run.mjs" "$dir/project" "$flow" $key > /dev/null
    f="$dir/project/.graphhelm/journeys/$flow.journey.yaml"
    sed 's#^purpose: "A journey whose flow#purpose: "Edited after its run. A journey whose flow#' "$f" > "$f.tmp" && mv "$f.tmp" "$f"
    git -C "$dir/project" add -f -- ".graphhelm/journeys/$flow.journey.yaml"
    git -C "$dir/project" -c user.email=demo@example.com -c user.name=demo commit -qm 'fixture: edit the flow after its run'
    "$bin" --json journey preview "$flow" --project "$dir/project" --read > "$dir/.graphhelm/preview-key.json" 2>/dev/null || true
    grep -q '"state":"none"' "$dir/.graphhelm/preview-key.json" || { echo "fixture: $flow still reads its run after the edit" >&2; exit 1; }
  fi
  # #137: the async Runtime driver records delegation; no model executor is wired. No studio2 flow
  # reads it yet; kept for a lanes flow that names a delegated worker.
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
  # #1556 studio-lanes-board: one lane at a working step, holding the build slot. The task records
  # are signed by the existing planner actor, like studio-work-graph's. The slot holder is a real
  # `workspace slot` running a real `cargo build` of a one-file crate whose build script waits
  # while `slot.hold` exists; `down` removes it and ends the tree, so nothing is faked.
  if [ "$flow" = studio2-lanes-board ] || [ "$flow" = studio2-lanes-control ]; then
    node - "$dir" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "http://127.0.0.1:$rport" "$rt/events.token" <<'NODE'
const fs = require('fs');
const [dir, at, runtimeUrl, tokenFile] = process.argv.slice(2);
const runtime = new URL(runtimeUrl);
if (runtime.protocol !== 'http:' || runtime.hostname !== '127.0.0.1' || !runtime.port || runtime.port === '8793' || runtime.pathname !== '/') {
  throw new Error('fixture: unsafe Runtime URL');
}
const common = { schema: 'graphhelm-task-event-v1', revision: 1, at };
const records = [
  ['lanes-claimed', 'task.claimed', { taskId: 'issue-1556-lanes', issue: 1556, lane: 'planner', branch: 'issue-1556-lanes-fixture', journeys: ['studio2-lanes-board'], title: 'Show what each lane is doing' }],
  ['lanes-planned', 'task.planned', { taskId: 'issue-1556-lanes', lane: 'planner', classes: ['user_visible'], reviews: 1, proof: 'journey', critic: { mode: 'none', passScore: 7, maxRounds: 1 }, summary: 'Fixture plan' }],
];
// #1808: Reassign hands the slice to another lane, so studio2-lanes-control gets more lanes.
if (process.env.GRAPHHELM_JOURNEY_FLOW === 'studio2-lanes-control') {
  records.push(['helper-claimed', 'task.claimed', { taskId: 'issue-1557-helper', issue: 1557, lane: 'helper', branch: 'issue-1557-helper-fixture', journeys: [], title: 'Take over a slice' }]);
  // Each viewport stops the lane it shows; a third lane keeps one to reassign to at the second.
  records.push(['spare-claimed', 'task.claimed', { taskId: 'issue-1558-spare', issue: 1558, lane: 'spare', branch: 'issue-1558-spare-fixture', journeys: [], title: 'Spare lane' }]);
}
const token = fs.readFileSync(tokenFile, 'utf8').trim();
(async () => {
  for (const [id, type, fields, attachments] of records) {
    const actor = fields.lane ?? 'planner';
    const signal = { id: `fixture-1556-${id}`, type, source: { type: 'user', id: actor }, severity: 'low',
      emittedAt: at, evidence: ['demo'], description: JSON.stringify({ ...common, ...fields }) };
    const response = await fetch(new URL('/v1/executions/demo/signal', runtime), {
      method: 'POST', headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json',
        'Idempotency-Key': `fixture-1556-${id}`, 'X-GraphHelm-Actor': actor, 'X-GraphHelm-Actor-Type': 'agent' },
      body: JSON.stringify(attachments ? { signal, attachments } : { signal }), signal: AbortSignal.timeout(10000),
    });
    const result = await response.json();
    fs.writeFileSync(`${dir}/.graphhelm/task-${id}.out`, JSON.stringify(result));
    if (!response.ok || result.ok !== true) throw new Error(`task seed refused: ${id}`);
  }
})().catch(() => { console.error('fixture: task record refused; stop without widening permissions'); process.exit(1); });
NODE
    holder="$dir/slot-holder"
    mkdir -p "$holder/src"
    printf '[package]\nname = "lanes-fixture-holder"\nversion = "0.0.0"\nedition = "2021"\nbuild = "build.rs"\n\n[workspace]\n' > "$holder/Cargo.toml"
    printf '' > "$holder/src/lib.rs"
    # Waits while the hold file exists, at most 30 minutes, so a crashed fixture frees the slot.
    printf '%s\n' 'fn main() {' \
      '    let hold = std::env::var_os("LANES_FIXTURE_HOLD").map(std::path::PathBuf::from);' \
      '    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1800);' \
      '    while hold.as_ref().is_some_and(|path| path.exists()) && std::time::Instant::now() < deadline {' \
      '        std::thread::sleep(std::time::Duration::from_millis(500));' \
      '    }' \
      '}' > "$holder/build.rs"
    : > "$dir/.graphhelm/slot.hold"
    (cd "$holder" && LANES_FIXTURE_HOLD="$dir/.graphhelm/slot.hold" nohup "$bin" workspace slot --root "$dir/slots" \
      --lane planner --label lanes-fixture --jobs 1 -- cargo build -p lanes-fixture-holder \
      > "$dir/.graphhelm/slot-holder.out" 2>&1 &
      echo "slot-holder $! $(cat /proc/$!/winpid 2> /dev/null)" >> "$pids")
    for _ in $(seq 1 120); do
      "$bin" --json workspace slot status --root "$dir/slots" 2> /dev/null | grep -q '"lane":"planner"' && break
      sleep 1
    done
    "$bin" --json workspace slot status --root "$dir/slots" 2> /dev/null | grep -q '"lane":"planner"' \
      || echo "fixture: the slot holder did not take the slot (see $dir/.graphhelm/slot-holder.out)" >&2
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
  # #1808 studio2-two-projects: a second project, `proj-two`, with its own Runtime and one run,
  # registered with this Studio the way `graphhelm studio open` does it. The ticket link it prints
  # is kept in a private file for the replay to open once; it is never echoed.
  if [ "$flow" = studio2-two-projects ]; then
    p2=$((rport + 2))
    [ "$p2" != 8793 ] && [ "$p2" != "$sport" ] || { echo 'fixture: unsafe second Runtime port' >&2; exit 2; }
    sed -i "s/^ports .*/& $p2/" "$pids"
    second="$dir/proj-two"; rt2="$second/.graphhelm"
    mkdir -p "$second"
    if [ ! -d "$second/.git" ]; then
      git -C "$second" init -q
      printf "# proj-two\n" > "$second/README.md"
      git -C "$second" add -A && git -C "$second" -c user.email=demo@example.com -c user.name=demo commit -qm init
    fi
    "$bin" --json init --project "$second" --bind "127.0.0.1:$p2" --harness codex > "$dir/.graphhelm/init-second.json"
    nohup "$bin" serve --events "$rt2/events" --bind "127.0.0.1:$p2" --project "$second" \
      --keyring "$rt2/keyring" --key-id studio > "$dir/.graphhelm/serve-second.out" 2> "$dir/.graphhelm/serve-second.err" &
    echo "serve-second $! $(cat /proc/$!/winpid 2> /dev/null)" >> "$pids"
    for _ in $(seq 1 30); do curl -sf --max-time 10 "http://127.0.0.1:$p2/health" > /dev/null && break; sleep 1; done
    curl -sf --max-time 10 "http://127.0.0.1:$p2/health" > /dev/null || { echo "second runtime did not answer on $p2"; exit 1; }
    if ! grep -q '"executionId":"second-run"' "$dir/.graphhelm/seed-second.json" 2>/dev/null; then
      GRAPHHELM_EVENTS_KEY="$(cat "$rt2/serve.key")" "$bin" --json execution start \
        --file "$repo/examples/graphs/manual-override-deploy.yaml" --events "$rt2/events" \
        --fixtures "$dir/.graphhelm/fixtures.json" --mode supervised --execution second-run \
        > "$dir/.graphhelm/seed-second.json"
    fi
    (umask 077; "$bin" --json studio open --project "$second" --port "$sport" --no-browser > "$dir/.graphhelm/second-open.json")
    grep -q '"ok":true' "$dir/.graphhelm/second-open.json" \
      || { echo "fixture: the second project was not registered (see $dir/.graphhelm/second-open.json)" >&2; exit 1; }
    # The owner adds proj-two in the sidebar with its folder and the ticket `studio open` minted,
    # as the real Studio does. Every viewport of a replay opens a fresh browser that spends a
    # ticket, so a FIXTURE-ONLY keeper puts the same ticket back once it is spent. Both values go
    # to the private secrets file the replay reads; neither is printed.
    ticket=$(cat "$rt2/events.studio-ticket")
    [ -n "$ticket" ] || { echo 'fixture: studio open left no ticket' >&2; exit 1; }
    folder2=$(cd "$second" && pwd -W 2> /dev/null || pwd)
    (umask 077; printf 'GRAPHHELM_SECRET_proj_two_folder=%s\nGRAPHHELM_SECRET_proj_two_ticket=%s\n' "$folder2" "$ticket" > "$dir/.graphhelm/secrets-second.env")
    (umask 077; printf '%s' "$ticket" > "$dir/.graphhelm/second.ticket")
    nohup node -e '
      const fs = require("fs"); const [file, saved] = process.argv.slice(1);
      const keep = fs.readFileSync(saved, "utf8");
      setInterval(() => { let now = ""; try { now = fs.readFileSync(file, "utf8").trim(); } catch {} if (now === "") fs.writeFileSync(file, keep, { mode: 0o600 }); }, 250);
      setTimeout(() => process.exit(0), 3600000);
    ' "$rt2/events.studio-ticket" "$dir/.graphhelm/second.ticket" > /dev/null 2>&1 &
    echo "ticket-keeper $! $(cat /proc/$!/winpid 2> /dev/null)" >> "$pids"
  fi
  if [ "$flow" = studio2-lanes-needs-action ]; then
    node "$repo/tools/studio-journey-fixture/seed_needs.mjs" fresh "$dir" "http://127.0.0.1:$rport" "$rt/events.token"
  fi
  # R6: the launcher reads this private data file after up; never echo the minted token.
  (umask 077; printf 'GRAPHHELM_SECRET_STUDIO_TOKEN=%s\n' "$(head -1 "$rt/events.token")" > "$dir/.graphhelm/secrets.env"
    if [ -f "$dir/.graphhelm/secrets-second.env" ]; then cat "$dir/.graphhelm/secrets-second.env" >> "$dir/.graphhelm/secrets.env"; fi)
  chmod 600 "$dir/.graphhelm/secrets.env"
  echo "fixture up: runtime http://127.0.0.1:$rport, studio http://127.0.0.1:$sport/?session=studio-fixture"
  ;;
down)
  # Each record is `<name> <shell pid> [<OS pid>]`. On Windows (Git Bash) the shell pid is an MSYS
  # pid the OS does not know, so the OS pid taken from /proc/<pid>/winpid at start is used.
  # #1556: the slot holder's build script ends when its hold file goes, freeing the slot.
  rm -f "$dir/.graphhelm/slot.hold"
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
