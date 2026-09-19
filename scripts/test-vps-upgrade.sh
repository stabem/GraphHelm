#!/usr/bin/env bash
set -Eeuo pipefail

REPOSITORY_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
readonly REPOSITORY_ROOT
readonly UPGRADE_SCRIPT="${REPOSITORY_ROOT}/deploy/upgrade-vps.sh"

fail() {
  printf 'vps upgrade test: %s\n' "$*" >&2
  exit 1
}

SANDBOX="$(mktemp -d)"
cleanup() { rm -rf -- "${SANDBOX}"; }
trap cleanup EXIT

ROOT="${SANDBOX}/root"
FAKE_BIN="${SANDBOX}/bin"
SOURCE="${SANDBOX}/source"
LOG="${SANDBOX}/calls.log"
STATE="${SANDBOX}/service-state"
HEALTH_COUNT="${SANDBOX}/candidate-health-count"
MUTATION_MARKER="${SANDBOX}/store-mutated"
TEMPLATE="${SANDBOX}/candidate"
mkdir -p -- "${ROOT}/usr/local/bin" "${ROOT}/var/lib/graphhelm/events" \
  "${ROOT}/etc/systemd/system" "${ROOT}/etc" "${ROOT}/run/lock" \
  "${ROOT}/var/tmp" \
  "${ROOT}/opt/graphhelm-cargo/bin" "${ROOT}/opt/graphhelm-rustup" \
  "${ROOT}/var/cache/graphhelm-build/target/release" "${FAKE_BIN}" \
  "${SOURCE}/apps/cli" "${SOURCE}/adapters" "${SOURCE}/core" \
  "${SOURCE}/extensions" "${SOURCE}/schemas" "${SOURCE}/tools"
chmod 1777 "${ROOT}/var/tmp"
printf 'ID=ubuntu\n' > "${ROOT}/etc/os-release"
printf '[workspace]\n' > "${SOURCE}/Cargo.toml"
printf '# lock\n' > "${SOURCE}/Cargo.lock"
printf '[toolchain]\nchannel = "1.97.1"\n' > "${SOURCE}/rust-toolchain.toml"
printf '[package]\nname="graphhelm-cli"\nversion="0.0.0"\n' > "${SOURCE}/apps/cli/Cargo.toml"

cat > "${ROOT}/etc/systemd/system/graphhelm.service" <<'UNIT'
[Unit]
Description=GraphHelm Runtime API
After=network.target

[Service]
Type=simple
User=graphhelm
Group=graphhelm
WorkingDirectory=/var/lib/graphhelm
UMask=0077
ExecStart=/usr/local/bin/graphhelm serve --events /var/lib/graphhelm/events --bind 127.0.0.1:8080
Restart=on-failure
RestartSec=3s
NoNewPrivileges=true
PrivateTmp=true
ProtectHome=true
ProtectSystem=strict
ReadWritePaths=/var/lib/graphhelm
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictSUIDSGID=true
LockPersonality=true

[Install]
WantedBy=multi-user.target
UNIT
printf '%064d' 7 > "${ROOT}/var/lib/graphhelm/events.token"
printf 'event evidence\n' > "${ROOT}/var/lib/graphhelm/events/journal.jsonl"

make_cli() {
  local path="$1"
  local version="$2"
  cat > "${path}" <<FAKE
#!/usr/bin/env bash
set -eu
readonly CLI_VERSION='${version}'
printf 'graphhelm-%s actor=%s %s\n' "\${CLI_VERSION}" \
  "\${GRAPHHELM_TEST_EFFECTIVE_USER:-root}" "\$*" >> "\${TEST_LOG}"
case " \$* " in
  *" events backup --help "*|*" events restore --help "*)
    [[ "\${TEST_MISSING_VERB:-}" != "\$2" ]]
    ;;
  *" events verify "*)
    repository=''
    while ((\$#)); do
      case "\$1" in
        --repository) shift; repository="\$1" ;;
      esac
      shift
    done
    if [[ "\${TEST_VERIFY_HANG:-0}" == 1 ]]; then
      sleep 120
    fi
    if [[ "\${TEST_VERIFY_MUTATES:-0}" == 1 ]]; then
      printf 'candidate-only mutation\n' >> "\${repository}/journal.jsonl"
    fi
    if [[ "\${TEST_FORMAT_INCOMPATIBLE:-0}" == 1 ]]; then
      printf '{"ok":true,"formatSupported":false}\n'
    else
      printf '{"ok":true,"formatSupported":true}\n'
    fi
    ;;
  *" events backup "*)
    repository=''
    output=''
    while ((\$#)); do
      case "\$1" in
        --repository) shift; repository="\$1" ;;
        --output) shift; output="\$1" ;;
      esac
      shift
    done
    if [[ "\${TEST_BACKUP_FAIL:-0}" == 1 && "\${CLI_VERSION}" == old \
      && "\${output}" == *'/graphhelm-backup.'* ]]; then
      exit 9
    fi
    printf 'serializer=%s\n' "\${CLI_VERSION}" > "\${output}"
    cat "\${repository}/journal.jsonl" >> "\${output}"
    printf '{"ok":true,"command":"events.backup"}\n'
    ;;
  *" events restore "*)
    repository=''
    archive=''
    while ((\$#)); do
      case "\$1" in
        --repository) shift; repository="\$1" ;;
        --archive) shift; archive="\$1" ;;
      esac
      shift
    done
    [[ -s "\${archive}" ]]
    mkdir -p -- "\${repository}"
    tail -n +2 "\${archive}" > "\${repository}/journal.jsonl"
    printf '{"ok":true,"command":"events.restore"}\n'
    ;;
  *) exit 2 ;;
esac
FAKE
  chmod +x -- "${path}"
}

make_cli "${ROOT}/usr/local/bin/graphhelm" old
make_cli "${TEMPLATE}" new

cat > "${FAKE_BIN}/id" <<'FAKE'
#!/usr/bin/env bash
[[ "${1:-}" == -u ]] && printf '0\n'
FAKE

cat > "${FAKE_BIN}/systemctl" <<'FAKE'
#!/usr/bin/env bash
set -eu
printf 'systemctl %s\n' "$*" >> "${TEST_LOG}"
case "${1:-}" in
  is-active) [[ "$(cat "${TEST_STATE}")" == active ]] ;;
  stop) printf 'inactive\n' > "${TEST_STATE}" ;;
  start)
    if [[ ! -e "${TEST_ROOT}/var/lib/graphhelm/events.token" ]]; then
      printf '%064d' 8 > "${TEST_ROOT}/var/lib/graphhelm/events.token"
      chmod 0600 "${TEST_ROOT}/var/lib/graphhelm/events.token"
    fi
    printf 'active\n' > "${TEST_STATE}"
    ;;
  daemon-reload) ;;
  *) exit 2 ;;
esac
FAKE

cat > "${FAKE_BIN}/runuser" <<'FAKE'
#!/usr/bin/env bash
set -eu
run_user=''
while (($#)); do
  case "$1" in
    --user) shift; run_user="$1"; shift ;;
    --) shift; break ;;
    *) break ;;
  esac
done
printf 'runuser %s %s\n' "${run_user}" "$*" >> "${TEST_LOG}"
if [[ "${1:-}" == env && "${2:-}" == -i ]]; then
  shift 2
  exec env -i \
    GRAPHHELM_TEST_EFFECTIVE_USER="${run_user}" \
    TEST_LOG="${TEST_LOG}" \
    TEST_ROOT="${TEST_ROOT}" \
    TEST_MISSING_VERB="${TEST_MISSING_VERB:-}" \
    TEST_FORMAT_INCOMPATIBLE="${TEST_FORMAT_INCOMPATIBLE:-0}" \
    TEST_VERIFY_MUTATES="${TEST_VERIFY_MUTATES:-0}" \
    TEST_VERIFY_HANG="${TEST_VERIFY_HANG:-0}" \
    TEST_BACKUP_FAIL="${TEST_BACKUP_FAIL:-0}" \
    TEST_BUILD_OUTPUT_SYMLINK_TARGET="${TEST_BUILD_OUTPUT_SYMLINK_TARGET:-}" \
    "$@"
fi
exec env GRAPHHELM_TEST_EFFECTIVE_USER="${run_user}" "$@"
FAKE

cat > "${FAKE_BIN}/cargo" <<FAKE
#!/usr/bin/env bash
set -eu
printf 'cargo %s\n' "\$*" >> '${LOG}'
mkdir -p -- "\${CARGO_TARGET_DIR}/release"
if [[ -n "\${TEST_BUILD_OUTPUT_SYMLINK_TARGET:-}" ]]; then
  ln -s -- "\${TEST_BUILD_OUTPUT_SYMLINK_TARGET}" \
    "\${CARGO_TARGET_DIR}/release/graphhelm"
else
  cp -- '${TEMPLATE}' "\${CARGO_TARGET_DIR}/release/graphhelm"
fi
FAKE

cat > "${ROOT}/opt/graphhelm-cargo/bin/rustup" <<FAKE
#!/usr/bin/env bash
set -eu
printf 'rustup %s\n' "\$*" >> '${LOG}'
FAKE

cat > "${FAKE_BIN}/install" <<'FAKE'
#!/usr/bin/env bash
set -eu
directory=false
mode=''
paths=()
while (($#)); do
  case "$1" in
    -d|--directory) directory=true ;;
    -m|--mode) shift; mode="$1" ;;
    --owner|--group) shift ;;
    --) ;;
    *) paths+=("$1") ;;
  esac
  shift
done
if [[ "${directory}" == true ]]; then
  mkdir -p -- "${paths[@]}"
else
  ((${#paths[@]} == 2))
  if [[ "${TEST_INSTALL_BINARY_FAIL:-0}" == 1 \
    && "${paths[0]}" == *'/old.graphhelm' ]]; then
    exit 23
  fi
  cp -- "${paths[0]}" "${paths[1]}"
fi
[[ -z "${mode}" ]] || chmod "${mode}" -- "${paths[@]}" 2>/dev/null || true
FAKE

cat > "${FAKE_BIN}/flock" <<'FAKE'
#!/usr/bin/env bash
set -eu
printf 'flock %s\n' "$*" >> "${TEST_FLOCK_LOG}"
[[ "${TEST_FLOCK_FAIL:-0}" != 1 ]]
FAKE

cat > "${FAKE_BIN}/od" <<'FAKE'
#!/usr/bin/env bash
printf ' 7f 45 4c 46\n'
FAKE

cat > "${FAKE_BIN}/chown" <<'FAKE'
#!/usr/bin/env bash
exit 0
FAKE

cat > "${FAKE_BIN}/stat" <<'FAKE'
#!/usr/bin/env bash
set -eu
case "$*" in
  *"%u"*) printf '0\n' ;;
  *"%g"*) printf '0\n' ;;
  *"%s"*) /usr/bin/stat -c '%s' "${@: -1}" ;;
  *"%a"*)
    if [[ "${@: -1}" == "${TEST_ROOT}/var/tmp/graphhelm" ]]; then
      printf '755\n'
    else
      printf '600\n'
    fi
    ;;
  *) printf '600\n' ;;
esac
FAKE

cat > "${FAKE_BIN}/curl" <<'FAKE'
#!/usr/bin/env bash
set -eu
body="$(cat || true)"
printf 'curl %s\n' "$*" >> "${TEST_LOG}"
if [[ "${TEST_MUTATE_STORE:-0}" == 1 \
  && " $* " == *" http://127.0.0.1:8080/health "* \
  && ! -e "${TEST_MUTATION_MARKER}" \
  && $(grep -c 'CLI_VERSION=.new.' "${TEST_ROOT}/usr/local/bin/graphhelm") -eq 1 ]]; then
  printf 'unexpected mutation\n' >> "${TEST_ROOT}/var/lib/graphhelm/events/journal.jsonl"
  : > "${TEST_MUTATION_MARKER}"
fi
if [[ "${TEST_SMOKE_FAIL:-0}" == 1 ]] \
  && grep -q 'CLI_VERSION=.new.' "${TEST_ROOT}/usr/local/bin/graphhelm"; then
  exit 22
fi
if [[ "${TEST_FINAL_SMOKE_FAIL:-0}" == 1 \
  && " $* " == *" http://127.0.0.1:8080/health "* \
  && "$(cat "${TEST_STATE}")" == active \
  && $(grep -c 'CLI_VERSION=.new.' "${TEST_ROOT}/usr/local/bin/graphhelm") -eq 1 ]]; then
  health_count="$(( $(cat "${TEST_HEALTH_COUNT}") + 1 ))"
  printf '%s\n' "${health_count}" > "${TEST_HEALTH_COUNT}"
  if [[ "${health_count}" -ge 2 ]]; then exit 22; fi
fi
case " $* " in
  *"/v1/executions/upgrade-smoke "*|*"/v1/executions/restore-smoke "*)
    [[ -z "${body}" ]]
    printf 'curl-unauth protected-401\n' >> "${TEST_LOG}"
    printf '401'
    ;;
  *" --header @"*)
    [[ -z "${body}" ]]
    header_path=''
    previous=''
    for argument in "$@"; do
      if [[ "${previous}" == --header && "${argument}" == @* ]]; then
        header_path="${argument#@}"
        break
      fi
      previous="${argument}"
    done
    [[ -n "${header_path}" && -f "${header_path}" && ! -L "${header_path}" ]]
    [[ "$(stat -c '%a' "${header_path}")" == 600 ]]
    grep -Eq '^Authorization: Bearer [0-9a-f]{64}$' "${header_path}"
    printf 'curl-auth valid-token unknown-404\n' >> "${TEST_LOG}"
    printf '404'
    ;;
  *" --config - "*)
    [[ "${body}" == "header = \"Authorization: Bearer $(cat "${TEST_ROOT}/var/lib/graphhelm/events.token")\"" ]]
    printf 'curl-auth valid-token unknown-404\n' >> "${TEST_LOG}"
    printf '404'
    ;;
  *) printf '{"ok":true,"command":"serve.health"}' ;;
esac
FAKE

cat > "${FAKE_BIN}/sha256sum" <<'FAKE'
#!/usr/bin/env bash
set -eu
for argument in "$@"; do
  if [[ "${argument}" == "${TEST_ROOT}/var/lib/graphhelm/events.token" ]]; then
    printf 'sha256sum read service-owned token pathname\n' >> "${TEST_LOG}"
    exit 77
  fi
done
exec /usr/bin/sha256sum "$@"
FAKE

cat > "${FAKE_BIN}/tar" <<'FAKE'
#!/usr/bin/env bash
exec /usr/bin/tar "$@"
FAKE

cat > "${FAKE_BIN}/sleep" <<'FAKE'
#!/usr/bin/env bash
exit 0
FAKE

chmod +x -- "${FAKE_BIN}"/* "${ROOT}/opt/graphhelm-cargo/bin/rustup"
cp -- "${FAKE_BIN}/cargo" "${ROOT}/opt/graphhelm-cargo/bin/cargo"
chmod +x -- "${ROOT}/opt/graphhelm-cargo/bin/cargo"

export PATH="${FAKE_BIN}:/usr/bin:/bin"
export GRAPHHELM_VPS_ROOT="${ROOT}"
export TEST_LOG="${LOG}"
export TEST_STATE="${STATE}"
export TEST_HEALTH_COUNT="${HEALTH_COUNT}"
export TEST_MUTATION_MARKER="${MUTATION_MARKER}"
export TEST_ROOT="${ROOT}"
export TEST_FLOCK_LOG="${SANDBOX}/flock.log"

reset_case() {
  : > "${LOG}"
  : > "${TEST_FLOCK_LOG}"
  printf 'active\n' > "${STATE}"
  printf '0\n' > "${HEALTH_COUNT}"
  rm -f -- "${MUTATION_MARKER}"
  printf 'event evidence\n' > "${ROOT}/var/lib/graphhelm/events/journal.jsonl"
  printf '%064d' 7 > "${ROOT}/var/lib/graphhelm/events.token"
  make_cli "${ROOT}/usr/local/bin/graphhelm" old
  make_cli "${TEMPLATE}" new
  unset TEST_MISSING_VERB TEST_FORMAT_INCOMPATIBLE TEST_SMOKE_FAIL \
    TEST_FINAL_SMOKE_FAIL TEST_BACKUP_FAIL TEST_MUTATE_STORE TEST_FLOCK_FAIL \
    TEST_INSTALL_BINARY_FAIL TEST_VERIFY_MUTATES TEST_VERIFY_HANG TEST_BUILD_OUTPUT_SYMLINK_TARGET
  rm -rf -- "${ROOT}/var/backups/graphhelm-upgrade" "${ROOT}/var/tmp/graphhelm-source."* 2>/dev/null || true
  rm -rf -- "${ROOT}/var/cache/graphhelm-build/target"
  mkdir -p -- "${ROOT}/var/cache/graphhelm-build/target/release"
}

run_upgrade() {
  "${UPGRADE_SCRIPT}" --source "${SOURCE}"
}

# No-op: candidate is identical, so there must be no lock, stop, or backup.
reset_case
cp -- "${ROOT}/usr/local/bin/graphhelm" "${TEMPLATE}"
noop_output="$(run_upgrade)"
[[ "${noop_output}" == *'already current'* ]] || fail 'no-op was not clearly reported'
! grep -Eq 'systemctl stop|events backup --repository|flock ' "${LOG}" "${TEST_FLOCK_LOG}" \
  || fail 'no-op stopped, backed up, or acquired the operation lock'

# Successful swap: pre/post archive and config hashes match, evidence is retained.
reset_case
success_output="$(run_upgrade)"
grep -Fq 'cargo +1.97.1 build --locked --release -p graphhelm-cli' "${LOG}" \
  || fail 'candidate build did not use the exact locked release command'
grep -Fq 'graphhelm-new actor=graphhelm-build events backup --help' "${LOG}" \
  || fail 'candidate backup help did not run as graphhelm-build'
grep -Fq 'graphhelm-new actor=graphhelm-build events restore --help' "${LOG}" \
  || fail 'candidate restore help did not run as graphhelm-build'
grep -Fq 'graphhelm-new actor=nobody events verify' "${LOG}" \
  || fail 'candidate repository verify did not run as the isolated nobody user'
grep -F 'graphhelm-new actor=nobody events verify' "${LOG}" \
  | grep -Fq '/graphhelm-candidate.' \
  || fail 'candidate repository verify did not use an isolated repository copy'
! grep -F 'graphhelm-new actor=nobody events verify' "${LOG}" \
  | grep -Fq -- "--repository ${ROOT}/var/lib/graphhelm/events" \
  || fail 'candidate repository verify touched the live repository'
! grep -Fq 'graphhelm-new actor=root' "${LOG}" \
  || fail 'an untrusted candidate command ran as root'
grep -Fq 'graphhelm-old actor=graphhelm events backup' "${LOG}" \
  || fail 'preserved old CLI did not produce the post-smoke fingerprint as graphhelm'
! grep -Fq 'graphhelm-old actor=root events backup' "${LOG}" \
  || fail 'an event backup read the service-owned repository as root'
grep -Fq 'curl-unauth protected-401' "${LOG}" || fail 'unauthenticated 401 was not proved'
grep -Fq 'curl-auth valid-token unknown-404' "${LOG}" || fail 'valid-token 404 was not proved'
[[ "$(cat "${STATE}")" == active ]] || fail 'successful upgrade did not restart the service'
[[ "$(/usr/bin/stat -c '%a' "${ROOT}/var/tmp")" == 1777 ]] \
  || fail 'upgrade changed the system temporary directory mode'
grep -q 'CLI_VERSION=.new.' "${ROOT}/usr/local/bin/graphhelm" || fail 'candidate was not installed'
recovery="$(find "${ROOT}/var/backups/graphhelm-upgrade" -mindepth 1 -maxdepth 1 -type d -print -quit)"
[[ -n "${recovery}" && -s "${recovery}/old.graphhelm" && -s "${recovery}/fingerprints.sha256" ]] \
  || fail 'successful upgrade did not retain rollback evidence'
grep -Fxq 'serializer=old' "${recovery}/events-after.archive" \
  || fail 'post-smoke fingerprint did not use the exact preserved old CLI serializer'
[[ "${success_output}" == *'completed'* ]] || fail 'success was not clearly reported'

# Candidate verification is untrusted and must not hold the service/operation lock forever.
reset_case
export TEST_VERIFY_HANG=1
set +e
verify_hang_output="$(timeout 20s "${UPGRADE_SCRIPT}" --source "${SOURCE}" 2>&1)"
verify_hang_status=$?
set -e
unset TEST_VERIFY_HANG
[[ ${verify_hang_status} -ne 124 \
  && "${verify_hang_output}" != *'upgrade completed'* ]] \
  || fail 'hung candidate verification was not terminated by its deadline'
grep -q 'CLI_VERSION=.old.' "${ROOT}/usr/local/bin/graphhelm" \
  || fail 'hung candidate verification changed the installed binary'
[[ "$(cat "${STATE}")" == active ]] \
  || fail 'hung candidate verification left the prior service stopped'

# A candidate verify implementation may write, but only inside its isolated copy.
reset_case
export TEST_VERIFY_MUTATES=1
run_upgrade >/dev/null
[[ "$(cat "${ROOT}/var/lib/graphhelm/events/journal.jsonl")" == 'event evidence' ]] \
  || fail 'candidate verification mutated the live event repository'

# A failure inside backup after it stops the service must restart the unchanged old service.
reset_case
export TEST_BACKUP_FAIL=1
set +e
backup_failure_output="$(run_upgrade 2>&1)"
backup_failure_status=$?
set -e
[[ ${backup_failure_status} -ne 0 ]] || fail 'injected mid-backup failure succeeded'
[[ "${backup_failure_output}" != *'upgrade completed'* ]] \
  || fail 'mid-backup failure printed a success result'
grep -q 'CLI_VERSION=.old.' "${ROOT}/usr/local/bin/graphhelm" \
  || fail 'mid-backup failure changed the installed binary'
[[ "$(cat "${STATE}")" == active ]] || fail 'mid-backup failure left old service stopped'
! grep -Fq 'graphhelm-new actor=nobody events verify' "${LOG}" \
  || fail 'mid-backup failure continued into candidate verification'

# Missing required CLI verb: reject before lock/downtime.
reset_case
export TEST_MISSING_VERB=restore
set +e
missing_output="$(run_upgrade 2>&1)"
missing_status=$?
set -e
[[ ${missing_status} -ne 0 && "${missing_output}" == *'events restore'* ]] \
  || fail 'missing restore verb was not rejected'
! grep -Eq 'systemctl stop|events backup --repository' "${LOG}" || fail 'missing verb caused downtime'

# Incompatible repository: retain backup, keep old binary, restart old service.
reset_case
export TEST_FORMAT_INCOMPATIBLE=1
set +e
format_output="$(run_upgrade 2>&1)"
format_status=$?
set -e
[[ ${format_status} -ne 0 && "${format_output}" == *'release-specific migration is required'* ]] \
  || fail 'incompatible format did not name the required next step'
grep -q 'CLI_VERSION=.old.' "${ROOT}/usr/local/bin/graphhelm" || fail 'format refusal swapped the binary'
[[ "$(cat "${STATE}")" == active ]] || fail 'format refusal did not restart the old service'
find "${ROOT}/var/backups/graphhelm-upgrade" -type f -name 'graphhelm-vps-*.tar' -print -quit | grep -q . \
  || fail 'format refusal did not retain its backup'

# Failed candidate smoke: automatic binary and event/config restore, old health restored.
reset_case
export TEST_SMOKE_FAIL=1
token_before_rollback="$(cat "${ROOT}/var/lib/graphhelm/events.token")"
set +e
smoke_output="$(run_upgrade 2>&1)"
smoke_status=$?
set -e
if [[ ${smoke_status} -eq 0 || "${smoke_output}" != *'rolled back'* ]]; then
  printf '%s\n' "${smoke_output}" >&2
  find "${ROOT}/var/backups/graphhelm-upgrade" -type f -name rollback-restore.log \
    -exec sh -c 'printf "%s\n" "--- rollback log ---"; cat "$1"' _ {} \; >&2 || true
  fail 'smoke failure did not report automatic rollback'
fi
grep -q 'CLI_VERSION=.old.' "${ROOT}/usr/local/bin/graphhelm" || fail 'rollback did not restore old binary'
[[ "$(cat "${ROOT}/var/lib/graphhelm/events/journal.jsonl")" == 'event evidence' ]] \
  || fail 'rollback did not restore event evidence'
[[ "$(cat "${STATE}")" == active ]] || fail 'rollback did not restore old service health'
[[ "$(cat "${ROOT}/var/lib/graphhelm/events.token")" == "${token_before_rollback}" ]] \
  || fail 'automatic same-machine rollback rotated the Runtime bearer token'
grep -Fq 'graphhelm-old actor=graphhelm events backup' "${LOG}" \
  || fail 'failed-store evidence backup did not run as graphhelm'
! grep -Fq 'graphhelm-old actor=root events backup' "${LOG}" \
  || fail 'failed-store evidence backup read the repository as root'
find "${ROOT}/var/backups/graphhelm-upgrade" -type f -name 'failed-candidate.graphhelm' -print -quit | grep -q . \
  || fail 'rollback did not retain the failed candidate'

# A failed rollback copy must not replace the installed binary with mktemp's empty file.
reset_case
export TEST_SMOKE_FAIL=1
export TEST_INSTALL_BINARY_FAIL=1
set +e
install_failure_output="$(run_upgrade 2>&1)"
install_failure_status=$?
set -e
[[ ${install_failure_status} -ne 0 && "${install_failure_output}" == *'rollback was incomplete'* ]] \
  || fail 'rollback binary-copy failure was not reported as incomplete'
[[ -s "${ROOT}/usr/local/bin/graphhelm" ]] \
  || fail 'failed atomic binary install replaced the CLI with an empty file'
grep -q 'CLI_VERSION=.new.' "${ROOT}/usr/local/bin/graphhelm" \
  || fail 'failed atomic binary install replaced the existing candidate'
[[ "$(cat "${STATE}")" == inactive ]] \
  || fail 'incomplete binary rollback did not leave the service stopped'

# The post-fingerprint restart needs a fresh full smoke; its failure must roll back.
reset_case
export TEST_FINAL_SMOKE_FAIL=1
set +e
final_smoke_output="$(run_upgrade 2>&1)"
final_smoke_status=$?
set -e
[[ ${final_smoke_status} -ne 0 ]] || fail 'failed final smoke was declared successful'
[[ "${final_smoke_output}" != *'upgrade completed'* ]] \
  || fail 'failed final smoke printed a success result'
[[ "${final_smoke_output}" == *'rolled back'* ]] \
  || fail 'failed final smoke did not trigger automatic rollback'
grep -q 'CLI_VERSION=.old.' "${ROOT}/usr/local/bin/graphhelm" \
  || fail 'failed final smoke did not restore the old binary'
[[ "$(cat "${STATE}")" == active ]] || fail 'failed final smoke did not restore old health'

# A real event-store mutation must fail the old-CLI fingerprint and roll back.
reset_case
export TEST_MUTATE_STORE=1
set +e
mutation_output="$(run_upgrade 2>&1)"
mutation_status=$?
set -e
[[ ${mutation_status} -ne 0 && "${mutation_output}" == *'event store changed'* ]] \
  || fail 'event-store mutation was not rejected by fingerprint proof'
[[ "${mutation_output}" == *'rolled back'* ]] || fail 'event-store mutation did not roll back'
grep -q 'CLI_VERSION=.old.' "${ROOT}/usr/local/bin/graphhelm" \
  || fail 'event-store mutation rollback did not restore old binary'
[[ "$(cat "${ROOT}/var/lib/graphhelm/events/journal.jsonl")" == 'event evidence' ]] \
  || fail 'rollback did not restore the actual archived event data'
[[ "$(cat "${STATE}")" == active ]] || fail 'event-store mutation rollback did not restore health'

# Shared contention: refuse before touching service.
reset_case
export TEST_FLOCK_FAIL=1
set +e
lock_output="$(run_upgrade 2>&1)"
lock_status=$?
set -e
[[ ${lock_status} -ne 0 && "${lock_output}" == *'already running'* ]] \
  || fail 'shared lock contention was not refused'
! grep -Eq 'systemctl stop|events backup --repository' "${LOG}" || fail 'lock refusal touched the service'

# A symlink at the predictable lock path must never be followed or truncated.
reset_case
lock_path="${ROOT}/run/graphhelm-operation.lock"
lock_target="${SANDBOX}/lock-target"
rm -f -- "${lock_path}"
printf 'do not truncate\n' > "${lock_target}"
ln -s -- "${lock_target}" "${lock_path}"
set +e
symlink_output="$(run_upgrade 2>&1)"
symlink_status=$?
set -e
[[ ${symlink_status} -ne 0 && "${symlink_output}" == *'lock file is unsafe'* ]] \
  || fail 'upgrade accepted a symlink operation lock'
[[ "$(cat "${lock_target}")" == 'do not truncate' ]] \
  || fail 'upgrade followed or truncated the lock symlink target'
! grep -Eq '^systemctl |events backup --repository' "${LOG}" \
  || fail 'unsafe upgrade lock touched the service or backup'

# The build user must not redirect root preparation through a target-directory symlink.
reset_case
build_target="${ROOT}/var/cache/graphhelm-build/target"
build_victim="${SANDBOX}/build-target-victim"
rm -rf -- "${build_target}"
mkdir -- "${build_victim}"
printf 'do not change\n' > "${build_victim}/canary"
chmod 0701 "${build_victim}"
ln -s -- "${build_victim}" "${build_target}"
set +e
build_symlink_output="$(run_upgrade 2>&1)"
build_symlink_status=$?
set -e
[[ ${build_symlink_status} -ne 0 && "${build_symlink_output}" == *'build target directory is a symlink'* ]] \
  || fail 'upgrade accepted a symlink build target directory'
[[ "$(cat "${build_victim}/canary")" == 'do not change' \
  && "$(/usr/bin/stat -c '%a' "${build_victim}")" == 701 ]] \
  || fail 'upgrade followed or changed the build target symlink target'
! grep -Eq '^systemctl |events backup --repository' "${LOG}" \
  || fail 'unsafe build target touched the service or backup'

# A builder-controlled result pathname must not let root follow a file symlink.
reset_case
build_output_victim="${SANDBOX}/builder-output-victim"
printf 'root-only build material\n' > "${build_output_victim}"
export TEST_BUILD_OUTPUT_SYMLINK_TARGET="${build_output_victim}"
set +e
build_output_symlink_output="$(run_upgrade 2>&1)"
build_output_symlink_status=$?
set -e
[[ ${build_output_symlink_status} -ne 0 \
  && "${build_output_symlink_output}" == *'unsafe untrusted file'* ]] \
  || fail 'upgrade followed the builder-controlled candidate symlink'
[[ "$(cat "${build_output_victim}")" == 'root-only build material' ]] \
  || fail 'upgrade changed the builder output symlink target'
! grep -Eq '^systemctl |events backup --repository' "${LOG}" \
  || fail 'unsafe builder output touched the service or backup'

printf 'vps upgrade tests: PASS\n'
