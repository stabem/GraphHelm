# Sourced by fixture.sh: wait for a fixture process to answer over HTTP.
# wait_answers <url> <shell pid> [seconds]: 0 once the url answers; 2 as soon as the process is
# gone, because a Studio that already died will never answer, and the caller prints its log then
# instead of waiting out the launcher's own deadline (#1448: a broken vite install hid its error
# behind a 120 s launch timeout); 1 when the time ran out.
wait_answers() {
  local url=$1 pid=$2 seconds=${3:-60} _
  for _ in $(seq 1 "$seconds"); do
    curl -sf --max-time 10 -o /dev/null "$url" && return 0
    kill -0 "$pid" 2>/dev/null || return 2
    sleep 1
  done
  curl -sf --max-time 10 -o /dev/null "$url"
}
