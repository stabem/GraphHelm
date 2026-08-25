#!/usr/bin/env bash
set -Eeuo pipefail

readonly SERVICE_NAME="graphhelm"
readonly SERVICE_USER="graphhelm"
readonly SERVICE_GROUP="graphhelm"
readonly BUILD_USER="graphhelm-build"
readonly BUILD_GROUP="graphhelm-build"
readonly BINARY_PATH="/usr/local/bin/graphhelm"
readonly STATE_DIR="/var/lib/graphhelm"
readonly EVENTS_DIR="${STATE_DIR}/events"
readonly TOKEN_PATH="${STATE_DIR}/events.token"
readonly UNIT_PATH="/etc/systemd/system/${SERVICE_NAME}.service"
readonly RUST_VERSION="1.97.1"
readonly INSTALL_CARGO_HOME="/opt/graphhelm-cargo"
readonly INSTALL_RUSTUP_HOME="/opt/graphhelm-rustup"
readonly BUILD_CACHE_DIR="/var/cache/graphhelm-build"
readonly BUILD_TARGET_DIR="${BUILD_CACHE_DIR}/target"

fail() {
  printf 'graphhelm install: %s\n' "$*" >&2
  exit 1
}

if [[ "${EUID}" -ne 0 ]]; then
  fail "run this script as root (for example: sudo ./install/install.sh)"
fi

if [[ ! -r /etc/os-release ]]; then
  fail "/etc/os-release is missing; Ubuntu is required"
fi

# shellcheck disable=SC1091
source /etc/os-release
if [[ "${ID:-}" != "ubuntu" ]]; then
  fail "Ubuntu is required; detected ${ID:-unknown}"
fi

command -v systemctl >/dev/null 2>&1 || fail "systemd is required"

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
readonly SCRIPT_DIR
REPOSITORY_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd -P)"
readonly REPOSITORY_ROOT
[[ -f "${REPOSITORY_ROOT}/Cargo.lock" ]] || fail "run the installer from a GraphHelm source checkout"
[[ -f "${REPOSITORY_ROOT}/apps/cli/Cargo.toml" ]] || fail "apps/cli is missing from this checkout"

export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install --yes --no-install-recommends \
  build-essential \
  ca-certificates \
  curl \
  git \
  pkg-config

if ! getent group "${BUILD_GROUP}" >/dev/null; then
  groupadd --system "${BUILD_GROUP}"
fi
if ! id --user "${BUILD_USER}" >/dev/null 2>&1; then
  useradd --system --gid "${BUILD_GROUP}" --no-create-home \
    --home-dir "${BUILD_CACHE_DIR}" --shell /usr/sbin/nologin "${BUILD_USER}"
fi
if ! getent group "${SERVICE_GROUP}" >/dev/null; then
  groupadd --system "${SERVICE_GROUP}"
fi
if ! id --user "${SERVICE_USER}" >/dev/null 2>&1; then
  useradd --system --gid "${SERVICE_GROUP}" --home-dir "${STATE_DIR}" \
    --shell /usr/sbin/nologin "${SERVICE_USER}"
fi

install --directory --owner "${BUILD_USER}" --group "${BUILD_GROUP}" --mode 0755 \
  "${INSTALL_CARGO_HOME}" "${INSTALL_RUSTUP_HOME}" "${BUILD_CACHE_DIR}" \
  "${BUILD_TARGET_DIR}"
install --directory --owner "${SERVICE_USER}" --group "${SERVICE_GROUP}" --mode 0700 \
  "${STATE_DIR}" "${EVENTS_DIR}"

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

if [[ ! -x "${INSTALL_CARGO_HOME}/bin/rustup" ]]; then
  curl --proto '=https' --tlsv1.2 --silent --show-error --fail \
    https://sh.rustup.rs \
    | run_as_builder sh -s -- -y --no-modify-path --default-toolchain none --profile minimal
fi

run_as_builder rustup toolchain install "${RUST_VERSION}" --profile minimal

BUILD_SOURCE_DIR="$(mktemp --directory /var/tmp/graphhelm-source.XXXXXX)"
readonly BUILD_SOURCE_DIR
BINARY_TEMP=""
cleanup_temporary_files() {
  case "${BUILD_SOURCE_DIR}" in
    /var/tmp/graphhelm-source.*) rm -rf -- "${BUILD_SOURCE_DIR}" ;;
    *) fail "refusing to clean unexpected build path: ${BUILD_SOURCE_DIR}" ;;
  esac
  if [[ -n "${BINARY_TEMP}" ]]; then
    case "${BINARY_TEMP}" in
      /usr/local/bin/.graphhelm.*) rm -f -- "${BINARY_TEMP}" ;;
      *) fail "refusing to clean unexpected binary path: ${BINARY_TEMP}" ;;
    esac
  fi
}
trap cleanup_temporary_files EXIT
install --directory --owner "${BUILD_USER}" --group "${BUILD_GROUP}" --mode 0700 \
  "${BUILD_SOURCE_DIR}"
cp --archive -- \
  "${REPOSITORY_ROOT}/Cargo.toml" \
  "${REPOSITORY_ROOT}/Cargo.lock" \
  "${REPOSITORY_ROOT}/rust-toolchain.toml" \
  "${REPOSITORY_ROOT}/adapters" \
  "${REPOSITORY_ROOT}/apps" \
  "${REPOSITORY_ROOT}/core" \
  "${REPOSITORY_ROOT}/extensions" \
  "${REPOSITORY_ROOT}/schemas" \
  "${REPOSITORY_ROOT}/tools" \
  "${BUILD_SOURCE_DIR}/"
chown --recursive "${BUILD_USER}:${BUILD_GROUP}" "${BUILD_SOURCE_DIR}"

(
  cd -- "${BUILD_SOURCE_DIR}"
  run_as_builder cargo "+${RUST_VERSION}" build --locked --release -p graphhelm-cli
)

BINARY_TEMP="$(mktemp /usr/local/bin/.graphhelm.XXXXXX)"
run_as_builder cat -- "${BUILD_TARGET_DIR}/release/graphhelm" > "${BINARY_TEMP}"
[[ -s "${BINARY_TEMP}" ]] || fail "the built GraphHelm binary is empty"
[[ "$(od -An -tx1 -N4 "${BINARY_TEMP}" | tr -d ' \n')" == "7f454c46" ]] \
  || fail "the built GraphHelm artifact is not an ELF executable"
chown root:root "${BINARY_TEMP}"
chmod 0755 "${BINARY_TEMP}"
mv --force -- "${BINARY_TEMP}" "${BINARY_PATH}"
BINARY_TEMP=""

install --owner root --group root --mode 0644 /dev/stdin "${UNIT_PATH}" <<'UNIT'
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

systemctl daemon-reload
systemctl enable "${SERVICE_NAME}.service" >/dev/null
systemctl restart "${SERVICE_NAME}.service"

health_ready=false
for _ in $(seq 1 30); do
  if systemctl is-active --quiet "${SERVICE_NAME}.service" \
    && [[ -f "${TOKEN_PATH}" ]] \
    && curl --silent --show-error --fail --max-time 2 \
      http://127.0.0.1:8080/health >/dev/null; then
    health_ready=true
    break
  fi
  sleep 1
done

if [[ "${health_ready}" != true ]]; then
  journalctl --unit "${SERVICE_NAME}.service" --no-pager --lines 50 >&2 || true
  fail "the service did not become healthy at http://127.0.0.1:8080/health"
fi

printf '\nGraphHelm is running at http://127.0.0.1:8080\n'
printf 'Bearer token (kept at %s):\n' "${TOKEN_PATH}"
cat -- "${TOKEN_PATH}"
printf '\n'
