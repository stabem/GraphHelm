#!/usr/bin/env bash
set -Eeuo pipefail

readonly SERVICE_NAME="graphhelm"
readonly SERVICE_USER="graphhelm"
readonly BUILD_USER="graphhelm-build"
readonly BUILD_GROUP="graphhelm-build"
readonly ROOT_PREFIX="${GRAPHHELM_VPS_ROOT:-}"
readonly BINARY_PATH="${ROOT_PREFIX}/usr/local/bin/graphhelm"
readonly STATE_DIR="${ROOT_PREFIX}/var/lib/graphhelm"
readonly EVENTS_DIR="${ROOT_PREFIX}/var/lib/graphhelm/events"
readonly TOKEN_PATH="${ROOT_PREFIX}/var/lib/graphhelm/events.token"
readonly UNIT_PATH="${ROOT_PREFIX}/etc/systemd/system/${SERVICE_NAME}.service"
readonly LOCK_PATH="${ROOT_PREFIX}/run/graphhelm-operation.lock"
readonly RUST_VERSION="1.97.1"
readonly INSTALL_CARGO_HOME="${ROOT_PREFIX}/opt/graphhelm-cargo"
readonly INSTALL_RUSTUP_HOME="${ROOT_PREFIX}/opt/graphhelm-rustup"
readonly BUILD_CACHE_DIR="${ROOT_PREFIX}/var/cache/graphhelm-build"
readonly BUILD_TARGET_DIR="${BUILD_CACHE_DIR}/target"
readonly RECOVERY_BASE="${ROOT_PREFIX}/var/backups/graphhelm-upgrade"

# THE ACCOUNT THAT VERIFIES A CANDIDATE MUST NOT OWN WHAT IT COULD DAMAGE. The isolated
# repository below is built 0700 and its archive 0400, which is careful work -- and every one of
# those bits restricts OTHER accounts. None of them restricts `graphhelm`, which owns the live
# store, so running an unverified candidate as `graphhelm` is isolation by argument rather than by
# permission. It does not take malice: a candidate with an ordinary bug -- a stale default path, a
# constant not updated -- writes into the live store with full rights.
#
# WORSE THAN CORRUPTION, IT IS CORRUPTION THE ROLLBACK CANNOT SEE. The candidate runs while
# `binary_swapped` is still false, and the rollback at the end only restores the backup when that
# flag is true. So the damage lands inside the one window where the operator is told the upgrade
# aborted safely, with the recovery bundle sitting unused beside it.
#
# WHY `nobody` AND NOT A DEDICATED ACCOUNT, chosen rather than left implicit: `nobody` exists on
# every supported Ubuntu, owns nothing in this installation, and needs no creation step -- and a
# new system account would have to be created idempotently by `install/install.sh`, which this
# change does not touch and which would widen a security fix into an installer change. The
# residual is that `nobody` is shared with other daemons, so a process already running as `nobody`
# could reach the isolated directory during the seconds it exists; that is strictly better than
# the present state, where the verifier owns the live store outright. A dedicated
# `graphhelm-verify` account is the stronger form and belongs with the installer that can create
# it.
readonly VERIFY_USER="nobody"
# 4 KiB for a 64-character hexadecimal token: large enough that no legitimate token is refused,
# small enough that a grown token file cannot fill anything. 512 MiB for the release binary,
# measured against a release build that is two orders of magnitude smaller.
readonly MAX_TOKEN_BYTES=4096
readonly MAX_BINARY_BYTES=536870912
# THE SAME CEILING `backup-vps.sh:19` ALREADY STATES, repeated here rather than shared because
# these two scripts have no common file to hold it; the duplication is named so a later divergence
# reads as a defect instead of as an intention.
readonly MAX_EVENT_ARCHIVE_BYTES=268435456
# Candidate verification is untrusted code. GNU timeout requests TERM after this interval, then
# allows the same interval as a kill grace before cleanup restores the old service. The nominal
# bound is therefore 10 seconds (two 5-second phases), excluding operating-system scheduling.
readonly VERIFY_TIMEOUT_SECONDS=5
# RESOURCE LIMITS ON THE UNTRUSTED VERIFIER (#595 review): `timeout` ends the direct child after
# the damage; these bound the damage. Address space, file size and process count are the three
# axes a hostile or buggy candidate can spend before TERM arrives, and --nproc reaches the forked
# grandchildren that `timeout` does not signal. The same fsize/as pair bounds the event captures
# this script takes, so the post-smoke and rollback captures run under the limits backup-vps.sh
# already applies to the same verb.
readonly MAX_VERIFY_ADDRESS_SPACE_BYTES=1073741824
readonly MAX_VERIFY_FILE_BYTES=268435456
readonly MAX_VERIFY_PROCESSES=64
readonly MAX_CAPTURE_ADDRESS_SPACE_BYTES=1073741824

fail() {
  printf 'graphhelm upgrade: %s\n' "$*" >&2
  exit 1
}

usage() {
  printf 'Usage: %s [--source GRAPHHELM_CHECKOUT]\n' "$0"
}

[[ "$(id -u)" == 0 ]] || fail 'run this script as root'
if [[ -n "${ROOT_PREFIX}" && "${ROOT_PREFIX}" != /* ]]; then
  fail 'GRAPHHELM_VPS_ROOT must be an absolute path'
fi
[[ -r "${ROOT_PREFIX}/etc/os-release" ]] || fail 'Ubuntu is required'
# shellcheck disable=SC1090,SC1091
source "${ROOT_PREFIX}/etc/os-release"
[[ "${ID:-}" == ubuntu ]] || fail "Ubuntu is required; detected ${ID:-unknown}"
command -v systemctl >/dev/null 2>&1 || fail 'systemd is required'

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
readonly SCRIPT_DIR
backup_script="${SCRIPT_DIR}/backup-vps.sh"
restore_script="${SCRIPT_DIR}/restore-vps.sh"
seal_helper="${SCRIPT_DIR}/seal-vps-file.py"
if [[ ! -x "${backup_script}" \
  && -x "${ROOT_PREFIX}/usr/local/sbin/graphhelm-backup-vps" ]]; then
  backup_script="${ROOT_PREFIX}/usr/local/sbin/graphhelm-backup-vps"
fi
if [[ ! -x "${restore_script}" \
  && -x "${ROOT_PREFIX}/usr/local/sbin/graphhelm-restore-vps" ]]; then
  restore_script="${ROOT_PREFIX}/usr/local/sbin/graphhelm-restore-vps"
fi
if [[ ! -r "${seal_helper}" \
  && -r "${ROOT_PREFIX}/usr/local/libexec/graphhelm/seal-vps-file.py" ]]; then
  seal_helper="${ROOT_PREFIX}/usr/local/libexec/graphhelm/seal-vps-file.py"
fi
readonly backup_script restore_script seal_helper
[[ -x "${backup_script}" ]] || fail 'the approved GraphHelm backup helper is missing'
[[ -x "${restore_script}" ]] || fail 'the approved GraphHelm restore helper is missing'
[[ -r "${seal_helper}" ]] || fail 'the approved untrusted-file sealing helper is missing'
default_source="$(cd -- "${SCRIPT_DIR}/.." && pwd -P)"
source_dir="${default_source}"
while (($#)); do
  case "$1" in
    --source)
      (($# >= 2)) || fail '--source requires a directory'
      source_dir="$2"
      shift 2
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *) fail "unknown argument: $1" ;;
  esac
done

for command in systemctl runuser install cp chown chmod mv sha256sum mktemp od \
  tr grep tar curl flock readlink realpath awk find date seq sleep stat timeout prlimit; do
  command -v "${command}" >/dev/null 2>&1 || fail "required command is missing: ${command}"
done
command -v python3 >/dev/null 2>&1 || fail 'required command is missing: python3'
[[ -d "${source_dir}" && ! -L "${source_dir}" ]] \
  || fail 'the GraphHelm source checkout is missing or unsafe'
source_dir="$(realpath -- "${source_dir}")"
[[ -f "${source_dir}/Cargo.lock" && -f "${source_dir}/Cargo.toml" \
  && -f "${source_dir}/apps/cli/Cargo.toml" ]] \
  || fail 'the source directory is not a GraphHelm checkout'
[[ -x "${BINARY_PATH}" ]] || fail "installed GraphHelm CLI is missing at ${BINARY_PATH}"
[[ -x "${INSTALL_CARGO_HOME}/bin/rustup" ]] \
  || fail 'the installed graphhelm-build Rust toolchain is missing; repair the installation first'
id -u "${BUILD_USER}" >/dev/null 2>&1 || fail 'the graphhelm-build user is missing'
id -u "${SERVICE_USER}" >/dev/null 2>&1 || fail 'the graphhelm service user is missing'

for build_root in "${INSTALL_CARGO_HOME}" "${INSTALL_RUSTUP_HOME}"; do
  [[ ! -L "${build_root}" ]] || fail "the build path is a symlink: ${build_root}"
  install --directory --owner "${BUILD_USER}" --group "${BUILD_GROUP}" --mode 0755 \
    "${build_root}"
done
[[ ! -L "${BUILD_CACHE_DIR}" ]] || fail 'the build cache directory is a symlink'
install --directory --owner root --group root --mode 0755 "${BUILD_CACHE_DIR}"
chown root:root "${BUILD_CACHE_DIR}"
chmod 0755 "${BUILD_CACHE_DIR}"
[[ ! -L "${BUILD_TARGET_DIR}" ]] || fail 'the build target directory is a symlink'
if [[ -e "${BUILD_TARGET_DIR}" && ! -d "${BUILD_TARGET_DIR}" ]]; then
  fail 'the build target path is not a directory'
fi
install --directory --owner "${BUILD_USER}" --group "${BUILD_GROUP}" --mode 0755 \
  "${BUILD_TARGET_DIR}"
workspace_parent="${ROOT_PREFIX}/var/tmp/graphhelm"
if [[ ! -e "${workspace_parent}" && ! -L "${workspace_parent}" ]]; then
  install -d -m 0755 -- "${workspace_parent}"
fi
[[ -d "${workspace_parent}" && ! -L "${workspace_parent}" \
  && "$(stat -c '%u' "${workspace_parent}")" == 0 \
  && "$(stat -c '%a' "${workspace_parent}")" == 755 ]] \
  || fail 'the GraphHelm temporary workspace parent must be a root-owned mode 0755 directory'
build_source=''
candidate=''
recovery=''
bundle=''
service_was_active=false
backup_attempted=false
binary_swapped=false
transaction_complete=false
rollback_running=false
service_candidate=''
service_candidate_dir=''
sealed_candidate_dir=''
event_capture_dir=''
rollback_secret_dir=''
rollback_token=''
rollback_auth_header=''

run_as_builder() {
  runuser --user "${BUILD_USER}" -- env -i \
    HOME="${BUILD_CACHE_DIR}" \
    USER="${BUILD_USER}" \
    LOGNAME="${BUILD_USER}" \
    CARGO_HOME="${INSTALL_CARGO_HOME}" \
    RUSTUP_HOME="${INSTALL_RUSTUP_HOME}" \
    CARGO_TARGET_DIR="${BUILD_TARGET_DIR}" \
    PATH="${INSTALL_CARGO_HOME}/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
    "$@"
}

run_as_verifier() {
  # The isolated directory is this account HOME: `env -i` clears the environment, and a HOME
  # pointing at the service state directory would hand the verifier a path into the very
  # installation it must not reach.
  runuser --user "${VERIFY_USER}" -- env -i \
    HOME="${1}" \
    USER="${VERIFY_USER}" \
    LOGNAME="${VERIFY_USER}" \
    PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
    "${@:2}"
}

run_as_verifier_bounded() {
  local verifier_home="$1"
  shift
  timeout --signal=TERM --kill-after="${VERIFY_TIMEOUT_SECONDS}s" "${VERIFY_TIMEOUT_SECONDS}s" \
    runuser --user "${VERIFY_USER}" -- env -i \
      HOME="${verifier_home}" \
      USER="${VERIFY_USER}" \
      LOGNAME="${VERIFY_USER}" \
      PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
      prlimit --as="${MAX_VERIFY_ADDRESS_SPACE_BYTES}" --fsize="${MAX_VERIFY_FILE_BYTES}" \
        --nproc="${MAX_VERIFY_PROCESSES}" -- \
        "$@"
}
run_as_service_bounded() {
  runuser --user "${SERVICE_USER}" -- env -i \
    HOME="${STATE_DIR}" \
    USER="${SERVICE_USER}" \
    LOGNAME="${SERVICE_USER}" \
    PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
    prlimit --fsize="${MAX_EVENT_ARCHIVE_BYTES}" --as="${MAX_CAPTURE_ADDRESS_SPACE_BYTES}" -- \
      "$@"
}

run_as_service() {
  runuser --user "${SERVICE_USER}" -- env -i \
    HOME="${STATE_DIR}" \
    USER="${SERVICE_USER}" \
    LOGNAME="${SERVICE_USER}" \
    PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
    "$@"
}

open_operation_lock() {
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
  exec 9<>"${LOCK_PATH}"
}

smoke_runtime() {
  local health_ready=false
  local unauth_status auth_status
  for _ in $(seq 1 30); do
    if systemctl is-active --quiet "${SERVICE_NAME}.service" \
      && curl --silent --show-error --fail --max-time 2 \
        http://127.0.0.1:8080/health </dev/null >/dev/null; then
      health_ready=true
      break
    fi
    sleep 1
  done
  [[ "${health_ready}" == true ]] || return 1
  unauth_status="$(
    curl --silent --show-error --output /dev/null --write-out '%{http_code}' --max-time 2 \
      http://127.0.0.1:8080/v1/executions/upgrade-smoke </dev/null
  )" || return 1
  [[ "${unauth_status}" == 401 ]] || return 1
  auth_status="$(
    curl --header "@${rollback_auth_header}" --silent --show-error --output /dev/null \
      --write-out '%{http_code}' --max-time 2 \
      http://127.0.0.1:8080/does-not-exist </dev/null
  )" || return 1
  [[ "${auth_status}" == 404 ]]
}

atomic_install_binary() {
  local source_binary="$1"
  local temporary
  temporary="$(mktemp "$(dirname -- "${BINARY_PATH}")/.graphhelm-upgrade.XXXXXX")" \
    || return 1
  if ! install --owner root --group root --mode 0755 -- "${source_binary}" "${temporary}"; then
    rm -f -- "${temporary}"
    return 1
  fi
  if ! mv --force -- "${temporary}" "${BINARY_PATH}"; then
    rm -f -- "${temporary}"
    return 1
  fi
}

atomic_install_token() {
  local source_token="$1"
  local temporary
  temporary="$(mktemp "${STATE_DIR}/.events.token.XXXXXX")" || return 1
  if ! install --owner "${SERVICE_USER}" --group "${SERVICE_USER}" --mode 0600 -- \
    "${source_token}" "${temporary}"; then
    rm -f -- "${temporary}"
    return 1
  fi
  if ! mv --force -- "${temporary}" "${TOKEN_PATH}"; then
    rm -f -- "${temporary}"
    return 1
  fi
}

capture_event_archive() {
  local backup_binary="$1"
  local output_archive="$2"
  local receipt_path="$3"
  local executable_copy
  local service_staging
  event_capture_dir="$(mktemp -d "${workspace_parent}/graphhelm-event-capture.XXXXXX")" \
    || return 1
  install --directory --owner root --group "${SERVICE_USER}" --mode 0710 \
    "${event_capture_dir}" || return 1
  executable_copy="${event_capture_dir}/graphhelm"
  install --owner root --group root --mode 0755 -- \
    "${backup_binary}" "${executable_copy}" || return 1
  service_staging="${event_capture_dir}/service"
  install --directory --owner "${SERVICE_USER}" --group "${SERVICE_USER}" --mode 0700 \
    "${service_staging}" || return 1
  if ! run_as_service_bounded "${executable_copy}" events backup --repository "${EVENTS_DIR}" \
    --output "${service_staging}/events.archive" > "${receipt_path}"; then
    return 1
  fi
  grep -Eq '"ok"[[:space:]]*:[[:space:]]*true' "${receipt_path}" || return 1
  python3 "${seal_helper}" --source "${service_staging}/events.archive" \
    --destination "${output_archive}" --mode 0600 \
    --max-bytes "${MAX_EVENT_ARCHIVE_BYTES}" || return 1
  rm -rf -- "${event_capture_dir}"
  event_capture_dir=''
}

rollback_upgrade() {
  local rollback_failed=false
  local evidence_failed=false
  local failed_store_saved=false
  rollback_running=true
  set +e
  systemctl stop "${SERVICE_NAME}.service" >/dev/null 2>&1 || rollback_failed=true
  if [[ -x "${BINARY_PATH}" ]]; then
    cp --archive -- "${BINARY_PATH}" "${recovery}/failed-candidate.graphhelm" \
      || evidence_failed=true
  fi
  atomic_install_binary "${recovery}/old.graphhelm" || rollback_failed=true
  if [[ "${rollback_failed}" == false && "${failed_store_saved}" != true ]]; then
    if capture_event_archive "${BINARY_PATH}" \
      "${recovery}/failed-store.events.archive" \
      "${recovery}/failed-store-receipt.json" 2>/dev/null; then
      failed_store_saved=true
    else
      evidence_failed=true
    fi
  fi
  if [[ "${rollback_failed}" == false ]]; then
    GRAPHHELM_OPERATION_LOCK_FD=9 \
      "${restore_script}" --bundle "${bundle}" --replace-existing \
      > "${recovery}/rollback-restore.log" 2>&1 || rollback_failed=true
  fi
  if [[ "${rollback_failed}" == false ]]; then
    systemctl stop "${SERVICE_NAME}.service" >/dev/null 2>&1 || rollback_failed=true
  fi
  if [[ "${rollback_failed}" == false ]]; then
    atomic_install_token "${rollback_token}" || rollback_failed=true
  fi
  if [[ "${rollback_failed}" == false ]]; then
    systemctl start "${SERVICE_NAME}.service" >/dev/null 2>&1 || rollback_failed=true
  fi
  if [[ "${rollback_failed}" == false ]]; then
    smoke_runtime || rollback_failed=true
  fi
  if [[ "${rollback_failed}" == true || "${evidence_failed}" == true \
    || "${failed_store_saved}" != true ]]; then
    systemctl stop "${SERVICE_NAME}.service" >/dev/null 2>&1 || true
    printf 'graphhelm upgrade: rollback was incomplete; service left stopped; recovery evidence: %s\n' \
      "${recovery}" >&2
  else
    printf 'graphhelm upgrade: candidate failed and was rolled back; recovery evidence: %s\n' \
      "${recovery}" >&2
  fi
  set -e
}

cleanup() {
  local status=$?
  trap - EXIT
  if [[ "${status}" -ne 0 && "${binary_swapped}" == true \
    && "${transaction_complete}" != true && "${rollback_running}" != true ]]; then
    rollback_upgrade
  elif [[ "${status}" -ne 0 && "${backup_attempted}" == true \
    && "${binary_swapped}" != true && "${service_was_active}" == true ]]; then
    if ! systemctl is-active --quiet "${SERVICE_NAME}.service"; then
      systemctl start "${SERVICE_NAME}.service" >/dev/null 2>&1 \
        || printf 'graphhelm upgrade: failed to restart the old service; it remains stopped\n' >&2
    fi
  fi
  if [[ -n "${service_candidate_dir}" ]]; then
    case "${service_candidate_dir}" in
      "${workspace_parent}/graphhelm-candidate."*) rm -rf -- "${service_candidate_dir}" ;;
      *) printf 'graphhelm upgrade: refusing to clean unexpected candidate path: %s\n' \
           "${service_candidate_dir}" >&2; status=1 ;;
    esac
  fi
  if [[ -n "${event_capture_dir}" ]]; then
    case "${event_capture_dir}" in
      "${workspace_parent}/graphhelm-event-capture."*) rm -rf -- "${event_capture_dir}" ;;
      *) printf 'graphhelm upgrade: refusing to clean unexpected event capture path: %s\n' \
           "${event_capture_dir}" >&2; status=1 ;;
    esac
  fi
  if [[ -n "${sealed_candidate_dir}" ]]; then
    case "${sealed_candidate_dir}" in
      "${workspace_parent}/graphhelm-sealed-candidate."*) rm -rf -- "${sealed_candidate_dir}" ;;
      *) printf 'graphhelm upgrade: refusing to clean unexpected sealed candidate path: %s\n' \
           "${sealed_candidate_dir}" >&2; status=1 ;;
    esac
  fi
  if [[ -n "${rollback_secret_dir}" ]]; then
    case "${rollback_secret_dir}" in
      "${workspace_parent}/graphhelm-upgrade-secret."*) rm -rf -- "${rollback_secret_dir}" ;;
      *) printf 'graphhelm upgrade: refusing to clean unexpected rollback secret path: %s\n' \
           "${rollback_secret_dir}" >&2; status=1 ;;
    esac
  fi
  case "${build_source}" in
    "${workspace_parent}/graphhelm-source."*) rm -rf -- "${build_source}" ;;
    *) printf 'graphhelm upgrade: refusing to clean unexpected build path: %s\n' \
         "${build_source}" >&2; status=1 ;;
  esac
  exit "${status}"
}
run_as_builder rustup toolchain install "${RUST_VERSION}" --profile minimal
build_source="$(mktemp -d "${workspace_parent}/graphhelm-source.XXXXXX")"
trap cleanup EXIT
install --directory --owner "${BUILD_USER}" --group "${BUILD_GROUP}" --mode 0700 \
  "${build_source}"
cp --archive -- \
  "${source_dir}/Cargo.toml" \
  "${source_dir}/Cargo.lock" \
  "${source_dir}/rust-toolchain.toml" \
  "${source_dir}/adapters" \
  "${source_dir}/apps" \
  "${source_dir}/core" \
  "${source_dir}/extensions" \
  "${source_dir}/schemas" \
  "${source_dir}/tools" \
  "${build_source}/"
chown --recursive "${BUILD_USER}:${BUILD_GROUP}" "${build_source}"
[[ -f "${build_source}/Cargo.toml" ]] \
  || fail 'the isolated build copy is missing Cargo.toml'
run_as_builder cargo "+${RUST_VERSION}" build --locked --release -p graphhelm-cli \
  --manifest-path "${build_source}/Cargo.toml"
sealed_candidate_dir="$(mktemp -d "${workspace_parent}/graphhelm-sealed-candidate.XXXXXX")"
chmod 0755 -- "${sealed_candidate_dir}"
candidate="${sealed_candidate_dir}/graphhelm"
python3 "${seal_helper}" --source "${BUILD_TARGET_DIR}/release/graphhelm" \
  --destination "${candidate}" --mode 0755 \
  --max-bytes "${MAX_BINARY_BYTES}" \
  || fail 'the built candidate crossed an unsafe untrusted file boundary'
[[ -s "${candidate}" ]] || fail 'the built GraphHelm candidate is empty'
[[ "$(od -An -tx1 -N4 "${candidate}" | tr -d ' \n')" == 7f454c46 ]] \
  || fail 'the built GraphHelm candidate is not an ELF executable'
run_as_builder "${candidate}" events backup --help >/dev/null \
  || fail 'candidate is missing required local CLI verb: events backup'
run_as_builder "${candidate}" events restore --help >/dev/null \
  || fail 'candidate is missing required local CLI verb: events restore'

candidate_sha="$(sha256sum "${candidate}" | awk '{print $1}')"
installed_sha="$(sha256sum "${BINARY_PATH}" | awk '{print $1}')"
if [[ "${candidate_sha}" == "${installed_sha}" ]]; then
  printf 'GraphHelm is already current (%s); no service stop or backup was needed.\n' \
    "${candidate_sha}"
  transaction_complete=true
  exit 0
fi

open_operation_lock
flock --nonblock 9 \
  || fail 'another GraphHelm backup, restore, or upgrade operation is already running'
installed_sha="$(sha256sum "${BINARY_PATH}" | awk '{print $1}')"
if [[ "${candidate_sha}" == "${installed_sha}" ]]; then
  printf 'GraphHelm is already current (%s); no service stop or backup was needed.\n' \
    "${candidate_sha}"
  transaction_complete=true
  exit 0
fi

[[ ! -L "${RECOVERY_BASE}" ]] || fail 'the upgrade recovery directory is an unsafe symlink'
install -d -m 0700 -- "${RECOVERY_BASE}"
recovery="$(mktemp -d "${RECOVERY_BASE}/upgrade-$(date -u +%Y%m%dT%H%M%SZ).XXXXXX")"
chmod 0700 "${recovery}"
rollback_secret_dir="$(mktemp -d "${workspace_parent}/graphhelm-upgrade-secret.XXXXXX")"
chmod 0700 "${rollback_secret_dir}"
rollback_token="${rollback_secret_dir}/events.token"
python3 "${seal_helper}" --source "${TOKEN_PATH}" \
  --destination "${rollback_token}" --mode 0600 \
  --max-bytes "${MAX_TOKEN_BYTES}" \
  || fail 'the Runtime token crossed an unsafe untrusted file boundary'
grep -Eq '^[0-9a-f]{64}$' "${rollback_token}" \
  || fail 'the Runtime token is not a lowercase 64-character hexadecimal value'
rollback_auth_header="${rollback_secret_dir}/authorization.header"
token="$(<"${rollback_token}")"
printf 'Authorization: Bearer %s\n' "${token}" > "${rollback_auth_header}"
unset token
chmod 0600 -- "${rollback_auth_header}"
install --owner root --group root --mode 0755 -- "${candidate}" "${recovery}/candidate.graphhelm"
install --owner root --group root --mode 0755 -- "${BINARY_PATH}" "${recovery}/old.graphhelm"
if systemctl is-active --quiet "${SERVICE_NAME}.service"; then
  service_was_active=true
fi
sha256sum "${recovery}/old.graphhelm" "${recovery}/candidate.graphhelm" \
  > "${recovery}/binary-hashes.sha256"
sha256sum "${rollback_token}" "${UNIT_PATH}" > "${recovery}/config-before.sha256"

backup_attempted=true
GRAPHHELM_OPERATION_LOCK_FD=9 \
  "${backup_script}" --destination "${recovery}" --leave-stopped \
  > "${recovery}/backup.log"
bundle="$(find "${recovery}" -maxdepth 1 -type f -name 'graphhelm-vps-*.tar' -print -quit)"
[[ -n "${bundle}" ]] || fail 'the approved backup script did not publish its bundle'
install -d -m 0700 -- "${recovery}/pre-upgrade"
tar -xf "${bundle}" -C "${recovery}/pre-upgrade" -- events.archive
sha256sum "${recovery}/pre-upgrade/events.archive" > "${recovery}/events-before.sha256"

service_candidate_dir="$(mktemp -d "${workspace_parent}/graphhelm-candidate.XXXXXX")"
chmod 0755 -- "${service_candidate_dir}"
service_candidate="${service_candidate_dir}/graphhelm"
install --owner root --group root --mode 0755 -- \
  "${recovery}/candidate.graphhelm" "${service_candidate}"
isolated_events="${service_candidate_dir}/events"
isolated_archive="${service_candidate_dir}/events.archive"
install --directory --owner "${VERIFY_USER}" --group "${VERIFY_USER}" --mode 0700 \
  "${isolated_events}"
install --owner "${VERIFY_USER}" --group "${VERIFY_USER}" --mode 0400 -- \
  "${recovery}/pre-upgrade/events.archive" "${isolated_archive}"
run_as_verifier "${service_candidate_dir}" "${BINARY_PATH}" events restore --repository "${isolated_events}" \
  --archive "${isolated_archive}" \
  > "${recovery}/isolated-restore.json"
grep -Eq '"ok"[[:space:]]*:[[:space:]]*true' "${recovery}/isolated-restore.json" \
  || fail 'the installed CLI could not create the isolated verification repository'
if run_as_verifier_bounded "${service_candidate_dir}" "${service_candidate}" events verify \
  --repository "${isolated_events}" > "${recovery}/candidate-verify.json"; then
  :
else
  verifier_status=$?
  if [[ "${verifier_status}" == 124 ]]; then
    fail "candidate verification exceeded the ${VERIFY_TIMEOUT_SECONDS}-second TERM deadline; the ${VERIFY_TIMEOUT_SECONDS}-second kill grace also expired; backup retained at ${recovery}"
  fi
  fail "candidate verification failed with status ${verifier_status}; backup retained at ${recovery}"
fi
grep -Eq '"ok"[[:space:]]*:[[:space:]]*true' "${recovery}/candidate-verify.json" \
  || fail 'candidate could not verify the installed local event repository'
if ! grep -Eq '"formatSupported"[[:space:]]*:[[:space:]]*true' \
  "${recovery}/candidate-verify.json"; then
  fail "candidate cannot read the current event format; a release-specific migration is required; backup retained at ${recovery}"
fi
rm -rf -- "${service_candidate_dir}"
service_candidate=''
service_candidate_dir=''

atomic_install_binary "${recovery}/candidate.graphhelm"
binary_swapped=true
systemctl start "${SERVICE_NAME}.service"
smoke_runtime || fail 'candidate health/auth smoke failed; automatic rollback will run'
systemctl stop "${SERVICE_NAME}.service"
capture_event_archive "${recovery}/old.graphhelm" \
  "${recovery}/events-after.archive" "${recovery}/events-after-receipt.json" \
  || fail 'post-smoke event fingerprint backup failed; automatic rollback will run'
before_events_sha="$(awk '{print $1}' "${recovery}/events-before.sha256")"
after_events_sha="$(sha256sum "${recovery}/events-after.archive" | awk '{print $1}')"
[[ "${before_events_sha}" == "${after_events_sha}" ]] \
  || fail 'the event store changed during upgrade smoke; automatic rollback will run'
final_token="${rollback_secret_dir}/events-after.token"
python3 "${seal_helper}" --source "${TOKEN_PATH}" \
  --destination "${final_token}" --mode 0600 --max-bytes "${MAX_TOKEN_BYTES}" \
  || fail 'the Runtime token crossed an unsafe untrusted file boundary during fingerprint proof'
grep -Eq '^[0-9a-f]{64}$' "${final_token}" \
  || fail 'the Runtime token changed to an invalid value during fingerprint proof'
sha256sum "${final_token}" "${UNIT_PATH}" > "${recovery}/config-after.sha256"
before_config="$(awk '{print $1}' "${recovery}/config-before.sha256" | tr '\n' ' ')"
after_config="$(awk '{print $1}' "${recovery}/config-after.sha256" | tr '\n' ' ')"
[[ "${before_config}" == "${after_config}" ]] \
  || fail 'the token or systemd unit changed during upgrade; automatic rollback will run'
{
  printf '%s  events-before-and-after\n' "${before_events_sha}"
  cat "${recovery}/config-after.sha256"
} > "${recovery}/fingerprints.sha256"
systemctl start "${SERVICE_NAME}.service"
smoke_runtime \
  || fail 'final post-fingerprint health/auth smoke failed; automatic rollback will run'
transaction_complete=true
printf 'GraphHelm upgrade completed and passed health/auth/fingerprint proof.\n'
printf 'Rollback evidence retained at: %s\n' "${recovery}"
