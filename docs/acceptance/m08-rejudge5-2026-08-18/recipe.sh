#!/bin/sh
# M07 Task 6: the closing rule. The blind judge re-judges the SAME story against the fixed
# surface. Deliberately harder than the M06 run: a real tool failure lands before the judge
# node (so F3's cause is inspectable) and an alarm is armed and rung (so F4's receipt is
# inspectable). Same user story, same charter, same MCP surface wording as M06 — changing
# the question would make the comparison worthless.
set -eu

WT=F:/projects/GraphHelm/.claude/worktrees/milestone-4-parallel-432658
BIN=$WT/target/debug/graphhelm.exe
RUN=C:/Users/example/AppData/Local/Temp/m08-judge5
ART=$WT/docs/acceptance/m08-rejudge5-2026-08-18
PORT=42219
rm -rf "$RUN"; mkdir -p "$RUN"

GRAPHHELM_GATEWAY_KEY=$(python -c "import secrets;print(secrets.token_hex(32))")
GRAPHHELM_EVENTS_KEY=$GRAPHHELM_GATEWAY_KEY
export GRAPHHELM_GATEWAY_KEY GRAPHHELM_EVENTS_KEY

EVENTS=$RUN/events BROKER=$RUN/broker KEYRING=$RUN/keyring STAGING=$RUN/staging
mkdir -p "$KEYRING"

PROJECT=$RUN/project
mkdir -p "$PROJECT/src"; printf '// m08\n' > "$PROJECT/src/lib.rs"
git -C "$PROJECT" init -q
git -C "$PROJECT" -c user.name=m08 -c user.email=m08@test.invalid add -A
git -C "$PROJECT" -c user.name=m08 -c user.email=m08@test.invalid commit -qm m08

printf 'dummy-never-leased' | "$BIN" gateway credential set \
  --broker "$BROKER" --keyring "$KEYRING" --key-id m08-key \
  --ref cred_unused --provider anthropic --usable-by judge_route > "$RUN/cred-set.json"

cat > "$RUN/mcp.json" <<EOF
{
  "mcpServers": {
    "graphhelm": {
      "command": "$BIN",
      "args": ["mcp", "--url", "http://127.0.0.1:$PORT",
               "--token-file", "$RUN/events.token", "--actor", "agent-judge"]
    }
  }
}
EOF

cat > "$RUN/manifest.json" <<EOF
{
  "manifestVersion": 1,
  "routes": [{
    "id": "judge_route",
    "provider": "anthropic",
    "transport": "native_runtime",
    "runtime": "claude_code",
    "authentication": "account_subscription",
    "billingMode": "subscription_quota",
    "command": { "program": "C:/Users/example/.local/bin/claude.exe",
                 "args": ["-p", "--output-format", "json",
                          "--strict-mcp-config", "--mcp-config", "$RUN/mcp.json"] },
    "profiles": ["critical_reasoning"],
    "enabled": true,
    "timeoutSeconds": 420
  }]
}
EOF

EXEC=exec-m08-judge5
python - "$RUN" "$EXEC" <<'PYEOF'
import json, sys
run, exec_id = sys.argv[1:3]
# The SAME judge block as M06 — identical story, identical surface wording.
judge_block = {"judgeId": "judge-usefulness",
               "userStory": ("As an operator I open the monitor and know in one glance "
                             "whether I can go back to sleep."),
               "mcpSurface": ("the graphhelm MCP server registered in your MCP config "
                              "(tools: status, events, wake_arm, wake_status, ...); "
                              f"probe execution {exec_id}")}
graph = f"""apiVersion: p50.dev/graph/v1
kind: ExecutionGraph
metadata:
  id: exec_m08_judge_v5
  name: M08 judge story
  executionId: {exec_id}
  version: 1
spec:
  entrypoints:
    - prepare
  nodes:
    prepare:
      type: tool
      name: Prepare
      objective: Prove the workspace.
      optionality: required
      input:
        schema: schema://TaskRequest@1
      output:
        schema: schema://TestReport@1
      tool:
        call:
          tool: shell
          program: git
          arguments:
            - status
      completion:
        requires:
          - expression: output.executed > 0
    flaky_check:
      type: tool
      name: Flaky check
      objective: Fail for real, so the failure has a cause to inspect.
      optionality: optional
      input:
        schema: schema://TaskRequest@1
      output:
        schema: schema://TestReport@1
      tool:
        call:
          tool: shell
          program: git
          arguments:
            - cat-file
            - "-e"
            - deadbeefdeadbeefdeadbeefdeadbeefdeadbeef
      completion:
        requires:
          - expression: output.executed > 0
    judge:
      type: evaluator
      name: Blind judge
      objective: Judge usefulness blind.
      optionality: required
      judge: {json.dumps(judge_block)}
      completion:
        requires:
          - expression: output.executed > 0
  edges:
    - id: prepare_to_flaky
      from: prepare
      to: flaky_check
      type: data
    - id: prepare_to_judge
      from: prepare
      to: judge
      type: data
  budgets:
    maxParallelModelCalls: 1
  completion:
    terminalNodes:
      - judge
      - flaky_check
"""
open(f"{run}/graph.yaml", "w", newline="\n").write(graph)
print("graph written")
PYEOF

cd "$RUN"
"$BIN" serve --events "$EVENTS" --bind 127.0.0.1:$PORT \
  --manifest "$RUN/manifest.json" --broker "$BROKER" --keyring "$KEYRING" \
  --key-id m08-key --route judge_route --staging "$STAGING" \
  --read-audit "$RUN/read-audit.jsonl" \
  > "$RUN/serve-out.txt" 2> "$RUN/serve-err.txt" &
SERVE_PID=$!
trap 'kill $SERVE_PID 2>/dev/null || true' EXIT INT TERM
sleep 4
TOKEN=$(cat "$RUN/events.token")

# The whole story in one drive: the tool failure lands, the alarm is armed and rung before
# the judge node runs, so both F3 and F4 have something real for the judge to inspect.
python - "$PORT" "$TOKEN" "$EXEC" "$RUN" <<'PYEOF'
import json, sys, threading, time, urllib.request, urllib.error
port, token, exec_id, run = sys.argv[1:5]
base = f"http://127.0.0.1:{port}/v1/executions/{exec_id}"

def post(path, body, key):
    request = urllib.request.Request(
        base + path, data=json.dumps(body).encode(), method="POST",
        headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json",
                 "Idempotency-Key": key, "X-GraphHelm-Actor": "owner-m08",
                 "X-GraphHelm-Actor-Type": "owner"})
    return json.load(urllib.request.urlopen(request, timeout=600))

def alarm():
    # Arm a lease, then ring it with a signal, so wake_status carries a real receipt by the
    # time the judge looks: "rang at #N", not just "not live".
    try:
        armed = post("/wake-lease", {"sessionId": "operator-m07",
                                     "rendezvousId": "m07-operator"}, "m08-arm5")
        open(f"{run}/alarm-armed.json", "w", newline="\n").write(json.dumps(armed, indent=2))
        signal = {"signal": {"id": "sig-m08-operator", "source": {"type": "node", "id": "prepare"},
                             "type": "no_progress", "severity": "low",
                             "description": "operator alarm for the re-judge run",
                             "evidence": ["exec-1"],
                             "emittedAt": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())},
                  "evidenceOut": f"{run}/alarm-evidence.json"}
        post("/signal", signal, "m08-ring5")
    except urllib.error.HTTPError as error:
        open(f"{run}/alarm-error.txt", "w", newline="\n").write(error.read().decode())

alarm()
body = {"file": f"{run}/graph.yaml", "mode": "autopilot", "project": f"{run}/project"}
try:
    reply = post("/start", body, "m08-judge5-start")
except urllib.error.HTTPError as error:
    text = error.read().decode()
    open(f"{run}/start-error.json", "w", newline="\n").write(text)
    raise SystemExit(f"START FAILED {error.code}: {text[:500]}")
open(f"{run}/start-reply.json", "w", newline="\n").write(json.dumps(reply, indent=2))
data = reply["data"]
print("status:", data["status"])
print("attention:", json.dumps(data.get("attention")))
print("silenceUnevaluated:", json.dumps(data.get("silenceUnevaluated")))
print("nodeStateCounts:", json.dumps(data["nodeStateCounts"]))
PYEOF

kill $SERVE_PID 2>/dev/null || true
sleep 1
mkdir -p "$ART"
"$BIN" graph replay --events "$EVENTS" > "$RUN/replay-1.txt"
"$BIN" graph replay --events "$EVENTS" > "$RUN/replay-2.txt"
cmp "$RUN/replay-1.txt" "$RUN/replay-2.txt" && echo "REPLAY BYTE-IDENTICAL"
echo "RUN DONE"
