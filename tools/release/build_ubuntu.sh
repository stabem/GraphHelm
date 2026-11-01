#!/bin/sh
# Builds and packages the GraphHelm CLI for Ubuntu 24.04 x86-64 inside an ubuntu:24.04 container.
# Input (mounted): /in/source.bundle, a git bundle of the release head (a clone keeps .git so the
# binary records its commit, which a plain archive cannot). Usage: build_ubuntu.sh <out dir> <sha>.
# Output (mounted): /out/<dir> with the archive, SHA256SUMS.txt and manifest.json.
# The cargo target and the rustup home live in named volumes, so a rerun is incremental.
set -eu
OUT_DIR="$1"
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq --no-install-recommends ca-certificates curl build-essential pkg-config git python3 >/dev/null
export RUSTUP_HOME=/rustup CARGO_HOME=/cargo PATH=/cargo/bin:$PATH
if ! command -v rustup >/dev/null 2>&1; then
  curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none --no-modify-path
fi
rustup toolchain install 1.97.1 --profile minimal >/dev/null
rm -rf /src && git clone -q /in/source.bundle /src
cd /src
git -c advice.detachedHead=false checkout -q "$2"
test "$(git rev-parse HEAD)" = "$2"
CARGO_TARGET_DIR=/target cargo +1.97.1 build --locked --release -p graphhelm-cli --bin graphhelm
/target/release/graphhelm --version
python3 /src/tools/release/package_release.py --binary /target/release/graphhelm --repo /src \
  --graph /src/examples/graphs/provider-less-demo.yaml --target ubuntu-24.04-x86_64 --out "/out/$OUT_DIR"
echo "rustc: $(rustc +1.97.1 --version)"
echo "os: $(. /etc/os-release && echo "$PRETTY_NAME")"
