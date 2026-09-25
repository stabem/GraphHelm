#!/usr/bin/env bash
set -Eeuo pipefail

REPOSITORY_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
readonly HELPER="${REPOSITORY_ROOT}/deploy/seal-vps-file.py"

fail() {
  printf 'vps file seal test: %s\n' "$*" >&2
  exit 1
}

SANDBOX="$(mktemp -d)"
cleanup() { rm -rf -- "${SANDBOX}"; }
trap cleanup EXIT

source_file="${SANDBOX}/source"
destination="${SANDBOX}/destination"
printf 'ordinary payload\n' > "${source_file}"
python3 "${HELPER}" --source "${source_file}" --destination "${destination}" --mode 0600
[[ "$(cat "${destination}")" == 'ordinary payload' ]] \
  || fail 'ordinary file did not survive sealing'
[[ "$(stat -c '%a' "${destination}")" == 600 ]] \
  || fail 'sealed file mode is not 0600'

fifo_source="${SANDBOX}/source-fifo"
fifo_destination="${SANDBOX}/fifo-destination"
mkfifo -- "${fifo_source}"
set +e
fifo_output="$(timeout 3s python3 "${HELPER}" --source "${fifo_source}" \
  --destination "${fifo_destination}" --mode 0600 2>&1)"
fifo_status=$?
set -e
[[ ${fifo_status} -ne 0 && "${fifo_output}" == *'unsafe untrusted file'* ]] \
  || fail 'a FIFO source was accepted or blocked instead of being refused'
[[ ! -e "${fifo_destination}" ]] \
  || fail 'FIFO refusal left a destination'

victim="${SANDBOX}/victim"
symlink_source="${SANDBOX}/symlink-source"
printf 'root-readable material\n' > "${victim}"
ln -s -- "${victim}" "${symlink_source}"
set +e
symlink_output="$(python3 "${HELPER}" --source "${symlink_source}" \
  --destination "${SANDBOX}/symlink-destination" --mode 0600 2>&1)"
symlink_status=$?
set -e
[[ ${symlink_status} -ne 0 && "${symlink_output}" == *'unsafe untrusted file'* ]] \
  || fail 'final-component symlink was accepted'
[[ ! -e "${SANDBOX}/symlink-destination" ]] \
  || fail 'final-component symlink left a destination'

real_parent="${SANDBOX}/real-parent"
linked_parent="${SANDBOX}/linked-parent"
mkdir -- "${real_parent}"
printf 'ancestor target\n' > "${real_parent}/source"
ln -s -- "${real_parent}" "${linked_parent}"
set +e
ancestor_output="$(python3 "${HELPER}" --source "${linked_parent}/source" \
  --destination "${SANDBOX}/ancestor-destination" --mode 0600 2>&1)"
ancestor_status=$?
set -e
[[ ${ancestor_status} -ne 0 && "${ancestor_output}" == *'unsafe untrusted file'* ]] \
  || fail 'ancestor symlink was accepted'
[[ ! -e "${SANDBOX}/ancestor-destination" ]] \
  || fail 'ancestor symlink left a destination'

printf 'vps file seal tests: PASS\n'
