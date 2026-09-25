#!/usr/bin/env bash
set -Eeuo pipefail

REPOSITORY_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
readonly REPOSITORY_ROOT
readonly BACKUP_SCRIPT="${REPOSITORY_ROOT}/deploy/backup-vps.sh"
readonly RESTORE_SCRIPT="${REPOSITORY_ROOT}/deploy/restore-vps.sh"
readonly RUNBOOK="${REPOSITORY_ROOT}/docs/operations/VPS_UPGRADE_BACKUP_RESTORE.md"

fail() {
  printf 'vps backup/restore test: %s\n' "$*" >&2
  exit 1
}

SANDBOX="$(mktemp -d)"
cleanup() {
  rm -rf -- "${SANDBOX}"
}
trap cleanup EXIT

ROOT="${SANDBOX}/root"
FAKE_BIN="${SANDBOX}/bin"
DESTINATION="${SANDBOX}/backups"
LOG="${SANDBOX}/calls.log"
STATE="${SANDBOX}/service-state"
mkdir -p -- "${ROOT}/usr/local/bin" "${ROOT}/var/lib/graphhelm/events" \
  "${ROOT}/etc/systemd/system" "${ROOT}/var/tmp" "${FAKE_BIN}" "${DESTINATION}"
chmod 1777 "${ROOT}/var/tmp"
printf 'active\n' > "${STATE}"
printf 'event evidence\n' > "${ROOT}/var/lib/graphhelm/events/journal.jsonl"
printf '%064d' 0 > "${ROOT}/var/lib/graphhelm/events.token"
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
CANONICAL_UNIT="${SANDBOX}/canonical-graphhelm.service"
cp -- "${ROOT}/etc/systemd/system/graphhelm.service" "${CANONICAL_UNIT}"

cat > "${FAKE_BIN}/id" <<'FAKE'
#!/usr/bin/env bash
[[ "${1:-}" == "-u" ]] && printf '0\n'
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
  cp -- "${paths[0]}" "${paths[1]}"
fi
[[ -z "${mode}" ]] || chmod "${mode}" -- "${paths[@]}" 2>/dev/null || true
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
      printf '%064d' 2 > "${TEST_ROOT}/var/lib/graphhelm/events.token"
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
    TEST_BACKUP_OUTPUT_SYMLINK_TARGET="${TEST_BACKUP_OUTPUT_SYMLINK_TARGET:-}" \
    TEST_BACKUP_LARGE_WRITE="${TEST_BACKUP_LARGE_WRITE:-0}" \
    TEST_BACKUP_LARGE_ALLOC="${TEST_BACKUP_LARGE_ALLOC:-0}" \
    TEST_RESTORE_FAIL="${TEST_RESTORE_FAIL:-0}" \
    PATH="${PATH}" \
    "$@"
fi
exec env GRAPHHELM_TEST_EFFECTIVE_USER="${run_user}" "$@"
FAKE

cat > "${FAKE_BIN}/stat" <<'FAKE'
#!/usr/bin/env bash
set -eu
if [[ "${@: -1}" == /proc/self/fd/* && "${1:-}" != -L* ]]; then
  printf '500\n'
  exit 0
fi
case "${2:-}" in
  '%u')
    if [[ -n "${TEST_DESTINATION_UID:-}" && "${3:-}" == "${TEST_DESTINATION:-}" ]]; then
      printf '%s\n' "${TEST_DESTINATION_UID}"
    elif [[ -n "${TEST_DESTINATION_PARENT_UID:-}" \
      && "${3:-}" == "${TEST_DESTINATION_PARENT:-}" ]]; then
      printf '%s\n' "${TEST_DESTINATION_PARENT_UID}"
    else
      printf '0\n'
    fi
    ;;
  '%g')
    if [[ -n "${TEST_DESTINATION_PARENT_GID:-}" \
      && "${3:-}" == "${TEST_DESTINATION_PARENT:-}" ]]; then
      printf '%s\n' "${TEST_DESTINATION_PARENT_GID}"
    else
      printf '0\n'
    fi
    ;;
  '%s')
    if [[ "${3:-}" == */events.archive && -n "${TEST_EVENT_ARCHIVE_SIZE:-}" ]]; then
      printf '%s\n' "${TEST_EVENT_ARCHIVE_SIZE}"
    else
      /usr/bin/stat -Lc '%s' "${@: -1}"
    fi
    ;;
  '%a')
    if [[ -n "${TEST_DESTINATION_MODE:-}" && "${3:-}" == "${TEST_DESTINATION:-}" ]]; then
      printf '%s\n' "${TEST_DESTINATION_MODE}"
    elif [[ -n "${TEST_DESTINATION_PARENT_MODE:-}" \
      && "${3:-}" == "${TEST_DESTINATION_PARENT:-}" ]]; then
      printf '%s\n' "${TEST_DESTINATION_PARENT_MODE}"
    elif [[ "${3:-}" == */var/tmp/graphhelm ]]; then
      printf '755\n'
    else
      printf '600\n'
    fi
    ;;
  *) printf '600\n' ;;
esac
FAKE

cat > "${FAKE_BIN}/flock" <<'FAKE'
#!/usr/bin/env bash
set -eu
printf 'flock %s\n' "$*" >> "${TEST_FLOCK_LOG}"
[[ "${TEST_FLOCK_FAIL:-0}" != 1 ]]
FAKE

cat > "${FAKE_BIN}/tar" <<'FAKE'
#!/usr/bin/env bash
set -eu
resolved_args=" $* "
for argument in "$@"; do
  if [[ "${argument}" == /proc/self/fd/* ]]; then
    resolved_args+=" $(readlink -f -- "${argument}") "
  fi
done
if [[ -n "${TEST_SWAP_BUNDLE:-}" && ! -e "${TEST_SWAP_MARKER}" \
  && "${resolved_args}" == *" ${TEST_SWAP_BUNDLE} "* ]]; then
  mv -- "${TEST_SWAP_BUNDLE}" "${TEST_SWAP_BUNDLE}.opened-inode"
  cp -- "${TEST_SWAP_REPLACEMENT}" "${TEST_SWAP_BUNDLE}"
  : > "${TEST_SWAP_MARKER}"
fi
if [[ " ${resolved_args} " == *' --numeric-owner -tvf '* \
  && "${resolved_args}" == *'oversized.tar'* ]]; then
  /usr/bin/tar "$@" | awk '{ if ($NF == "events.archive") $3 = "8589934593"; print }'
else
  exec /usr/bin/tar "$@"
fi
FAKE

cat > "${FAKE_BIN}/mv" <<'FAKE'
#!/usr/bin/env bash
set -eu
args=("$@")
if [[ "${args[0]:-}" == -- ]]; then
  args=("${args[@]:1}")
fi
if [[ "${TEST_ROLLBACK_MV_FAIL:-0}" == 1 \
  && "${args[0]:-}" == *'/events.quarantine.'* \
  && "${args[1]:-}" == */events ]]; then
  exit 12
fi
exec /usr/bin/mv "$@"
FAKE

cat > "${FAKE_BIN}/prlimit" <<'FAKE'
#!/usr/bin/env bash
set -eu
fsize=''
address_space=''
while (($#)); do
  case "$1" in
    --fsize=*) fsize="${1#*=}" ;;
    --as=*) address_space="${1#*=}" ;;
    --) shift; break ;;
    *) exit 2 ;;
  esac
  shift
done
[[ -n "${fsize}" && -n "${address_space}" && $# -gt 0 ]]
printf 'prlimit fsize=%s as=%s\n' "${fsize}" "${address_space}" >> "${TEST_LOG}"
ulimit -f "$(( (fsize + 1023) / 1024 ))"
ulimit -v "$(( (address_space + 1023) / 1024 ))"
exec "$@"
FAKE

cat > "${FAKE_BIN}/cp" <<'FAKE'
#!/usr/bin/env bash
set -eu
if [[ "${TEST_TOKEN_PRIOR_SYMLINK_ON_CP:-0}" == 1 \
  && ! -e "${TEST_TOKEN_PRIOR_SYMLINK_MARKER}" ]]; then
  for argument in "$@"; do
    if [[ "${argument}" == "${TEST_ROOT}/var/lib/graphhelm/events.token" ]]; then
      rm -f -- "${argument}"
      ln -s -- "${TEST_TOKEN_PRIOR_SYMLINK_TARGET}" "${argument}"
      : > "${TEST_TOKEN_PRIOR_SYMLINK_MARKER}"
      break
    fi
  done
fi
exec /usr/bin/cp "$@"
FAKE

cat > "${ROOT}/usr/local/bin/graphhelm" <<'FAKE'
#!/usr/bin/env bash
set -eu
printf 'graphhelm actor=%s %s\n' "${GRAPHHELM_TEST_EFFECTIVE_USER:-root}" "$*" >> "${TEST_LOG}"
case " $* " in
  *" events backup "*)
    while (($#)); do
      if [[ "$1" == --output ]]; then
        shift
        if [[ -n "${TEST_BACKUP_OUTPUT_SYMLINK_TARGET:-}" ]]; then
          ln -s -- "${TEST_BACKUP_OUTPUT_SYMLINK_TARGET}" "$1"
          printf '{"ok":true,"command":"events.backup"}\n'
          exit 0
        fi
        if [[ "${TEST_BACKUP_LARGE_WRITE:-0}" == 1 ]]; then
          dd if=/dev/zero of="$1" bs=1048576 count=300 status=none
        elif [[ "${TEST_BACKUP_LARGE_ALLOC:-0}" == 1 ]]; then
          python3 -c 'bytearray(2 * 1024 * 1024 * 1024)'
        else
          printf '{"archiveVersion":"1.0.0","journal":"event evidence\\n","blobs":{}}' > "$1"
        fi
        printf '{"ok":true,"command":"events.backup"}\n'
        exit 0
      fi
      shift
    done
    ;;
  *" events restore "*)
    repository=''
    archive=''
    while (($#)); do
      case "$1" in
        --repository) shift; repository="$1" ;;
        --archive) shift; archive="$1" ;;
      esac
      shift
    done
    mkdir -p -- "$repository"
    # What the restore could see beside the archive, recorded at the instant it ran. The stage is
    # removed by the script's EXIT trap, so nothing after the run can inspect it -- this is the
    # only moment the question can be asked.
    if [[ -n "${archive:-}" ]]; then
      printf 'archive-dir %s [%s]\n' "$(dirname -- "${archive}")" \
        "$(cd -- "$(dirname -- "${archive}")" && printf '%s ' * | sed 's/ $//')" >> "${TEST_LOG}"
      [[ -r "${archive}" ]] || { printf 'the archive was not readable\n' >&2; exit 9; }
    fi
    if [[ "${TEST_RESTORE_FAIL:-0}" == 1 ]]; then
      printf 'failed restored evidence\n' > "${repository}/journal.jsonl"
      exit 9
    fi
    printf 'restored evidence\n' > "${repository}/journal.jsonl"
    printf '{"ok":true,"command":"events.restore"}\n'
    exit 0
    ;;
esac
exit 2
FAKE

cat > "${FAKE_BIN}/curl" <<'FAKE'
#!/usr/bin/env bash
set -eu
body="$(cat || true)"
printf 'curl %s\n' "$*" >> "${TEST_LOG}"
[[ "$body" != *"$(cat "${TEST_ROOT}/var/lib/graphhelm/events.token")"* ]] \
  || [[ "$body" == *'Authorization: Bearer '* ]]
if [[ "${TEST_TOKEN_SYMLINK_ON_HEALTH:-0}" == 1 \
  && " $* " == *" http://127.0.0.1:8080/health "* ]]; then
  if [[ ! -e "${TEST_TOKEN_SWAP_MARKER}" ]]; then
    rm -f -- "${TEST_ROOT}/var/lib/graphhelm/events.token"
    ln -s -- "${TEST_TOKEN_SYMLINK_TARGET}" \
      "${TEST_ROOT}/var/lib/graphhelm/events.token"
    : > "${TEST_TOKEN_SWAP_MARKER}"
  fi
  exit 22
fi
if [[ "${TEST_TOKEN_FIFO_ON_HEALTH:-0}" == 1 \
  && " $* " == *" http://127.0.0.1:8080/health "* \
  && ! -e "${TEST_TOKEN_FIFO_MARKER}" ]]; then
  rm -f -- "${TEST_ROOT}/var/lib/graphhelm/events.token"
  mkfifo -- "${TEST_ROOT}/var/lib/graphhelm/events.token"
  : > "${TEST_TOKEN_FIFO_MARKER}"
fi
if [[ "${TEST_TOKEN_OVERSIZED_ON_HEALTH:-0}" == 1 \
  && " $* " == *" http://127.0.0.1:8080/health "* \
  && ! -e "${TEST_TOKEN_OVERSIZED_MARKER}" ]]; then
  printf '%065d' 3 > "${TEST_ROOT}/var/lib/graphhelm/events.token"
  chmod 0600 "${TEST_ROOT}/var/lib/graphhelm/events.token"
  : > "${TEST_TOKEN_OVERSIZED_MARKER}"
fi
case " $* " in
  *"/v1/executions/restore-smoke "*) printf '401' ;;
  *" --write-out "*) printf '404' ;;
  *) printf '{"ok":true,"command":"serve.health"}' ;;
esac
FAKE

chmod +x -- "${FAKE_BIN}/id" "${FAKE_BIN}/install" "${FAKE_BIN}/systemctl" \
  "${FAKE_BIN}/stat" "${FAKE_BIN}/flock" "${FAKE_BIN}/tar" "${FAKE_BIN}/mv" \
  "${FAKE_BIN}/cp" "${FAKE_BIN}/curl" "${FAKE_BIN}/runuser" "${FAKE_BIN}/prlimit" \
  "${ROOT}/usr/local/bin/graphhelm"

export PATH="${FAKE_BIN}:/usr/bin:/bin"
export GRAPHHELM_VPS_ROOT="${ROOT}"
export TEST_LOG="${LOG}"
export TEST_STATE="${STATE}"
export TEST_ROOT="${ROOT}"
export TEST_FLOCK_LOG="${SANDBOX}/flock.log"
export TEST_DESTINATION="${DESTINATION}"

replacement_instructions="$(sed -n '182,190p' "${RUNBOOK}")"
grep -Fq 'install -m 0755 /opt/GraphHelm/deploy/seal-vps-file.py /usr/local/libexec/graphhelm/seal-vps-file.py' \
  <<< "${replacement_instructions}" \
  || fail 'replacement instructions did not install the file-sealing helper'

# An existing destination is accepted only when root-owned and non-writable by group/other.
chmod 0700 -- "${DESTINATION}"
export TEST_DESTINATION_UID=1001
set +e
untrusted_destination_output="$(${BACKUP_SCRIPT} --destination "${DESTINATION}" 2>&1)"
untrusted_destination_status=$?
set -e
unset TEST_DESTINATION_UID
[[ ${untrusted_destination_status} -ne 0 \
  && "${untrusted_destination_output}" == *'root-owned'* ]] \
  || fail 'backup accepted a destination owned by an untrusted account'

export TEST_DESTINATION_MODE=0770
set +e
writable_destination_output="$(${BACKUP_SCRIPT} --destination "${DESTINATION}" 2>&1)"
writable_destination_status=$?
set -e
unset TEST_DESTINATION_MODE
[[ ${writable_destination_status} -ne 0 \
  && "${writable_destination_output}" == *'non-writable'* ]] \
  || fail 'backup accepted a group-writable destination'

# The destination itself can be root-owned and mode 0700 while an untrusted writable parent
# still permits pathname replacement after the destination check. The parent must be refused too.
UNTRUSTED_PARENT="${SANDBOX}/untrusted-parent"
UNTRUSTED_PARENT_DESTINATION="${UNTRUSTED_PARENT}/root-owned-destination"
mkdir -- "${UNTRUSTED_PARENT}"
mkdir -- "${UNTRUSTED_PARENT_DESTINATION}"
chmod 0700 -- "${UNTRUSTED_PARENT_DESTINATION}"
export TEST_DESTINATION="${UNTRUSTED_PARENT_DESTINATION}"
export TEST_DESTINATION_PARENT="${UNTRUSTED_PARENT}"
export TEST_DESTINATION_PARENT_UID=1001
export TEST_DESTINATION_PARENT_GID=1001
export TEST_DESTINATION_PARENT_MODE=0770
set +e
unsafe_parent_output="$(${BACKUP_SCRIPT} --destination "${UNTRUSTED_PARENT_DESTINATION}" 2>&1)"
unsafe_parent_status=$?
set -e
unset TEST_DESTINATION_PARENT TEST_DESTINATION_PARENT_UID TEST_DESTINATION_PARENT_GID \
  TEST_DESTINATION_PARENT_MODE
export TEST_DESTINATION="${DESTINATION}"
[[ ${unsafe_parent_status} -ne 0 \
  && "${unsafe_parent_output}" == *'destination path component'* ]] \
  || fail 'backup accepted a root-owned destination beneath an untrusted writable parent'
! find "${UNTRUSTED_PARENT_DESTINATION}" -maxdepth 1 -type f \
  -name 'graphhelm-vps-*.tar' | grep -q . \
  || fail 'unsafe parent test published a bundle'

# A service-owned result pathname must not let root follow a replacement symlink.
SYMLINK_DESTINATION="${SANDBOX}/symlink-backups"
SYMLINK_VICTIM="${SANDBOX}/root-readable-victim"
mkdir -- "${SYMLINK_DESTINATION}"
printf 'root-only material\n' > "${SYMLINK_VICTIM}"
export TEST_BACKUP_OUTPUT_SYMLINK_TARGET="${SYMLINK_VICTIM}"
set +e
staging_symlink_output="$(${BACKUP_SCRIPT} --destination "${SYMLINK_DESTINATION}" 2>&1)"
staging_symlink_status=$?
set -e
unset TEST_BACKUP_OUTPUT_SYMLINK_TARGET
[[ ${staging_symlink_status} -ne 0 \
  && "${staging_symlink_output}" == *'unsafe untrusted file'* ]] \
  || fail 'backup followed a service-owned staging symlink'
! find "${SYMLINK_DESTINATION}" -maxdepth 1 -type f -name 'graphhelm-vps-*.tar' | grep -q . \
  || fail 'backup published a bundle from a service-owned staging symlink'
[[ "$(cat "${SYMLINK_VICTIM}")" == 'root-only material' ]] \
  || fail 'backup changed the staging symlink target'

# Backup must never publish an archive that the restore helper will reject.
export TEST_EVENT_ARCHIVE_SIZE=8589934593
bundles_before="$(find "${DESTINATION}" -maxdepth 1 -type f -name 'graphhelm-vps-*.tar' | wc -l)"
set +e
oversized_backup_output="$(${BACKUP_SCRIPT} --destination "${DESTINATION}" 2>&1)"
oversized_backup_status=$?
set -e
unset TEST_EVENT_ARCHIVE_SIZE
if [[ ${oversized_backup_status} -eq 0 \
  || "${oversized_backup_output}" != *'restore limit of 256 MiB'* ]]; then
  printf '%s\n' "${oversized_backup_output}" >&2
  fail 'backup accepted an event archive above the restore limit'
fi
[[ "$(find "${DESTINATION}" -maxdepth 1 -type f -name 'graphhelm-vps-*.tar' | wc -l)" \
  == "${bundles_before}" ]] || fail 'oversized backup published a bundle'
[[ "$(cat "${STATE}")" == active ]] || fail 'oversized backup did not restart the service'

# The producer must be bounded before it builds an oversized archive. The write limit stops a
# 300 MiB producer at the 256 MiB restore ceiling, and the address-space limit stops a producer
# that tries to allocate a 2 GiB JSON buffer. Both failures must restart the service.
export TEST_BACKUP_LARGE_WRITE=1
set +e
large_write_output="$(${BACKUP_SCRIPT} --destination "${DESTINATION}" 2>&1)"
large_write_status=$?
set -e
unset TEST_BACKUP_LARGE_WRITE
[[ ${large_write_status} -ne 0 && "${large_write_output}" != *'events.backup'* ]] \
  || fail 'backup producer exceeded the file-size limit without failing closed'
[[ "$(cat "${STATE}")" == active ]] || fail 'file-size limit left service stopped'

export TEST_BACKUP_LARGE_ALLOC=1
set +e
large_alloc_output="$(${BACKUP_SCRIPT} --destination "${DESTINATION}" 2>&1)"
large_alloc_status=$?
set -e
unset TEST_BACKUP_LARGE_ALLOC
[[ ${large_alloc_status} -ne 0 && "${large_alloc_output}" != *'events.backup'* ]] \
  || fail 'backup producer exceeded the address-space limit without failing closed'
[[ "$(cat "${STATE}")" == active ]] || fail 'address-space limit left service stopped'

backup_output="$(${BACKUP_SCRIPT} --destination "${DESTINATION}")"
grep -Fq 'prlimit fsize=268435456 as=1073741824' "${LOG}" \
  || fail 'backup producer did not run under the documented kernel resource limits'
[[ "${backup_output}" != *"$(cat "${ROOT}/var/lib/graphhelm/events.token")"* ]] \
  || fail 'backup printed the raw token'
BUNDLE="$(find "${DESTINATION}" -maxdepth 1 -type f -name 'graphhelm-vps-*.tar' -print -quit)"
[[ -n "${BUNDLE}" ]] || fail 'backup did not atomically publish a bundle'
[[ "$(/usr/bin/stat -c '%a' "${ROOT}/var/tmp")" == 1777 ]] \
  || fail 'backup changed the system temporary directory mode'
! find "${DESTINATION}" -maxdepth 1 -type f -name '.graphhelm-vps.*' | grep -q . \
  || fail 'backup left a temporary bundle after publication'
[[ "$(stat -c '%a' "${BUNDLE}")" == 600 ]] || fail 'bundle mode is not 0600'
tar -tf "${BUNDLE}" | sort > "${SANDBOX}/members"
diff -u <(printf '%s\n' events.archive graphhelm.service manifest.sha256 | sort) \
  "${SANDBOX}/members"
! grep -aFq "$(printf '%064d' 0)" "${BUNDLE}" \
  || fail 'backup bundle contains the Runtime bearer token'
grep -Fq 'graphhelm actor=graphhelm events backup --repository' "${LOG}" \
  || fail 'backup did not use the local repository selector'
grep -Fq 'runuser graphhelm env -i' "${LOG}" \
  || fail 'backup did not drop privileges to the graphhelm service user'
grep -Fq 'systemctl stop graphhelm.service' "${LOG}" || fail 'backup did not stop the service'
grep -Fq 'systemctl start graphhelm.service' "${LOG}" || fail 'backup did not restart the service'
! grep -Fq -- '--config' "${LOG}" || fail 'a Postgres/config selector was used'
grep -Fq 'flock --nonblock' "${TEST_FLOCK_LOG}" || fail 'backup did not acquire a nonblocking lock'
systemctl_before="$(grep -c '^systemctl ' "${LOG}")"
export TEST_FLOCK_FAIL=1
set +e
lock_refusal="$(${BACKUP_SCRIPT} --destination "${DESTINATION}" 2>&1)"
lock_status=$?
set -e
unset TEST_FLOCK_FAIL
[[ ${lock_status} -ne 0 && "${lock_refusal}" == *'already running'* ]] \
  || fail 'overlapping backup was not refused by the single-instance lock'
[[ "$(grep -c '^systemctl ' "${LOG}")" == "${systemctl_before}" ]] \
  || fail 'lock refusal touched the service'

systemctl_before="$(grep -c '^systemctl ' "${LOG}")"
set +e
contained_refusal="$(${BACKUP_SCRIPT} --destination "${ROOT}/var/lib/graphhelm/events/backups" 2>&1)"
contained_status=$?
set -e
[[ ${contained_status} -ne 0 && "${contained_refusal}" == *'event repository'* ]] \
  || fail 'backup accepted a destination inside the event repository'
[[ ! -e "${ROOT}/var/lib/graphhelm/events/backups" ]] \
  || fail 'contained backup destination was created before refusal'
[[ "$(grep -c '^systemctl ' "${LOG}")" == "${systemctl_before}" ]] \
  || fail 'contained destination refusal touched the service'

export TEST_FLOCK_FAIL=1
systemctl_before="$(grep -c '^systemctl ' "${LOG}")"
set +e
restore_lock_refusal="$(${RESTORE_SCRIPT} --bundle "${BUNDLE}" --replace-existing 2>&1)"
restore_lock_status=$?
set -e
unset TEST_FLOCK_FAIL
[[ ${restore_lock_status} -ne 0 && "${restore_lock_refusal}" == *'already running'* ]] \
  || fail 'restore did not contend on the shared operation lock'
[[ "$(grep -c '^systemctl ' "${LOG}")" == "${systemctl_before}" ]] \
  || fail 'restore lock refusal touched the service'

LOCK_PATH="${ROOT}/run/graphhelm-operation.lock"
LOCK_TARGET="${SANDBOX}/lock-target"
rm -f -- "${LOCK_PATH}"
printf 'do not truncate\n' > "${LOCK_TARGET}"
ln -s -- "${LOCK_TARGET}" "${LOCK_PATH}"
systemctl_before="$(grep -c '^systemctl ' "${LOG}")"
set +e
backup_symlink_output="$(${BACKUP_SCRIPT} --destination "${DESTINATION}" 2>&1)"
backup_symlink_status=$?
restore_symlink_output="$(${RESTORE_SCRIPT} --bundle "${BUNDLE}" --replace-existing 2>&1)"
restore_symlink_status=$?
set -e
[[ ${backup_symlink_status} -ne 0 && "${backup_symlink_output}" == *'lock file is unsafe'* ]] \
  || fail 'backup accepted a symlink operation lock'
[[ ${restore_symlink_status} -ne 0 && "${restore_symlink_output}" == *'lock file is unsafe'* ]] \
  || fail 'restore accepted a symlink operation lock'
[[ "$(cat "${LOCK_TARGET}")" == 'do not truncate' ]] \
  || fail 'backup or restore followed the lock symlink target'
[[ "$(grep -c '^systemctl ' "${LOG}")" == "${systemctl_before}" ]] \
  || fail 'unsafe lock refusal touched the service'
rm -f -- "${LOCK_PATH}"
printf '' > "${LOCK_PATH}"
chmod 0600 "${LOCK_PATH}"

MALICIOUS_DIR="${SANDBOX}/malicious"
MALICIOUS_BUNDLE="${SANDBOX}/malicious.tar"
mkdir -- "${MALICIOUS_DIR}"
tar -xf "${BUNDLE}" -C "${MALICIOUS_DIR}"
printf 'outside\n' > "${SANDBOX}/outside"
(cd -- "${MALICIOUS_DIR}" && sha256sum "${SANDBOX}/outside" >> manifest.sha256)
tar -cf "${MALICIOUS_BUNDLE}" -C "${MALICIOUS_DIR}" \
  events.archive graphhelm.service manifest.sha256
chmod 0600 "${MALICIOUS_BUNDLE}" 2>/dev/null || true
calls_before="$(wc -l < "${LOG}")"
set +e
${RESTORE_SCRIPT} --bundle "${MALICIOUS_BUNDLE}" --replace-existing >/dev/null 2>&1
malicious_status=$?
set -e
[[ ${malicious_status} -ne 0 ]] || fail 'restore accepted a manifest that checks an outside file'
[[ "$(wc -l < "${LOG}")" == "${calls_before}" ]] \
  || fail 'restore touched the service before rejecting an unsafe manifest'

HOSTILE_DIR="${SANDBOX}/hostile-unit"
HOSTILE_BUNDLE="${SANDBOX}/hostile-unit.tar"
mkdir -- "${HOSTILE_DIR}"
tar -xf "${BUNDLE}" -C "${HOSTILE_DIR}"
printf 'ExecStartPre=/bin/sh -c evil\n' >> "${HOSTILE_DIR}/graphhelm.service"
(cd -- "${HOSTILE_DIR}" && sha256sum events.archive graphhelm.service > manifest.sha256)
tar -cf "${HOSTILE_BUNDLE}" -C "${HOSTILE_DIR}" \
  events.archive graphhelm.service manifest.sha256
chmod 0600 "${HOSTILE_BUNDLE}" 2>/dev/null || true
calls_before="$(wc -l < "${LOG}")"
set +e
${RESTORE_SCRIPT} --bundle "${HOSTILE_BUNDLE}" --replace-existing >/dev/null 2>&1
hostile_status=$?
set -e
[[ ${hostile_status} -ne 0 ]] || fail 'restore accepted an extra systemd execution directive'
[[ "$(wc -l < "${LOG}")" == "${calls_before}" ]] \
  || fail 'hostile unit refusal touched the service'

# Swap the pathname after restore opens it. Every tar pass must stay on the opened inode.
SWAP_BUNDLE="${SANDBOX}/swap-after-open.tar"
cp -- "${BUNDLE}" "${SWAP_BUNDLE}"
chmod 0600 "${SWAP_BUNDLE}" 2>/dev/null || true
export TEST_SWAP_BUNDLE="${SWAP_BUNDLE}"
export TEST_SWAP_REPLACEMENT="${HOSTILE_BUNDLE}"
export TEST_SWAP_MARKER="${SANDBOX}/swap-triggered"
calls_before="$(wc -l < "${LOG}")"
set +e
swap_output="$(${RESTORE_SCRIPT} --bundle "${SWAP_BUNDLE}" 2>&1)"
swap_status=$?
set -e
unset TEST_SWAP_BUNDLE TEST_SWAP_REPLACEMENT TEST_SWAP_MARKER
[[ ${swap_status} -ne 0 && "${swap_output}" == *'event repository already exists'* ]] \
  || fail 'restore did not keep all validation passes on the originally opened bundle inode'
[[ -e "${SANDBOX}/swap-triggered" ]] || fail 'bundle pathname swap injection did not run'
[[ "$(wc -l < "${LOG}")" == "${calls_before}" ]] \
  || fail 'bundle pathname swap test touched the service'

OVERSIZED_BUNDLE="${SANDBOX}/oversized.tar"
cp -- "${BUNDLE}" "${OVERSIZED_BUNDLE}"
chmod 0600 "${OVERSIZED_BUNDLE}" 2>/dev/null || true
calls_before="$(wc -l < "${LOG}")"
set +e
${RESTORE_SCRIPT} --bundle "${OVERSIZED_BUNDLE}" --replace-existing >/dev/null 2>&1
oversized_status=$?
set -e
[[ ${oversized_status} -ne 0 ]] || fail 'restore accepted an oversized sparse archive member'
[[ "$(wc -l < "${LOG}")" == "${calls_before}" ]] \
  || fail 'oversized bundle refusal touched the service'

# A service account can replace the token after the health checks. Sealing must enforce the
# 64-byte caller budget before copying the replacement into the root workspace.
printf '%064d' 4 > "${ROOT}/var/lib/graphhelm/events.token"
rm -f -- "${SANDBOX}/oversized-token-triggered"
export TEST_TOKEN_OVERSIZED_ON_HEALTH=1
export TEST_TOKEN_OVERSIZED_MARKER="${SANDBOX}/oversized-token-triggered"
set +e
oversized_token_output="$(${RESTORE_SCRIPT} --bundle "${BUNDLE}" --replace-existing 2>&1)"
oversized_token_status=$?
set -e
unset TEST_TOKEN_OVERSIZED_ON_HEALTH TEST_TOKEN_OVERSIZED_MARKER
[[ ${oversized_token_status} -ne 0 \
  && "${oversized_token_output}" == *'unsafe untrusted file'* ]] \
  || fail 'restore copied an oversized replacement token before refusing it'

# Prior-token preservation must use the no-follow seal. The cp wrapper replaces the source only if
# the implementation still copies the service-owned pathname directly.
printf '%064d' 4 > "${ROOT}/var/lib/graphhelm/events.token"
TOKEN_PRIOR_VICTIM="${SANDBOX}/prior-token-victim"
TOKEN_PRIOR_MARKER="${SANDBOX}/prior-token-copy-marker"
printf 'root-readable prior token victim\n' > "${TOKEN_PRIOR_VICTIM}"
rm -f -- "${TOKEN_PRIOR_MARKER}"
export TEST_TOKEN_PRIOR_SYMLINK_ON_CP=1
export TEST_TOKEN_PRIOR_SYMLINK_TARGET="${TOKEN_PRIOR_VICTIM}"
export TEST_TOKEN_PRIOR_SYMLINK_MARKER="${TOKEN_PRIOR_MARKER}"
set +e
prior_token_output="$(${RESTORE_SCRIPT} --bundle "${BUNDLE}" --replace-existing 2>&1)"
prior_token_status=$?
set -e
unset TEST_TOKEN_PRIOR_SYMLINK_ON_CP TEST_TOKEN_PRIOR_SYMLINK_TARGET TEST_TOKEN_PRIOR_SYMLINK_MARKER
[[ ${prior_token_status} -eq 0 ]] || fail 'restore rejected a valid prior token during sealed preservation'
[[ ! -e "${TOKEN_PRIOR_MARKER}" ]] \
  || fail 'restore copied the prior token through an unsealed pathname'
[[ "$(cat "${TOKEN_PRIOR_VICTIM}")" == 'root-readable prior token victim' ]] \
  || fail 'prior token preservation read through an injected symlink'

# A service-owned FIFO can appear immediately after health succeeds. The bounded seal must refuse
# it before the old wc redirection can block while holding the operation lock.
rm -f -- "${ROOT}/var/lib/graphhelm/events.token"
printf '%064d' 4 > "${ROOT}/var/lib/graphhelm/events.token"
rm -f -- "${SANDBOX}/token-fifo-marker"
export TEST_TOKEN_FIFO_ON_HEALTH=1
export TEST_TOKEN_FIFO_MARKER="${SANDBOX}/token-fifo-marker"
set +e
fifo_token_output="$(timeout 8s "${RESTORE_SCRIPT}" --bundle "${BUNDLE}" --replace-existing 2>&1)"
fifo_token_status=$?
set -e
unset TEST_TOKEN_FIFO_ON_HEALTH TEST_TOKEN_FIFO_MARKER
[[ ${fifo_token_status} -ne 124 \
  && "${fifo_token_output}" == *'unsafe untrusted file'* ]] \
  || fail 'restore blocked on a replacement token FIFO before sealing it'
rm -f -- "${ROOT}/var/lib/graphhelm/events.token"
printf '%064d' 4 > "${ROOT}/var/lib/graphhelm/events.token"

# A locally modified unit must be refused before a successful backup bundle is published.
cp -- "${CANONICAL_UNIT}" "${ROOT}/etc/systemd/system/graphhelm.service"
printf '\n# local modification\n' >> "${ROOT}/etc/systemd/system/graphhelm.service"
bundles_before="$(find "${DESTINATION}" -maxdepth 1 -type f -name 'graphhelm-vps-*.tar' | wc -l)"
set +e
noncanonical_output="$(${BACKUP_SCRIPT} --destination "${DESTINATION}" 2>&1)"
noncanonical_status=$?
set -e
[[ ${noncanonical_status} -ne 0 \
  && "${noncanonical_output}" == *'exact approved GraphHelm systemd unit'* ]] \
  || fail 'backup published or accepted a noncanonical systemd unit'
[[ "$(find "${DESTINATION}" -maxdepth 1 -type f -name 'graphhelm-vps-*.tar' | wc -l)" == "${bundles_before}" ]] \
  || fail 'noncanonical unit refusal published a bundle'
[[ "$(cat "${STATE}")" == active ]] || fail 'noncanonical unit refusal left service stopped'
cp -- "${CANONICAL_UNIT}" "${ROOT}/etc/systemd/system/graphhelm.service"

set +e
restore_refusal="$(${RESTORE_SCRIPT} --bundle "${BUNDLE}" 2>&1)"
restore_status=$?
set -e
[[ ${restore_status} -ne 0 ]] || fail 'restore accepted a non-empty repository without opt-in'
[[ "${restore_refusal}" == *'--replace-existing'* ]] || fail 'restore refusal did not name the opt-in'

restore_calls_before="$(wc -l < "${LOG}")"
token_before_restore="$(cat "${ROOT}/var/lib/graphhelm/events.token")"
restore_output="$(${RESTORE_SCRIPT} --bundle "${BUNDLE}" --replace-existing)"
tail -n "+$((restore_calls_before + 1))" "${LOG}" > "${SANDBOX}/restore-calls.log"
[[ "${restore_output}" != *"$(cat "${ROOT}/var/lib/graphhelm/events.token")"* ]] \
  || fail 'restore printed the raw token'
[[ "$(cat "${ROOT}/var/lib/graphhelm/events/journal.jsonl")" == 'restored evidence' ]] \
  || fail 'restore did not use the CLI archive'
[[ "$(cat "${ROOT}/var/lib/graphhelm/events.token")" != "${token_before_restore}" ]] \
  || fail 'restore did not mint a fresh local Runtime token'
find "${ROOT}/var/lib/graphhelm-quarantine" -maxdepth 1 -type d \
  -name 'events.quarantine.*' -print -quit | grep -q . \
  || fail 'restore did not quarantine the old repository'
# CHANGED FROM `actor=root` (#786), deliberately and not incidentally. This line asserted the
# defect: the local restore ran as root, which is what the ticket exists to end. One line proves
# both halves -- only the restore emits `events restore`, and only a dropped privilege emits
# `actor=graphhelm` -- so a separate `runuser` grep would be the weaker duplicate. It would also be
# VACUOUS here: the backup drops privilege earlier in this same run and writes `runuser graphhelm
# env -i` into this same log, so that grep passes whatever the restore does.
grep -Fq 'graphhelm actor=graphhelm events restore --repository' "${LOG}" \
  || fail 'restore did not drop privilege to the graphhelm service user'
# The archive is read from the STAGE, never from the 0700 workspace. A restore still pointed at the
# workspace would pass the actor assertion above and break on a real machine, where the service
# account cannot traverse there -- which is exactly the breakage a bare wrapper produces.
grep -Eq 'graphhelm actor=graphhelm events restore .*--archive [^ ]*graphhelm-restore-stage\.' \
  "${LOG}" || fail 'restore read the archive from outside the service-readable stage'
# The stage held ONLY the archive. The bundle and the sealed token copy stay in the workspace; a
# stage that grew a second file would be handing the service account material this ticket's first
# constraint says must stay out of it.
grep -Eq 'archive-dir [^ ]*graphhelm-restore-stage\.[^ ]* \[events\.archive\]' "${LOG}" \
  || fail 'the staging boundary held something other than exactly the event archive'
grep -Fq 'systemctl daemon-reload' "${LOG}" || fail 'restore did not reload systemd'
grep -Fq 'systemctl start graphhelm.service' "${SANDBOX}/restore-calls.log" \
  || fail 'restore did not start the service'
grep -Fq 'curl --silent --show-error --fail --max-time 2 http://127.0.0.1:8080/health' \
  "${SANDBOX}/restore-calls.log" || fail 'restore did not run the health smoke'
grep -Fq 'curl --silent --show-error --output /dev/null --write-out %{http_code} --max-time 2 http://127.0.0.1:8080/v1/executions/restore-smoke' \
  "${SANDBOX}/restore-calls.log" || fail 'restore did not prove unauthenticated enforcement'
grep -Eq 'curl --header @[^ ]*/authorization\.header' "${SANDBOX}/restore-calls.log" \
  || fail 'restore did not send the authenticated smoke through a sealed header file'
# The negative half, and it is the one that carries the security claim: the header path is only
# safe because no config parser sees the token. Asserting the new spelling alone would stay green
# if someone reinstated `--config` beside it.
if grep -Fq 'curl --config' "${SANDBOX}/restore-calls.log"; then
  fail 'restore still parses a service-owned token through curl config directives'
fi
[[ -s "${ROOT}/var/lib/graphhelm/events.token" ]] \
  || fail 'the restore smoke should have left the service token in place'
grep -Fq 'http://127.0.0.1:8080/does-not-exist' "${SANDBOX}/restore-calls.log" \
  || fail 'restore did not run the authenticated endpoint smoke'

rm -rf -- "${ROOT}/var/lib/graphhelm/events"
mkdir -- "${ROOT}/var/lib/graphhelm/events"
set +e
empty_refusal="$(${RESTORE_SCRIPT} --bundle "${BUNDLE}" 2>&1)"
empty_status=$?
set -e
[[ ${empty_status} -ne 0 && "${empty_refusal}" == *'--replace-existing'* ]] \
  || fail 'restore accepted an existing empty repository without opt-in'
quarantines_before="$(find "${ROOT}/var/lib/graphhelm-quarantine" -maxdepth 1 -type d \
  -name 'events.quarantine.*' | wc -l)"
${RESTORE_SCRIPT} --bundle "${BUNDLE}" --replace-existing >/dev/null
quarantines_after="$(find "${ROOT}/var/lib/graphhelm-quarantine" -maxdepth 1 -type d \
  -name 'events.quarantine.*' | wc -l)"
[[ "${quarantines_after}" -eq $((quarantines_before + 1)) ]] \
  || fail 'explicit replacement did not quarantine the existing empty repository'

printf 'old evidence\n' > "${ROOT}/var/lib/graphhelm/events/journal.jsonl"
printf '%064d' 1 > "${ROOT}/var/lib/graphhelm/events.token"
printf 'prior unit\n' > "${ROOT}/etc/systemd/system/graphhelm.service"
printf 'active\n' > "${STATE}"
export TEST_RESTORE_FAIL=1
set +e
rollback_output="$(${RESTORE_SCRIPT} --bundle "${BUNDLE}" --replace-existing 2>&1)"
rollback_status=$?
set -e
unset TEST_RESTORE_FAIL
[[ ${rollback_status} -ne 0 ]] || fail 'injected restore failure unexpectedly succeeded'
[[ "${rollback_output}" != *"$(cat "${ROOT}/var/lib/graphhelm/events.token")"* ]] \
  || fail 'rollback printed the raw token'
[[ "$(cat "${ROOT}/var/lib/graphhelm/events/journal.jsonl")" == 'old evidence' ]] \
  || fail 'rollback did not restore the prior event repository'
[[ "$(cat "${ROOT}/var/lib/graphhelm/events.token")" == "$(printf '%064d' 1)" ]] \
  || fail 'rollback did not restore the prior token'
[[ "$(cat "${ROOT}/etc/systemd/system/graphhelm.service")" == 'prior unit' ]] \
  || fail 'rollback did not restore the prior systemd unit'
[[ "$(cat "${STATE}")" == active ]] || fail 'rollback did not restore the prior active service'
find "${ROOT}/var/lib/graphhelm" -maxdepth 1 -type d \
  -name 'events.failed-restore.*' -print -quit | grep -q . \
  || fail 'rollback did not preserve the failed restored repository'

# A restarted service may replace the token pathname before smoke fails. Rollback
# must replace that symlink atomically instead of writing through it.
printf 'old evidence\n' > "${ROOT}/var/lib/graphhelm/events/journal.jsonl"
printf '%064d' 1 > "${ROOT}/var/lib/graphhelm/events.token"
printf 'active\n' > "${STATE}"
TOKEN_VICTIM="${SANDBOX}/token-victim"
TOKEN_SWAP_MARKER="${SANDBOX}/token-swap-marker"
printf 'do not overwrite\n' > "${TOKEN_VICTIM}"
rm -f -- "${TOKEN_SWAP_MARKER}"
export TEST_TOKEN_SYMLINK_ON_HEALTH=1
export TEST_TOKEN_SYMLINK_TARGET="${TOKEN_VICTIM}"
export TEST_TOKEN_SWAP_MARKER="${TOKEN_SWAP_MARKER}"
set +e
token_symlink_output="$(${RESTORE_SCRIPT} --bundle "${BUNDLE}" --replace-existing 2>&1)"
token_symlink_status=$?
set -e
unset TEST_TOKEN_SYMLINK_ON_HEALTH TEST_TOKEN_SYMLINK_TARGET TEST_TOKEN_SWAP_MARKER
[[ ${token_symlink_status} -ne 0 ]] || fail 'injected restore smoke failure unexpectedly succeeded'
[[ "${token_symlink_output}" != *"$(printf '%064d' 1)"* ]] \
  || fail 'token symlink rollback printed the prior token'
[[ -e "${TOKEN_SWAP_MARKER}" ]] || fail 'token symlink injection did not run'
[[ "$(cat "${TOKEN_VICTIM}")" == 'do not overwrite' ]] \
  || fail 'restore rollback wrote through the token symlink'
[[ -f "${ROOT}/var/lib/graphhelm/events.token" \
  && ! -L "${ROOT}/var/lib/graphhelm/events.token" ]] \
  || fail 'restore rollback did not atomically replace the token symlink'
[[ "$(cat "${ROOT}/var/lib/graphhelm/events.token")" == "$(printf '%064d' 1)" ]] \
  || fail 'restore rollback did not restore the prior token after symlink injection'
[[ "$(cat "${STATE}")" == active ]] \
  || fail 'token symlink rollback did not restore the prior active service'

rollback_calls_before="$(wc -l < "${LOG}")"
quarantines_before="$(find "${ROOT}/var/lib/graphhelm-quarantine" -maxdepth 1 -type d \
  -name 'events.quarantine.*' | wc -l)"
failed_before="$(find "${ROOT}/var/lib/graphhelm" -maxdepth 1 -type d \
  -name 'events.failed-restore.*' | wc -l)"
export TEST_RESTORE_FAIL=1
export TEST_ROLLBACK_MV_FAIL=1
set +e
${RESTORE_SCRIPT} --bundle "${BUNDLE}" --replace-existing >/dev/null 2>&1
mixed_status=$?
set -e
unset TEST_RESTORE_FAIL TEST_ROLLBACK_MV_FAIL
[[ ${mixed_status} -ne 0 ]] || fail 'injected rollback-operation failure unexpectedly succeeded'
tail -n "+$((rollback_calls_before + 1))" "${LOG}" > "${SANDBOX}/failed-rollback-calls.log"
! grep -Fq 'systemctl start graphhelm.service' "${SANDBOX}/failed-rollback-calls.log" \
  || fail 'incomplete rollback restarted the prior service'
[[ "$(cat "${STATE}")" == inactive ]] || fail 'incomplete rollback did not leave the service stopped'
quarantines_after="$(find "${ROOT}/var/lib/graphhelm-quarantine" -maxdepth 1 -type d \
  -name 'events.quarantine.*' | wc -l)"
failed_after="$(find "${ROOT}/var/lib/graphhelm" -maxdepth 1 -type d \
  -name 'events.failed-restore.*' | wc -l)"
[[ "${quarantines_after}" -eq $((quarantines_before + 1)) \
  && "${failed_after}" -eq $((failed_before + 1)) ]] \
  || fail 'incomplete rollback did not preserve both old and failed repository evidence'

printf 'vps backup/restore test: PASS\n'
