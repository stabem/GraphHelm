#!/bin/sh
# studio-up.sh: the POSIX twin of studio-up.ps1 (#1152), so `graphhelm studio start` works on
# Linux and macOS without PowerShell. Same flags, same order of work:
#   1. start `graphhelm serve` on --bind IF nothing answers there; an answering address is reused
#      only when THIS project's token opens it (never two writers on one events directory);
#   2. `npm ci` when node_modules is missing or older than package-lock.json;
#   3. start the dev server with the environment that makes the page open connected;
#   4. open the browser, unless --no-browser.
# The token is never printed; the sealing key arrives in the environment, never as a flag.

set -eu

events="" bind="127.0.0.1:8791" studio_port=5183 project="" project_path="" graphhelm="graphhelm"
keyring="" key_id="" manifest="" broker="" route="" no_browser=0 runtime_only=0

while [ $# -gt 0 ]; do
  case "$1" in
    --events) events=$2; shift 2 ;;
    --bind) bind=$2; shift 2 ;;
    --studio-port) studio_port=$2; shift 2 ;;
    --project) project=$2; shift 2 ;;
    --project-path) project_path=$2; shift 2 ;;
    --graph-helm|--graphhelm) graphhelm=$2; shift 2 ;;
    --keyring) keyring=$2; shift 2 ;;
    --key-id) key_id=$2; shift 2 ;;
    --manifest) manifest=$2; shift 2 ;;
    --broker) broker=$2; shift 2 ;;
    --route) route=$2; shift 2 ;;
    --no-browser) no_browser=1; shift ;;
    --runtime-only) runtime_only=1; shift ;;
    *) echo "[up] unknown argument: $1" >&2; exit 2 ;;
  esac
done
[ -n "$events" ] || { echo "[up] --events is required" >&2; exit 2; }

studio=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
runtime_url="http://$bind"

answers() { curl -fsS --max-time 2 -o /dev/null "$1" 2>/dev/null; }

# #1767: --runtime-only starts (or reuses) this project's Runtime and stops there, for a project that
# joins a Studio already serving another one; that Studio keeps its port, so it is not probed here.
if [ "$runtime_only" -eq 0 ] && answers "http://127.0.0.1:$studio_port/"; then
  echo "[up] Studio address http://127.0.0.1:$studio_port is already in use. Stop that Studio explicitly before starting this project; no other port was selected." >&2
  exit 1
fi

[ -d "$events" ] || { echo "[up] events directory not found, creating: $events"; mkdir -p "$events"; }
trimmed=${events%/}
token_file="$(dirname -- "$trimmed")/$(basename -- "$trimmed").token"

# #1298: one events directory has one Runtime (its lifecycle lock), and it must serve THIS folder.
# An agent's worktree once started a Runtime on these events with --project <worktree>; reusing it
# ran every journey against the worktree's code. Such a Runtime is refused and named, never reused.
lifecycle="$(dirname -- "$trimmed")/$(basename -- "$trimmed").runtime-lifecycle.json"
canonical() { (CDPATH= cd -- "$1" 2>/dev/null && pwd -P) || printf '%s\n' "${1%/}"; }
if [ -n "$project_path" ] && [ -f "$lifecycle" ] && answers "$runtime_url/health"; then
  owner=$(sed -n 's/.*"pid"[[:space:]]*:[[:space:]]*\([0-9][0-9]*\).*/\1/p' "$lifecycle" | head -n 1)
  served=""
  if [ -n "$owner" ]; then
    served=$(ps -o args= -p "$owner" 2>/dev/null | sed -n 's/.*--project \(.*\)$/\1/p' \
      | sed 's/ --[a-z-]* .*//')
  fi
  if [ -n "$served" ] && [ "$(canonical "$served")" != "$(canonical "$project_path")" ]; then
    echo "[up] The GraphHelm Runtime at $runtime_url (pid $owner) serves another folder of this project: $served (this Studio is for ${project_path%/}). Journeys run there would test that folder's code. Stop that Runtime or start this Studio with a different --bind." >&2
    exit 1
  fi
fi

# 1 - the Runtime.
if answers "$runtime_url/health"; then
  [ -f "$token_file" ] || { echo "[up] Something already answers at $runtime_url, and no token exists beside $events to prove it serves this project. Stop that service or pass a different --bind." >&2; exit 1; }
  # The header travels on stdin, never in argv, where `ps` would show the token.
  if printf 'Authorization: Bearer %s\n' "$(tr -d '\r\n' < "$token_file")" \
    | curl -fsS --max-time 5 -o /dev/null -H @- "$runtime_url/v1/executions?limit=1"; then
    echo "[up] Runtime already answering at $runtime_url and this project's token opens it - reusing it"
  else
    echo "[up] Something answers at $runtime_url but refuses this project's token - it is a different service or another project's Runtime. Stop it or pass a different --bind." >&2
    exit 1
  fi
else
  set -- serve --events "$events" --bind "$bind"
  [ -z "$project_path" ] || set -- "$@" --project "${project_path%/}"
  [ -z "$keyring" ] || set -- "$@" --keyring "$keyring"
  [ -z "$key_id" ] || set -- "$@" --key-id "$key_id"
  if [ -n "$manifest" ] && [ -n "$broker" ] && [ -n "$route" ] && [ -n "$keyring" ] && [ -n "$key_id" ]; then
    set -- "$@" --manifest "$manifest" --broker "$broker" --route "$route"
  fi
  echo "[up] starting: $graphhelm $*"
  log_dir=$(dirname -- "$trimmed")
  nohup "$graphhelm" "$@" > "$log_dir/serve.log" 2>&1 < /dev/null &
  runtime=$!
  # #549: a loaded machine can take a minute to answer; the wait is bounded and ends at once
  # when the Runtime it started has exited.
  waited=0
  until answers "$runtime_url/health"; do
    if ! kill -0 "$runtime" 2>/dev/null; then
      echo "[up] The Runtime '$graphhelm' exited before it answered at $runtime_url (log: $log_dir/serve.log). Is '$graphhelm' on PATH (or pass --graph-helm)?" >&2
      exit 1
    fi
    waited=$((waited + 1))
    [ "$waited" -lt 300 ] || { echo "[up] The Runtime did not answer at $runtime_url within 120s." >&2; exit 1; }
    sleep 0.4
  done
  echo "[up] Runtime up at $runtime_url"
  [ -n "$keyring" ] || echo "[up] WARNING: no --keyring given: the Studio opens read-and-drive only - SENDING MESSAGES WILL BE REFUSED." >&2
fi

if [ "$runtime_only" -eq 1 ]; then
  echo "[up] Runtime ready at $runtime_url (runtime only: no Studio started)"
  exit 0
fi

# 2 - dependencies, on a cold tree or one older than the lock file.
modules="$studio/node_modules"
if [ ! -f "$modules/.package-lock.json" ] || [ ! -e "$modules/.bin/vite" ] || [ "$studio/package-lock.json" -nt "$modules/.package-lock.json" ]; then
  echo "[up] installing Studio dependencies (npm ci: missing or older than package-lock.json)"
  (cd "$studio" && npm ci)
fi

# 3 - the dev server environment; the nonce travels only through the environment and the URL.
GRAPHHELM_EVENTS=$events
GRAPHHELM_RUNTIME_URL=$runtime_url
GRAPHHELM_STUDIO_SESSION_NONCE=$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')
export GRAPHHELM_EVENTS GRAPHHELM_RUNTIME_URL GRAPHHELM_STUDIO_SESSION_NONCE
if [ -n "$project" ]; then GRAPHHELM_PROJECT=$project; export GRAPHHELM_PROJECT; fi
if [ -n "$project_path" ]; then GRAPHHELM_PROJECT_PATH=$project_path; export GRAPHHELM_PROJECT_PATH; fi
nonce=$GRAPHHELM_STUDIO_SESSION_NONCE

# 4 - the door, opened only once THIS launch's session endpoint answers.
if [ "$no_browser" -eq 0 ]; then
  (
    tries=0
    while [ "$tries" -lt 60 ]; do
      if answers "http://127.0.0.1:$studio_port/__studio/session?nonce=$nonce"; then
        url="http://127.0.0.1:$studio_port/?session=$nonce"
        # `open` is macOS's; on Debian `open` can be openvt, so Linux always uses xdg-open.
        if [ "$(uname -s)" = Darwin ]; then open "$url"; else xdg-open "$url" >/dev/null 2>&1 || true; fi
        exit 0
      fi
      tries=$((tries + 1)); sleep 0.5
    done
  ) &
fi

echo "[up] Studio starting at http://127.0.0.1:$studio_port - the page opens already connected"
[ "$no_browser" -eq 0 ] || echo "[up] open: http://127.0.0.1:$studio_port/?session=$nonce"
cd "$studio"
exec npm run dev -- --port "$studio_port"
