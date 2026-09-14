# Clean Ubuntu VPS Acceptance Rehearsal

Recorded runs: [`docs/acceptance/install-rehearsal-2026-09-13.md`](../docs/acceptance/install-rehearsal-2026-09-13.md) — the Runtime half, in a clean Ubuntu 24.04 container (systemd absent, so section 4's service checks were not exercised there). The single-machine path that precedes any server is [`docs/install/GETTING_STARTED.md`](../docs/install/GETTING_STARTED.md).

This script proves the Docker and systemd installation paths on a new Ubuntu 24.04 VPS. Run every command on the VPS. A failed command, a different HTTP status, or a missing expected field fails the rehearsal.

## Preconditions

- A new Ubuntu 24.04 VPS with outbound internet access.
- A login user with passwordless or interactive `sudo` access.
- Nothing listening on `127.0.0.1:8080`.

## 1. Record the clean host

Run:

```bash
set -euo pipefail
. /etc/os-release
test "$ID" = "ubuntu"
test "$VERSION_ID" = "24.04"
! sudo ss -lnt '( sport = :8080 )' | grep -q LISTEN
printf 'clean host: %s %s\n' "$PRETTY_NAME" "$(uname -m)"
```

Expected:

- One line beginning with `clean host: Ubuntu 24.04`.
- Exit status `0`.

## 2. Fetch the source

Run:

```bash
sudo apt-get update
sudo apt-get install --yes --no-install-recommends ca-certificates git
git clone https://github.com/stabem/GraphHelm.git
cd GraphHelm
git switch main
git status --short --branch
```

Expected:

- The final line starts with `## main...origin/main`.
- No changed or untracked files are listed.

## 3. Prove the Docker path

Install the Ubuntu Docker packages:

```bash
sudo apt-get install --yes --no-install-recommends docker.io docker-compose-v2
sudo systemctl enable --now docker
sudo docker version --format '{{.Server.Version}}'
sudo docker compose version
```

Expected:

- Both version commands print a version and exit `0`.

Create a one-session token and start GraphHelm:

```bash
GRAPHHELM_API_TOKEN="$(od -An -N32 -tx1 /dev/urandom | tr -d ' \n')"
export GRAPHHELM_API_TOKEN
test "${#GRAPHHELM_API_TOKEN}" -eq 64
sudo env GRAPHHELM_API_TOKEN="$GRAPHHELM_API_TOKEN" docker compose up --build --detach
sudo env GRAPHHELM_API_TOKEN="$GRAPHHELM_API_TOKEN" docker compose ps
for attempt in $(seq 1 30); do
  if curl --silent --show-error --fail --max-time 2 http://127.0.0.1:8080/health >/dev/null; then break; fi
  sleep 1
done
curl --silent --show-error --fail http://127.0.0.1:8080/health >/dev/null
```

Expected:

- The image build completes.
- `graphhelm` is listed with state `Up` or `running`.
- Health returns before the bounded readiness loop ends.

Probe loopback and authentication:

```bash
curl --silent --show-error --fail http://127.0.0.1:8080/health | tee /tmp/graphhelm-health.json
grep -q '"ok":true' /tmp/graphhelm-health.json
grep -q '"command":"serve.health"' /tmp/graphhelm-health.json
test "$(curl --silent --output /dev/null --write-out '%{http_code}' http://127.0.0.1:8080/does-not-exist)" = "401"
test "$(curl --silent --output /dev/null --write-out '%{http_code}' --header "Authorization: Bearer $GRAPHHELM_API_TOKEN" http://127.0.0.1:8080/does-not-exist)" = "404"
```

Expected:

- `/health` returns JSON containing `"ok":true` and `"command":"serve.health"`.
- The unauthenticated probe is `401`.
- The authenticated probe reaches the router and is `404`.

Prove restart safety and the persistent volume:

```bash
sudo env GRAPHHELM_API_TOKEN="$GRAPHHELM_API_TOKEN" docker compose restart graphhelm
for attempt in $(seq 1 30); do
  if curl --silent --show-error --fail --max-time 2 http://127.0.0.1:8080/health >/dev/null; then break; fi
  sleep 1
done
curl --silent --show-error --fail http://127.0.0.1:8080/health >/dev/null
CONTAINER_ID="$(sudo env GRAPHHELM_API_TOKEN="$GRAPHHELM_API_TOKEN" docker compose ps --quiet graphhelm)"
sudo docker inspect "$CONTAINER_ID" --format '{{range .Mounts}}{{println .Destination .Type}}{{end}}' \
  | grep -q '^/var/lib/graphhelm volume$'
sudo env GRAPHHELM_API_TOKEN="$GRAPHHELM_API_TOKEN" docker compose down
! sudo ss -lnt '( sport = :8080 )' | grep -q LISTEN
```

Expected:

- Health returns after the restart.
- The mount check exits `0`, proving that `/var/lib/graphhelm` is a Docker volume.
- Port `8080` is free after `docker compose down`.

## 4. Prove the native systemd path

Run the installer once:

```bash
sudo ./install/install.sh
sudo systemctl is-enabled graphhelm.service
sudo systemctl is-active graphhelm.service
sudo systemctl status graphhelm.service --no-pager --lines 10
sudo systemctl show graphhelm.service --property=ExecStart --value
sudo stat --format '%U:%G %a %n' /var/lib/graphhelm/events.token
```

Expected:

- The installer prints the loopback URL and one 64-character bearer token.
- The next two commands print `enabled` and `active`.
- The `systemctl show` line contains `graphhelm serve --events /var/lib/graphhelm/events --bind 127.0.0.1:8080`.
- `stat` prints `graphhelm:graphhelm 600 /var/lib/graphhelm/events.token`.

Run the installer a second time and prove that it does not rotate the token:

```bash
TOKEN_BEFORE="$(sudo cat /var/lib/graphhelm/events.token)"
sudo ./install/install.sh
TOKEN_AFTER="$(sudo cat /var/lib/graphhelm/events.token)"
test "$TOKEN_BEFORE" = "$TOKEN_AFTER"
test "${#TOKEN_AFTER}" -eq 64
sudo systemctl is-active --quiet graphhelm.service
```

Expected:

- The second installation exits `0`.
- The token comparison and service check exit `0` with no output.

Probe the installed service:

```bash
curl --silent --show-error --fail http://127.0.0.1:8080/health | tee /tmp/graphhelm-systemd-health.json
grep -q '"ok":true' /tmp/graphhelm-systemd-health.json
test "$(curl --silent --output /dev/null --write-out '%{http_code}' http://127.0.0.1:8080/v1/executions/rehearsal)" = "401"
test "$(curl --silent --output /dev/null --write-out '%{http_code}' --header "Authorization: Bearer $TOKEN_AFTER" http://127.0.0.1:8080/does-not-exist)" = "404"
sudo journalctl --unit graphhelm.service --since '10 minutes ago' --no-pager
```

Expected:

- Health contains `"ok":true`.
- The request without a token is `401`.
- The request with the persisted token reaches the router and is `404`.
- The journal contains a `serve.started` JSON line and no restart loop or startup failure.

## 5. Final acceptance record

Run:

```bash
printf 'commit=%s\n' "$(git rev-parse HEAD)"
printf 'service=%s\n' "$(sudo systemctl is-active graphhelm.service)"
printf 'health=%s\n' "$(curl --silent --output /dev/null --write-out '%{http_code}' http://127.0.0.1:8080/health)"
printf 'token_sha256=%s\n' "$(printf '%s' "$TOKEN_AFTER" | sha256sum | cut -d' ' -f1)"
```

Expected:

- Four lines: the tested commit, `service=active`, `health=200`, and a token hash.
- The raw token is not copied into the acceptance record.
