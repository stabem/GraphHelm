#!/usr/bin/env bash
set -Eeuo pipefail

readonly SERVICE_NAME="graphhelm"
readonly SERVICE_USER="graphhelm"
readonly SERVICE_GROUP="graphhelm"
readonly ROOT_PREFIX="${GRAPHHELM_VPS_ROOT:-}"
readonly BINARY_PATH="${ROOT_PREFIX}/usr/local/bin/graphhelm"
readonly STATE_DIR="${ROOT_PREFIX}/var/lib/graphhelm"
readonly EVENTS_DIR="${STATE_DIR}/events"
readonly TOKEN_PATH="${STATE_DIR}/events.token"
# THE QUARANTINE LIVES OUTSIDE THE SERVICE-OWNED STATE DIRECTORY (#595 review): under
# ${STATE_DIR} the graphhelm account owned the moved repository and could alter, rename or
# delete it while the restored service was being tested, so a later rollback could report a
# clean rollback onto a repository that was not the one quarantined. A root-owned 0700 parent
# the service account cannot traverse closes that; the rollback checks it is still so.
readonly QUARANTINE_DIR="${ROOT_PREFIX}/var/lib/graphhelm-quarantine"
readonly UNIT_PATH="${ROOT_PREFIX}/etc/systemd/system/${SERVICE_NAME}.service"
readonly LOCK_PATH="${ROOT_PREFIX}/run/graphhelm-operation.lock"
readonly MAX_BUNDLE_BYTES=10737418240
# 256 MiB, not 8 GiB. The local restore decoder is whole-file: `events/restore.rs` does
# `read_to_string` and then `serde_json::from_str` into a `Value`, so peak residency is the
# archive text PLUS a parsed tree several times its size. At 8 GiB an archive that passes
# every preflight check in this script cannot be decoded on any ordinary VPS, and the
# failure lands in the middle of a disaster recovery -- accepting input we cannot process
# is failing toward false confidence (M, adjudicating Codex on #595).
#
# The multiplier is the decoder's documented shape, NOT a figure I measured, and this
# runbook documents no minimum RAM to derive a limit from. Revisit when it does, or drop
# this bound entirely once the decoder streams.
readonly MAX_EVENT_ARCHIVE_BYTES=268435456
readonly MAX_UNIT_BYTES=4096
readonly MAX_MANIFEST_BYTES=1024
readonly MAX_TOKEN_BYTES=64

fail() {
  printf 'graphhelm restore: %s\n' "$*" >&2
  exit 1
}

usage() {
  printf 'Usage: %s --bundle FILE [--replace-existing]\n' "$0"
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

bundle=''
replace_existing=false
while (($#)); do
  case "$1" in
    --bundle)
      (($# >= 2)) || fail '--bundle requires a file'
      bundle="$2"
      shift 2
      ;;
    --replace-existing)
      replace_existing=true
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *) fail "unknown argument: $1" ;;
  esac
done
[[ -n "${bundle}" ]] || fail '--bundle is required'
[[ -f "${bundle}" && ! -L "${bundle}" ]] || fail 'the bundle is missing or unsafe'
exec 8<"${bundle}" || fail 'the bundle could not be opened safely'
readonly BUNDLE_FD_PATH="/proc/self/fd/8"
[[ -f "${BUNDLE_FD_PATH}" ]] || fail 'the opened bundle is not a regular file'

# The same sealing helper `upgrade-vps.sh` already resolves, resolved the same way. Its absence
# from this script was the defect: one script sealed a service-owned file before reading it and
# its sibling read it directly, and nothing recorded that the two had diverged.
#
# `SCRIPT_DIR` is defined here rather than reused: this script never had one, and under `set -u`
# an undefined expansion aborts the run at the first reference.
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
readonly SCRIPT_DIR
seal_helper="${SCRIPT_DIR}/seal-vps-file.py"
if [[ ! -r "${seal_helper}" \
  && -r "${ROOT_PREFIX}/usr/local/libexec/graphhelm/seal-vps-file.py" ]]; then
  seal_helper="${ROOT_PREFIX}/usr/local/libexec/graphhelm/seal-vps-file.py"
fi
readonly seal_helper
[[ -r "${seal_helper}" ]] || fail 'the approved untrusted-file sealing helper is missing'

for command in systemctl tar sha256sum mktemp stat curl flock cmp readlink chmod python3 \
  runuser install; do
  command -v "${command}" >/dev/null 2>&1 || fail "required command is missing: ${command}"
done
[[ -x "${BINARY_PATH}" ]] || fail "GraphHelm CLI is missing at ${BINARY_PATH}"
if [[ -z "${ROOT_PREFIX}" ]]; then
  [[ "$(stat -Lc '%u' "${BUNDLE_FD_PATH}")" == 0 ]] || fail 'the bundle must be owned by root'
fi
[[ "$(stat -Lc '%a' "${BUNDLE_FD_PATH}")" == 600 ]] || fail 'the bundle must have mode 0600'
bundle_bytes="$(stat -Lc '%s' "${BUNDLE_FD_PATH}")"
[[ "${bundle_bytes}" =~ ^[0-9]+$ && "${bundle_bytes}" -le "${MAX_BUNDLE_BYTES}" ]] \
  || fail 'the bundle exceeds the 10 GiB safety limit'

workspace_parent="${ROOT_PREFIX}/var/tmp/graphhelm"
if [[ ! -e "${workspace_parent}" && ! -L "${workspace_parent}" ]]; then
  install -d -m 0755 -- "${workspace_parent}"
fi
[[ -d "${workspace_parent}" && ! -L "${workspace_parent}" \
  && "$(stat -c '%u' "${workspace_parent}")" == 0 \
  && "$(stat -c '%a' "${workspace_parent}")" == 755 ]] \
  || fail 'the GraphHelm temporary workspace parent must be a root-owned mode 0755 directory'
workspace="$(mktemp -d "${workspace_parent}/graphhelm-restore.XXXXXX")"
# The stage the service account reads the archive FROM, BESIDE the workspace and never inside it.
#
# `mktemp -d` makes the workspace 0700 root-owned, so a bare privilege drop does not narrow this
# script -- it BREAKS the restore, which is worse than the exposure it closes (#786). Widening the
# workspace is not the alternative: it holds the bundle and the sealed token copy, and loosening it
# to let the service read the archive would expose those. So the boundary is a SEPARATE directory
# holding exactly one file.
#
# Beside works only because `workspace_parent` is 0755 root-owned, asserted directly above: the
# service account can traverse it. Inside cannot, at any mode on the stage itself.
#
# Shape taken from `upgrade-vps.sh:457-467`, which already stages an archive for a service-account
# `events restore`: a 0755 directory holding a 0400 file OWNED by the service account. Deliberately
# not the 0750 root:graphhelm + 0640 group-read shape -- 0400 service-owned is tighter (one account,
# read-only, no group), and a third shape across two sibling scripts is the divergence this ticket
# is a second instance of.
stage="$(mktemp -d "${workspace_parent}/graphhelm-restore-stage.XXXXXX")"
chmod 0755 -- "${stage}"
stage_archive="${stage}/events.archive"
transaction_started=false
transaction_complete=false
service_was_active=false
repository_moved=false
restore_attempted=false
config_mutated=false
prior_unit_exists=false
prior_token_exists=false
quarantine=''
failed_repository=''

# Drop to the service account for the one call that must not run as root, in the shape
# `upgrade-vps.sh:148` already uses. Copied rather than referenced because each of these scripts
# is self-contained -- and copied rather than re-invented because the last divergence between
# these two siblings is what this script's own comment at the sealing helper records: "one script
# sealed a service-owned file before reading it and its sibling read it directly, and nothing
# recorded that the two had diverged."
#
# `env -i` is part of the boundary, not tidiness: the restore must not inherit root's environment.
run_as_service() {
  runuser --user "${SERVICE_USER}" -- env -i \
    HOME="${STATE_DIR}" \
    USER="${SERVICE_USER}" \
    LOGNAME="${SERVICE_USER}" \
    PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
    "$@"
}

atomic_install_token() {
  local source_token="$1"
  local temporary
  temporary="$(mktemp "${STATE_DIR}/.events.token.XXXXXX")" || return 1
  if ! install -m 0600 -- "${source_token}" "${temporary}"; then
    rm -f -- "${temporary}"
    return 1
  fi
  if [[ -z "${ROOT_PREFIX}" ]] \
    && ! chown "${SERVICE_USER}:${SERVICE_GROUP}" "${temporary}"; then
    rm -f -- "${temporary}"
    return 1
  fi
  if ! mv --force -- "${temporary}" "${TOKEN_PATH}"; then
    rm -f -- "${temporary}"
    return 1
  fi
}

cleanup_and_rollback() {
  local status=$?
  local rollback_failed=false
  trap - EXIT
  if [[ "${status}" -ne 0 && "${transaction_started}" == true && "${transaction_complete}" != true ]]; then
    set +e
    systemctl stop "${SERVICE_NAME}.service" >/dev/null 2>&1 || rollback_failed=true
    if [[ "${restore_attempted}" == true && -e "${EVENTS_DIR}" ]]; then
      failed_repository="${STATE_DIR}/events.failed-restore.$(date -u +%Y%m%dT%H%M%SZ).$$"
      mv -- "${EVENTS_DIR}" "${failed_repository}" || rollback_failed=true
    fi
    if [[ "${repository_moved}" == true ]]; then
      # The quarantine parent must still be root-owned and closed to the service account, or the
      # repository being rolled back is not provably the one that was quarantined.
      if [[ -z "${ROOT_PREFIX}" ]] && [[ "$(stat -c '%u %a' -- "${QUARANTINE_DIR}" 2>/dev/null)" != '0 700' ]]; then
        rollback_failed=true
      fi
      mv -- "${quarantine}" "${EVENTS_DIR}" || rollback_failed=true
    fi
    if [[ "${config_mutated}" == true ]]; then
      if [[ "${prior_unit_exists}" == true ]]; then
        cp --archive --force -- "${workspace}/prior.graphhelm.service" "${UNIT_PATH}" \
          || rollback_failed=true
      else
        rm -f -- "${UNIT_PATH}" || rollback_failed=true
      fi
      if [[ "${prior_token_exists}" == true ]]; then
        atomic_install_token "${workspace}/prior.events.token" \
          || rollback_failed=true
      else
        rm -f -- "${TOKEN_PATH}" || rollback_failed=true
      fi
      systemctl daemon-reload >/dev/null 2>&1 || rollback_failed=true
    fi
    if [[ "${service_was_active}" == true && "${rollback_failed}" == false ]]; then
      systemctl start "${SERVICE_NAME}.service" >/dev/null 2>&1 || rollback_failed=true
    fi
    set -e
    [[ -z "${failed_repository}" ]] \
      || printf 'graphhelm restore: failed restored repository preserved at %s\n' \
        "${failed_repository}" >&2
    if [[ "${rollback_failed}" == true ]]; then
      printf 'graphhelm restore: rollback was incomplete; the service was left fail-closed\n' >&2
      status=1
    fi
  fi
  # The stage goes with the workspace. Both are created before this trap is installed, so
  # neither expansion can be unset here under `set -u`. The stage holds a service-owned file
  # inside a root-owned directory, which root clears without loosening anything.
  rm -rf -- "${workspace}" "${stage}"
  exit "${status}"
}
trap cleanup_and_rollback EXIT

# Reject extra names, duplicates, directories, links, and special files before extraction.
tar -tf "${BUNDLE_FD_PATH}" > "${workspace}/members.actual"
printf '%s\n' events.archive graphhelm.service manifest.sha256 \
  | sort > "${workspace}/members.expected"
sort "${workspace}/members.actual" > "${workspace}/members.sorted"
cmp -s "${workspace}/members.expected" "${workspace}/members.sorted" \
  || fail 'the bundle member list is not the expected closed layout'
[[ "$(wc -l < "${workspace}/members.actual" | tr -d ' ')" == 3 ]] \
  || fail 'the bundle contains duplicate members'
while read -r mode _owner size _date_value _time_value name extra; do
  [[ "${mode:0:1}" == '-' && "${size}" =~ ^[0-9]+$ && -n "${name}" && -z "${extra:-}" ]] \
    || fail 'the bundle contains a link, special file, or malformed member'
  case "${name}" in
    events.archive)
      [[ "${size}" -le "${MAX_EVENT_ARCHIVE_BYTES}" ]] \
        || fail "the event archive exceeds the $((MAX_EVENT_ARCHIVE_BYTES / 1048576)) MiB safety limit"
      ;;
    graphhelm.service)
      [[ "${size}" -le "${MAX_UNIT_BYTES}" ]] || fail 'the bundled unit exceeds the 4 KiB safety limit'
      ;;
    manifest.sha256)
      [[ "${size}" -le "${MAX_MANIFEST_BYTES}" ]] || fail 'the manifest exceeds the 1 KiB safety limit'
      ;;
    *) fail 'the bundle member list is not the expected closed layout' ;;
  esac
done < <(tar --numeric-owner -tvf "${BUNDLE_FD_PATH}")

tar --extract --file "${BUNDLE_FD_PATH}" --directory "${workspace}" \
  --no-same-owner --no-same-permissions
for member in events.archive graphhelm.service manifest.sha256; do
  [[ -f "${workspace}/${member}" && ! -L "${workspace}/${member}" ]] \
    || fail "the extracted bundle member is unsafe: ${member}"
done
[[ "$(wc -l < "${workspace}/manifest.sha256" | tr -d ' ')" == 2 ]] \
  || fail 'the checksum manifest has an unexpected entry count'
while read -r digest name extra; do
  [[ "${digest}" =~ ^[0-9a-f]{64}$ && -n "${name}" && -z "${extra:-}" ]] \
    || fail 'the checksum manifest contains a malformed entry'
  name="${name#\*}"
  printf '%s\n' "${name}"
done < "${workspace}/manifest.sha256" | sort > "${workspace}/manifest.names"
printf '%s\n' events.archive graphhelm.service \
  | sort > "${workspace}/manifest.expected"
cmp -s "${workspace}/manifest.expected" "${workspace}/manifest.names" \
  || fail 'the checksum manifest names files outside the bundle'
(
  cd -- "${workspace}"
  sha256sum --check --strict manifest.sha256 >/dev/null
)
[[ -s "${workspace}/events.archive" ]] || fail 'the event archive is empty'
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
  || fail 'the bundled unit is not the exact approved GraphHelm systemd unit'

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

if [[ -L "${EVENTS_DIR}" ]]; then
  fail 'the existing event repository is a symlink'
fi
repository_exists=false
if [[ -e "${EVENTS_DIR}" ]]; then
  repository_exists=true
  [[ -d "${EVENTS_DIR}" ]] || fail 'the existing event repository is not a directory'
fi
if [[ "${repository_exists}" == true && "${replace_existing}" != true ]]; then
  fail 'the event repository already exists; pass --replace-existing to quarantine and replace it'
fi

if systemctl is-active --quiet "${SERVICE_NAME}.service"; then
  service_was_active=true
fi
if [[ -f "${UNIT_PATH}" && ! -L "${UNIT_PATH}" ]]; then
  cp --archive -- "${UNIT_PATH}" "${workspace}/prior.graphhelm.service"
  prior_unit_exists=true
elif [[ -e "${UNIT_PATH}" || -L "${UNIT_PATH}" ]]; then
  fail 'the existing systemd unit is unsafe'
fi
if [[ -f "${TOKEN_PATH}" && ! -L "${TOKEN_PATH}" ]]; then
  python3 "${seal_helper}" --source "${TOKEN_PATH}" \
    --destination "${workspace}/prior.events.token" --mode 0600 \
    --max-bytes "${MAX_TOKEN_BYTES}" \
    || fail 'the existing Runtime token crossed an unsafe untrusted file boundary'
  prior_token_exists=true
elif [[ -e "${TOKEN_PATH}" || -L "${TOKEN_PATH}" ]]; then
  fail 'the existing token is unsafe'
fi
transaction_started=true
systemctl stop "${SERVICE_NAME}.service"

if [[ "${repository_exists}" == true ]]; then
  timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
  install -d -m 0700 -- "${QUARANTINE_DIR}"
  if [[ -z "${ROOT_PREFIX}" ]]; then
    chown root:root "${QUARANTINE_DIR}"
  fi
  quarantine="${QUARANTINE_DIR}/events.quarantine.${timestamp}.$$"
  [[ ! -e "${quarantine}" ]] || fail 'the quarantine destination already exists'
  mv -- "${EVENTS_DIR}" "${quarantine}"
  repository_moved=true
fi

install -d -m 0700 -- "${STATE_DIR}"
config_mutated=true
install -m 0644 -- "${workspace}/graphhelm.service" "${UNIT_PATH}"
rm -f -- "${TOKEN_PATH}"
if [[ -z "${ROOT_PREFIX}" ]]; then
  chown root:root "${UNIT_PATH}"
  chown "${SERVICE_USER}:${SERVICE_GROUP}" "${STATE_DIR}"
fi
systemctl daemon-reload

receipt="${workspace}/restore-receipt.json"
restore_attempted=true
# Only the ARCHIVE crosses into the stage. The bundle, the sealed token copy and the receipt stay
# in the 0700 workspace, which is the constraint that made a separate directory necessary.
#
# The archive is read whole and nothing is written beside it -- `events/restore.rs:99` is a
# `read_to_string`, which is also what MAX_EVENT_ARCHIVE_BYTES above records about the decoder. So
# 0400 is enough and the stage never needs to be writable by the service account.
install --owner "${SERVICE_USER}" --group "${SERVICE_GROUP}" --mode 0400 -- \
  "${workspace}/events.archive" "${stage_archive}"
# `> "${receipt}"` is redirected by THIS shell, as root, before `runuser` execs -- the service
# account never needs to write into the workspace.
#
# There is no root-privileged path left through this call: the same call site serves both, so a
# boundary that stops working produces an unreadable archive and a non-zero exit that the `if !`
# below already turns into a rollback. It cannot silently fall back to root, because there is no
# root branch to fall back TO.
if ! run_as_service "${BINARY_PATH}" events restore --repository "${EVENTS_DIR}" \
  --archive "${stage_archive}" > "${receipt}"; then
  fail 'the CLI local restore failed; rollback will restore the prior installation'
fi
grep -Eq '"ok"[[:space:]]*:[[:space:]]*true' "${receipt}" \
  || fail 'the CLI did not report a successful local restore'
grep -Eq '"command"[[:space:]]*:[[:space:]]*"events\.restore"' "${receipt}" \
  || fail 'the CLI restore receipt named an unexpected command'
if [[ -z "${ROOT_PREFIX}" ]]; then
  chown -R "${SERVICE_USER}:${SERVICE_GROUP}" "${EVENTS_DIR}"
fi

systemctl start "${SERVICE_NAME}.service"
health_ready=false
for _ in $(seq 1 30); do
  if systemctl is-active --quiet "${SERVICE_NAME}.service" \
    && [[ -f "${TOKEN_PATH}" && ! -L "${TOKEN_PATH}" ]] \
    && curl --silent --show-error --fail --max-time 2 \
      http://127.0.0.1:8080/health </dev/null >/dev/null; then
    health_ready=true
    break
  fi
  sleep 1
done
if [[ "${health_ready}" != true ]]; then
  systemctl stop "${SERVICE_NAME}.service" || true
  fail 'the restored service did not become healthy; rollback will restore the prior installation'
fi

# SEAL, then read ONCE. The service-owned token can be replaced after the health check, so no
# root-side length, shape, or authentication read may touch its pathname. Sealing copies it once
# through a validated descriptor; all later checks use the root-controlled copy.
sealed_token="${workspace}/sealed.events.token"
python3 "${seal_helper}" --source "${TOKEN_PATH}" \
  --destination "${sealed_token}" --mode 0600 --max-bytes 64 \
  || fail 'the restored token crossed an unsafe untrusted file boundary'
[[ "$(wc -c < "${sealed_token}" | tr -d ' ')" == 64 ]] \
  || fail 'the sealed copy of the restored token is not a valid local token'
grep -Eq '^[0-9a-f]{64}$' "${sealed_token}" \
  || fail 'the sealed copy of the restored token is not a valid local token'

if ! unauth_status="$(
    curl --silent --show-error --output /dev/null --write-out '%{http_code}' --max-time 2 \
      http://127.0.0.1:8080/v1/executions/restore-smoke </dev/null
  )"; then
  systemctl stop "${SERVICE_NAME}.service" || true
  fail 'the unauthenticated Runtime smoke could not complete; rollback will restore the prior installation'
fi
if [[ "${unauth_status}" != 401 ]]; then
  systemctl stop "${SERVICE_NAME}.service" || true
  fail 'the Runtime did not enforce authentication; rollback will restore the prior installation'
fi

# `curl --header @file` never parses service-owned bytes as configuration directives. The
# root-controlled sealed copy is the only token input after the boundary above.
auth_header="${workspace}/authorization.header"
token="$(<"${sealed_token}")"
printf 'Authorization: Bearer %s\n' "${token}" > "${auth_header}"
unset token
chmod 0600 -- "${auth_header}"
if ! auth_status="$(
    curl --header "@${auth_header}" --silent --show-error --output /dev/null \
      --write-out '%{http_code}' --max-time 2 \
      http://127.0.0.1:8080/does-not-exist </dev/null
  )"; then
  unset token
  systemctl stop "${SERVICE_NAME}.service" || true
  fail 'the authenticated Runtime smoke could not complete and the service was stopped'
fi
unset token
if [[ "${auth_status}" != 404 ]]; then
  systemctl stop "${SERVICE_NAME}.service" || true
  fail 'the authenticated Runtime smoke failed; rollback will restore the prior installation'
fi

transaction_complete=true
printf 'GraphHelm restore completed and passed health/auth smoke.\n'
[[ -z "${quarantine}" ]] || printf 'Previous event repository: %s\n' "${quarantine}"
