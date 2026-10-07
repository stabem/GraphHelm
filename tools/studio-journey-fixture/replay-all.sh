#!/usr/bin/env bash
# Replays every approved `studio-*` flow against the fixture started by fixture.sh and records
# the captures into the fixture's own run (#381).
#
#   tools/studio-journey-fixture/replay-all.sh <fixture dir> <output dir> [flow id ...]
#
# Reads the sealing key and the Studio token from <fixture dir>/.graphhelm into the environment
# of the replay process only; nothing is printed. One JSON reply per flow lands in <output dir>.
set -uo pipefail
repo=$(cd "$(dirname "$0")/../.." && pwd)
bin=${GRAPHHELM_BIN:-graphhelm}
dir=${1:-}; out=${2:-}; shift 2 || { sed -n 2,9p "$0"; exit 2; }
[ -n "$dir" ] && [ -n "$out" ] || { sed -n 2,9p "$0"; exit 2; }
dir=$(cd "$dir" && pwd); mkdir -p "$out"
export GRAPHHELM_EVENTS_KEY="$(cat "$dir/.graphhelm/serve.key")"
export GRAPHHELM_SECRET_STUDIO_TOKEN="$(head -1 "$dir/.graphhelm/events.token")"
ids=("$@")
if [ ${#ids[@]} -eq 0 ]; then
  for f in "$repo"/.graphhelm/journeys/studio-*.journey.yaml; do ids+=("$(basename "$f" .journey.yaml)"); done
fi
green=0; red=0
for id in "${ids[@]}"; do
  "$bin" --json journey replay "$id" --project "$repo" --events "$dir/.graphhelm/events" --execution demo \
    --keyring "$dir/.graphhelm/keyring" --key-id studio > "$out/$id.json" 2> "$out/$id.err"
  rc=$?
  summary=$(node -e '
    const r = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
    const paths = (r.data && r.data.paths) || [];
    const d = (r.diagnostics || []).map(x => x.code + "@" + x.path).join(" ");
    console.log(paths.map(p => p.name + ":" + p.outcome + "(" + (p.capturedSignalIds || []).length + "cap)").join(" ") + (d ? " | " + d : ""));
  ' "$out/$id.json" 2>/dev/null || echo "unreadable reply")
  if [ "$rc" -eq 0 ]; then green=$((green+1)); else red=$((red+1)); fi
  printf '%-38s rc=%s %s\n' "$id" "$rc" "$summary"
done
echo "green=$green red=$red"
