#!/usr/bin/env bash
set -Eeuo pipefail

readonly SERVICE_NAME="graphhelm"
readonly SERVICE_USER="graphhelm"
readonly ROOT_PREFIX="${GRAPHHELM_VPS_ROOT:-}"
readonly BINARY_PATH="${ROOT_PREFIX}/usr/local/bin/graphhelm"
readonly EVENTS_DIR="${ROOT_PREFIX}/var/lib/graphhelm/events"
readonly STATE_DIR="${ROOT_PREFIX}/var/lib/graphhelm"
readonly UNIT_PATH="${ROOT_PREFIX}/etc/systemd/system/${SERVICE_NAME}.service"
readonly DEFAULT_DESTINATION="${ROOT_PREFIX}/var/backups/graphhelm"
readonly LOCK_PATH="${ROOT_PREFIX}/run/graphhelm-operation.lock"
# This constant is a PAIR with `restore-vps.sh`'s. Lowering the restore side alone would have been
# worse than leaving both at 8 GiB: the operator would receive a backup that reports success and
# can never be restored by these scripts, which moves the false confidence from recovery time to
# backup time, where it is discovered even later. Whatever this number becomes, the two move
# together -- a bundle this script is willing to publish must be a bundle its sibling is willing
# to read (Codex on #694, catching exactly that one-sided change).
readonly MAX_EVENT_ARCHIVE_BYTES=268435456
# The local backup CLI builds one JSON archive in memory before writing it. Keep its address space
# finite so an oversized repository cannot consume unbounded RAM before the sealed-file limit can
# reject the result. One GiB leaves room for a normal 256 MiB archive plus the CLI/runtime image;
# this is a resource refusal bound, not a promise that every archive below 256 MiB fits.
readonly MAX_BACKUP_ADDRESS_SPACE_BYTES=1073741824
# A DEADLINE ON THE STOPPED-SERVICE SNAPSHOT (#595 review). The service is stopped before the
# snapshot and restarted only by cleanup, so a backup that blocks -- an independent local CLI
# writer still holding repository.lock keeps `with_repository_read_lock` waiting with no deadline
# of its own -- was an unbounded outage. prlimit bounds bytes, not time; this bounds time. GNU
# timeout requests TERM at the deadline and KILL the same interval later.
readonly BACKUP_TIMEOUT_SECONDS="${GRAPHHELM_BACKUP_TIMEOUT_SECONDS:-600}"

fail() {
  printf 'graphhelm backup: %s\n' "$*" >&2
  exit 1
}

usage() {
  printf 'Usage: %s [--destination DIRECTORY] [--leave-stopped]\n' "$0"
}

validate_destination_path() {
  local current="$1" component_mode
  # Walk every existing component, because checking only the final directory leaves a writable
  # parent able to rename it and substitute a path before tar publishes the bundle. Sticky paths
  # such as /tmp are deliberately refused too: this conservative boundary does not rely on the
  # sticky-bit rules to make a multi-step pathname safe.
  while [[ "${current}" != / ]]; do
    if [[ -L "${current}" ]]; then
      fail 'the destination path component must not be a symlink'
    elif [[ -e "${current}" ]]; then
      [[ -d "${current}" ]] || fail 'the destination path component must be a directory'
      [[ "$(stat -c '%u' "${current}")" == 0 \
        && "$(stat -c '%g' "${current}")" == 0 ]] \
        || fail 'the destination path component must be root-owned'
      component_mode="$(stat -c '%a' "${current}")"
      [[ "${component_mode}" =~ ^[0-7]{3,4}$ ]] \
        || fail 'the destination path component mode is invalid'
      (( (8#${component_mode} & 8#022) == 0 )) \
        || fail 'the destination path component must be non-writable by group and other'
    fi
    current="$(dirname -- "${current}")"
  done
}

prepare_operation_lock() {
  local lock_dir lock_dir_mode
  lock_dir="$(dirname -- "${LOCK_PATH}")"
  if [[ ! -e "${lock_dir}" && ! -L "${lock_dir}" ]]; then
    install -d -m 0755 -- "${lock_dir}"
  fi
  [[ -d "${lock_dir}" && ! -L "${lock_dir}" ]] \
    || fail 'the GraphHelm operation lock directory is unsafe'
  [[ "$(stat -c '%u' "${lock_dir}")" == 0 \
    && "$(stat -c '%g' "${lock_dir}")" == 0 ]] \
    || fail 'the GraphHelm operation lock directory must be owned by root:root'
  lock_dir_mode="$(stat -c '%a' "${lock_dir}")"
  [[ "${lock_dir_mode}" =~ ^[0-7]{3,4}$ ]] \
    || fail 'the GraphHelm operation lock directory mode is invalid'
  (( (8#${lock_dir_mode} & 8#002) == 0 )) \
    || fail 'the GraphHelm operation lock directory is world-writable'
  if [[ ! -e "${LOCK_PATH}" && ! -L "${LOCK_PATH}" ]]; then
    (set -o noclobber; : > "${LOCK_PATH}") 2>/dev/null || true
    chmod 0600 -- "${LOCK_PATH}" 2>/dev/null || true
  fi
  [[ -f "${LOCK_PATH}" && ! -L "${LOCK_PATH}" ]] \
    || fail 'the GraphHelm operation lock file is unsafe'
  [[ "$(stat -c '%u' "${LOCK_PATH}")" == 0 \
    && "$(stat -c '%a' "${LOCK_PATH}")" == 600 ]] \
    || fail 'the GraphHelm operation lock file must be root-owned mode 0600'
}

[[ "$(id -u)" == 0 ]] || fail 'run this script as root'
if [[ -n "${ROOT_PREFIX}" && "${ROOT_PREFIX}" != /* ]]; then
  fail 'GRAPHHELM_VPS_ROOT must be an absolute path'
fi

destination="${DEFAULT_DESTINATION}"
leave_stopped=false
while (($#)); do
  case "$1" in
    --destination)
      (($# >= 2)) || fail '--destination requires a directory'
      destination="$2"
      shift 2
      ;;
    --leave-stopped)
      leave_stopped=true
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *) fail "unknown argument: $1" ;;
  esac
done

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
seal_helper="${SCRIPT_DIR}/seal-vps-file.py"
if [[ ! -r "${seal_helper}" \
  && -r "${ROOT_PREFIX}/usr/local/libexec/graphhelm/seal-vps-file.py" ]]; then
  seal_helper="${ROOT_PREFIX}/usr/local/libexec/graphhelm/seal-vps-file.py"
fi
readonly SCRIPT_DIR seal_helper
[[ -r "${seal_helper}" ]] || fail 'the approved untrusted-file sealing helper is missing'

for command in systemctl runuser install tar sha256sum mktemp stat ln flock realpath readlink chmod cmp python3 timeout; do
  command -v "${command}" >/dev/null 2>&1 || fail "required command is missing: ${command}"
done
prlimit_path="$(command -v prlimit)" || fail 'required command is missing: prlimit'
readonly prlimit_path
[[ -x "${BINARY_PATH}" ]] || fail "GraphHelm CLI is missing at ${BINARY_PATH}"
id -u "${SERVICE_USER}" >/dev/null 2>&1 || fail 'the graphhelm service user is missing'

run_as_service() {
  runuser --user "${SERVICE_USER}" -- env -i \
    HOME="${STATE_DIR}" \
    USER="${SERVICE_USER}" \
    LOGNAME="${SERVICE_USER}" \
    PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
    "$@"
}

run_as_service_bounded() {
  # runuser may reset inherited resource limits while switching users. Apply them after the
  # switch, immediately before the producer, so the CLI actually inherits these bounds.
  timeout --signal=TERM --kill-after="${BACKUP_TIMEOUT_SECONDS}s" "${BACKUP_TIMEOUT_SECONDS}s" \
  runuser --user "${SERVICE_USER}" -- env -i \
      HOME="${STATE_DIR}" \
      USER="${SERVICE_USER}" \
      LOGNAME="${SERVICE_USER}" \
      PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
      "${prlimit_path}" --fsize="${MAX_EVENT_ARCHIVE_BYTES}" \
        --as="${MAX_BACKUP_ADDRESS_SPACE_BYTES}" -- \
        "$@"
}

prepare_operation_lock
if [[ -n "${GRAPHHELM_OPERATION_LOCK_FD:-}" ]]; then
  [[ "${GRAPHHELM_OPERATION_LOCK_FD}" =~ ^[0-9]+$ ]] \
    || fail 'the inherited operation lock descriptor is invalid'
  inherited_lock_path="$(readlink -f -- "/proc/self/fd/${GRAPHHELM_OPERATION_LOCK_FD}" 2>/dev/null || true)"
  [[ "${inherited_lock_path}" == "$(readlink -f -- "${LOCK_PATH}")" ]] \
    || fail 'the inherited operation lock does not match the GraphHelm lock file'
  flock --nonblock "${GRAPHHELM_OPERATION_LOCK_FD}" \
    || fail 'another GraphHelm backup, restore, or upgrade operation is already running'
else
  exec 9<>"${LOCK_PATH}"
  flock --nonblock 9 \
    || fail 'another GraphHelm backup, restore, or upgrade operation is already running'
fi

[[ -d "${EVENTS_DIR}" && ! -L "${EVENTS_DIR}" ]] || fail 'the local event repository is missing or unsafe'
[[ -f "${UNIT_PATH}" && ! -L "${UNIT_PATH}" ]] || fail 'the systemd unit is missing or unsafe'

canonical_events="$(realpath -m -- "${EVENTS_DIR}")"
canonical_destination="$(realpath -m -- "${destination}")"
case "${canonical_destination}" in
  "${canonical_events}"|"${canonical_events}"/*)
    fail 'the backup destination cannot be the event repository or a directory below it'
    ;;
esac
destination="${canonical_destination}"

if [[ -e "${destination}" && ! -d "${destination}" ]] || [[ -L "${destination}" ]]; then
  fail 'the destination must be a real directory, not a file or symlink'
fi
validate_destination_path "${destination}"
install -d -m 0700 -- "${destination}"

workspace_parent="${ROOT_PREFIX}/var/tmp/graphhelm"
if [[ ! -e "${workspace_parent}" && ! -L "${workspace_parent}" ]]; then
  install -d -m 0755 -- "${workspace_parent}"
fi
[[ -d "${workspace_parent}" && ! -L "${workspace_parent}" \
  && "$(stat -c '%u' "${workspace_parent}")" == 0 \
  && "$(stat -c '%a' "${workspace_parent}")" == 755 ]] \
  || fail 'the GraphHelm temporary workspace parent must be a root-owned mode 0755 directory'
workspace="$(mktemp -d "${workspace_parent}/graphhelm-backup.XXXXXX")"
install --directory --owner root --group "${SERVICE_USER}" --mode 0710 "${workspace}"
bundle_temp=''
verification=''
service_was_active=false
service_stopped=false

cleanup() {
  local status=$?
  trap - EXIT
  rm -rf -- "${workspace}"
  [[ -z "${verification}" ]] || rm -rf -- "${verification}"
  [[ -z "${bundle_temp}" ]] || rm -f -- "${bundle_temp}"
  if [[ "${service_was_active}" == true && "${service_stopped}" == true && "${leave_stopped}" != true ]]; then
    if ! systemctl start "${SERVICE_NAME}.service"; then
      printf 'graphhelm backup: failed to restart %s.service; the service remains stopped\n' \
        "${SERVICE_NAME}" >&2
      status=1
    fi
  fi
  exit "${status}"
}
trap cleanup EXIT

if systemctl is-active --quiet "${SERVICE_NAME}.service"; then
  service_was_active=true
fi
systemctl stop "${SERVICE_NAME}.service"
service_stopped=true

receipt="${workspace}/backup-receipt.json"
archive_staging="${workspace}/event-backup"
install --directory --owner "${SERVICE_USER}" --group "${SERVICE_USER}" --mode 0700 \
  "${archive_staging}"
backup_status=0
run_as_service_bounded "${BINARY_PATH}" events backup --repository "${EVENTS_DIR}" \
  --output "${archive_staging}/events.archive" > "${receipt}" || backup_status=$?
if [[ "${backup_status}" -eq 124 || "${backup_status}" -eq 137 ]]; then
  fail "the stopped-service snapshot did not finish within ${BACKUP_TIMEOUT_SECONDS}s (a local writer may hold repository.lock); the service is restarted by cleanup"
fi
[[ "${backup_status}" -eq 0 ]] || fail "the CLI backup exited ${backup_status}"
grep -Eq '"ok"[[:space:]]*:[[:space:]]*true' "${receipt}" \
  || fail 'the CLI did not report a successful local backup'
grep -Eq '"command"[[:space:]]*:[[:space:]]*"events\.backup"' "${receipt}" \
  || fail 'the CLI backup receipt named an unexpected command'
python3 "${seal_helper}" --source "${archive_staging}/events.archive" \
  --destination "${workspace}/events.archive" --mode 0600 \
  --max-bytes "${MAX_EVENT_ARCHIVE_BYTES}" \
  || fail 'the CLI event archive crossed an unsafe untrusted file boundary'
rm -rf -- "${archive_staging}"
[[ -s "${workspace}/events.archive" ]] || fail 'the CLI produced an empty event archive'
event_archive_bytes="$(stat -c '%s' "${workspace}/events.archive")"
[[ "${event_archive_bytes}" =~ ^[0-9]+$ \
  && "${event_archive_bytes}" -le "${MAX_EVENT_ARCHIVE_BYTES}" ]] \
  || fail "the event archive exceeds the restore limit of $((MAX_EVENT_ARCHIVE_BYTES / 1048576)) MiB"

install -m 0644 -- "${UNIT_PATH}" "${workspace}/graphhelm.service"
cat > "${workspace}/canonical.graphhelm.service" <<'UNIT'
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
cmp -s "${workspace}/canonical.graphhelm.service" "${workspace}/graphhelm.service" \
  || fail 'the live systemd unit is not the exact approved GraphHelm systemd unit'
(
  cd -- "${workspace}"
  sha256sum events.archive graphhelm.service > manifest.sha256
  sha256sum --check --strict manifest.sha256 >/dev/null
)

timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
final_path="${destination}/graphhelm-vps-${timestamp}-$$.tar"
[[ ! -e "${final_path}" ]] || fail 'the generated destination already exists'
bundle_temp="$(mktemp "${destination}/.graphhelm-vps.XXXXXX")"
tar -C "${workspace}" -cf "${bundle_temp}" \
  events.archive graphhelm.service manifest.sha256
chmod 0600 "${bundle_temp}"
[[ "$(stat -c '%a' "${bundle_temp}")" == 600 ]] || fail 'the bundle mode is not 0600'

verification="$(mktemp -d "${workspace_parent}/graphhelm-backup-check.XXXXXX")"
tar -xf "${bundle_temp}" -C "${verification}"
(
  cd -- "${verification}"
  sha256sum --check --strict manifest.sha256 >/dev/null
)
rm -rf -- "${verification}"
verification=''

ln -- "${bundle_temp}" "${final_path}"
rm -f -- "${bundle_temp}"
bundle_temp=''

if [[ "${service_was_active}" == true && "${leave_stopped}" != true ]]; then
  systemctl start "${SERVICE_NAME}.service"
  service_stopped=false
fi

printf 'GraphHelm backup created: %s\n' "${final_path}"
