#!/usr/bin/env bash
# Replays every approved `studio2-*` flow against the Studio journey fixture (#381, #2903).
#
#   tools/studio-journey-fixture/replay-all.sh <fixture dir|-> <output dir> [flow id ...]
#
# A flow that declares ONE viewport (or none) replays on <fixture dir>, the fixture started by
# fixture.sh, and records its captures into that fixture's own run (#381), as before.
#
# A flow that declares SEVERAL viewports changes state (Run, Start, Approve): replayed against one
# fixture, the first viewport leaves the state the second one then finds. So each such viewport
# gets a fresh fixture of its own, started and stopped by this script (#2903): mobile first, web
# last, mirroring `journey replay`. Each viewport replays with `graphhelm journey replay <id>
# --viewport <name>`, and the flow is green only if every viewport is. Such a replay RECORDS
# NOTHING: no captures reach a run, no cache is published and nothing heals (the CLI refuses to
# leave proof from one viewport of several, #1878); its reply says `recording:
# withheld_viewport_limited`. Proof recording needs the full `journey replay` of the flow.
# A red viewport ends its flow: a red replay writes drift into the flow file (the approval is voided
# in the project; `git checkout` the file to restore it), so the later viewports would only be
# refused and are reported as not run.
#
# The flows' base is http://127.0.0.1:5184, so the Studio port is fixed. The fresh fixtures use
# the Runtime port in REPLAY_RUNTIME_PORT (default 8811); 5183, 8793, 8795 and 8797 are refused,
# as they belong to other sessions' fixtures. Both ports must be free when a fresh fixture starts,
# so when a multi-viewport flow is among the flows, <fixture dir> is stopped (fixture.sh down)
# after the single-viewport flows have replayed on it. `-` for <fixture dir> means none: then
# every flow must declare several viewports. Fresh fixtures live under <output dir>/fixtures/ and
# are stopped after each viewport; their logs stay there.
#
# Needs GRAPHHELM_BIN (or `graphhelm` on PATH) built with `journey replay --viewport`. Reads the
# sealing key and the Studio token from the fixture's .graphhelm into the environment of the
# replay process only; nothing is printed. Output: one JSON reply per flow in <output dir>
# (`<id>.json`; a multi-viewport flow also has `<id>.<viewport>.json`, and `<id>.json` is the
# first failing viewport's reply, else the last viewport's), a summary line per flow and
# `green=<n> red=<n>`.
set -uo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
bin=${GRAPHHELM_BIN:-graphhelm}
dir=${1:-}; out=${2:-}; shift 2 || { sed -n 2,33p "$0"; exit 2; }
[ -n "$dir" ] && [ -n "$out" ] || { sed -n 2,33p "$0"; exit 2; }
rport=${REPLAY_RUNTIME_PORT:-8811}; sport=5184
for p in "$rport" "$sport"; do
  case "$p" in 5183|8793|8795|8797) echo "replay-all: port $p belongs to another fixture; choose another REPLAY_RUNTIME_PORT" >&2; exit 2;; esac
done
[[ "$rport" =~ ^[0-9]+$ ]] && (( rport > 0 && rport < 65536 )) && [ "$rport" != "$sport" ] || { echo "replay-all: unsafe REPLAY_RUNTIME_PORT" >&2; exit 2; }
mkdir -p "$out"; out=$(cd "$out" && pwd)
if [ "$dir" != - ]; then dir=$(cd "$dir" && pwd); fi
flowfile() { echo "$repo/.graphhelm/journeys/$1.journey.yaml"; }
# The declared viewports of a flow that names several, in the order `journey replay` plays them:
# all but the first, then the first (web, which records when the whole flow is replayed, is last).
# Nothing for a flow that names one or none.
views_of() {
  local list first
  list=$(sed -n 's/^viewports: *\[\(.*\)\].*/\1/p' "$(flowfile "$1")" 2>/dev/null | tr -d ' \r' | tr ',' ' ')
  set -- $list
  [ $# -ge 2 ] || return 0
  first=$1; shift; echo "$* $first"
}
ids=("$@")
# Default order respects the fixture's state: flows that only read, then flows that record,
# then flows that change the run's state. A flow
# listed here that has no file is skipped; a file not listed here runs after them.
ORDER="studio2-open studio2-graph-requests studio2-graph-review-screen studio2-lanes-board studio2-proof-compare studio2-two-projects studio2-proof-run studio2-proof-run-progress studio2-proof-confirm studio2-lanes-reply studio2-lanes-needs-action studio2-lanes-control studio2-proof-approve studio2-no-run"
if [ ${#ids[@]} -eq 0 ]; then
  approved() { grep -q '^status: approved$' "$repo/.graphhelm/journeys/$1.journey.yaml" 2>/dev/null; }
  for id in $ORDER; do approved "$id" && ids+=("$id"); done
  for f in "$repo"/.graphhelm/journeys/studio2-*.journey.yaml; do id=$(basename "$f" .journey.yaml); case " $ORDER " in *" $id "*) ;; *) approved "$id" && ids+=("$id");; esac; done
  for f in "$repo"/.graphhelm/journeys/studio2-*.journey.yaml; do id=$(basename "$f" .journey.yaml); approved "$id" || echo "skipped (draft): $id"; done
fi
single=(); multi=()
for id in "${ids[@]}"; do if [ -n "$(views_of "$id")" ]; then multi+=("$id"); else single+=("$id"); fi; done
if [ "$dir" = - ] && [ ${#single[@]} -gt 0 ]; then echo "replay-all: no fixture dir given, but these flows need one: ${single[*]}" >&2; exit 2; fi

summarize() {
  node -e '
    const r = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
    const paths = (r.data && r.data.paths) || [];
    const d = (r.diagnostics || []).map(x => x.code + "@" + x.path).join(" ");
    console.log(paths.map(p => p.name + ":" + p.outcome + "(" + (p.capturedSignalIds || []).length + "cap)").join(" ") + (d ? " | " + d : ""));
  ' "$1" 2>/dev/null || echo "unreadable reply"
}
green=0; red=0
tally() { if [ "$1" -eq 0 ]; then green=$((green+1)); else red=$((red+1)); fi; }

# --- flows that declare one viewport: the given fixture, recording as before -------------------
for id in "${single[@]}"; do
  # The fixture's Runtime keeps its key and token under <fixture dir>/project/.graphhelm; the
  # private secrets file holds the token and, for studio2-two-projects, proj-two's folder and ticket.
  (
    export GRAPHHELM_EVENTS_KEY="$(cat "$dir/project/.graphhelm/serve.key")"
    set -a; . "$dir/.graphhelm/secrets.env"; set +a
    "$bin" --json journey replay "$id" --project "$repo" --events "$dir/project/.graphhelm/events" --execution demo \
      --keyring "$dir/project/.graphhelm/keyring" --key-id studio > "$out/$id.json" 2> "$out/$id.err"
  )
  rc=$?
  tally "$rc"
  printf '%-38s rc=%s %s\n' "$id" "$rc" "$(summarize "$out/$id.json")"
done

# --- flows that declare several viewports: a fresh fixture per viewport ------------------------
live=""
stop_live() { if [ -n "$live" ]; then "$here/fixture.sh" down "$live" > /dev/null 2>&1; live=""; fi; }
trap stop_live EXIT
if [ ${#multi[@]} -gt 0 ] && [ "$dir" != - ]; then "$here/fixture.sh" down "$dir" > /dev/null 2>&1; fi
for id in "${multi[@]}"; do
  flow_rc=0; line=""; first_bad=""; last=""
  for view in $(views_of "$id"); do
    fx="$out/fixtures/$id-$view"
    live="$fx"
    if ! GRAPHHELM_JOURNEY_FLOW="$id" "$here/fixture.sh" up "$fx" "$rport" "$sport" > "$out/$id.$view.fixture.log" 2>&1; then
      stop_live
      line="$line $view[fixture did not start: see $id.$view.fixture.log]"
      [ "$flow_rc" -ne 0 ] || flow_rc=70
      continue
    fi
    (
      set -a; . "$fx/.graphhelm/secrets.env"; set +a
      "$bin" --json journey replay "$id" --project "$repo" --viewport "$view" > "$out/$id.$view.json" 2> "$out/$id.$view.err"
    )
    rc=$?
    stop_live
    line="$line $view[$(summarize "$out/$id.$view.json")]"
    last="$view"
    if [ "$rc" -ne 0 ]; then
      [ "$flow_rc" -ne 0 ] || flow_rc=$rc; [ -n "$first_bad" ] || first_bad="$view"
      # A red replay voids the flow's approval in the project (drift is written to the flow file),
      # so a later viewport would only be refused (flow.not_approved): it is not run.
      rest=""; after=""
      for v in $(views_of "$id"); do [ -z "$after" ] || rest="$rest $v"; [ "$v" != "$view" ] || after=1; done
      [ -z "${rest// /}" ] || line="$line [not run, $view was red:${rest% }]"
      break
    fi
  done
  pick=${first_bad:-$last}
  if [ -n "$pick" ]; then cp "$out/$id.$pick.json" "$out/$id.json"; fi
  tally "$flow_rc"
  printf '%-38s rc=%s%s\n' "$id" "$flow_rc" "$line"
done
echo "green=$green red=$red"
