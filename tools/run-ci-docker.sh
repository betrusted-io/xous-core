#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Run GitHub-hosted CI workflows locally inside the rust-xous Docker image.
#
# Usage (from repo root):
#   tools/run-ci-docker.sh              # all hosted CI (incl. PDDB Renode)
#   tools/run-ci-docker.sh --quick      # skip PDDB Renode (~30-75 min saved)
#   tools/run-ci-docker.sh --inner      # already inside container; do not re-exec
#
# Uses docker when the daemon is reachable; falls back to podman otherwise.
# Container runs as root so apt works; build artifacts under target/ may be root-owned.
#
# Skipped (not reproducible in this container):
#   - ccid-hil.yml (self-hosted USB hardware)
#   - docker.yml (GHCR image publish; overlaps build.yml xtask targets)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

RUST_VERSION="${RUST_VERSION:-1.93.0}"
RUST_IMAGE_SHA256="${RUST_IMAGE_SHA256:-5fdb652f0f6e83c24ebbba6d9c51a9836c6f3c4fa12dc3040e6327c6eb355769}"
RUST_IMAGE_SHA256="${RUST_IMAGE_SHA256#sha256:}"
RUST_IMAGE="ghcr.io/sbellem/rust-xous:${RUST_VERSION}-slim-bullseye@sha256:${RUST_IMAGE_SHA256}"

QUICK=0
INNER=0
for arg in "$@"; do
  case "$arg" in
    --quick) QUICK=1 ;;
    --inner) INNER=1 ;;
    -h|--help)
      sed -n '3,14p' "$0"
      exit 0
      ;;
    *)
      echo "Unknown arg: $arg (try --help)" >&2
      exit 2
      ;;
  esac
done

step() {
  echo ""
  echo "======================================================================"
  echo "== $*"
  echo "======================================================================"
}

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

run_hosted_ci() {
  trap 'echo "CI FAILED at line ${LINENO}: ${BASH_COMMAND}" >&2' ERR
  export PATH="/usr/local/cargo/bin:${PATH}"

  step "Install Ubuntu dependencies"
  if command -v sudo >/dev/null 2>&1; then
    sudo apt-get update -qq
    sudo apt-get install -y -qq libxkbcommon-dev curl jq python3 python3-pip perl findutils
  else
    apt-get update -qq
    apt-get install -y -qq libxkbcommon-dev curl jq python3 python3-pip perl findutils
  fi

  step "Git: fetch tags (CI parity)"
  git fetch --prune --unshallow --tags 2>/dev/null || git fetch --prune --tags || true
  git remote add upstream https://github.com/betrusted-io/xous-core.git 2>/dev/null || true
  git fetch upstream --tags --prune --no-recurse-submodules 2>/dev/null || true

  step "trailing_whitespace_check"
  local ws_violations
  ws_violations="$(
    find . -name '*.rs' \
      -not -path './target/*' \
      -not -path './.git/*' \
      -print0 \
      | xargs -0 grep -c --with-filename -P '[ \t]$' \
      | grep -v ':0$' || true
  )"
  if [[ -n "${ws_violations}" ]]; then
    echo "${ws_violations}"
    fail "trailing whitespace in .rs files"
  fi
  echo "PASS"

  step "rustfmt_check"
  rustup toolchain install nightly --profile minimal
  rustup component add rustfmt --toolchain nightly
  DEFAULT_BRANCH="$(git remote show origin 2>/dev/null | awk '/HEAD branch/ {print $NF}' || echo dev)"
  git fetch --no-tags --prune --no-recurse-submodules --depth=1 origin "$DEFAULT_BRANCH" 2>/dev/null || true
  if git show "origin/${DEFAULT_BRANCH}:rustfmt.toml" >/dev/null 2>&1; then
    git checkout "origin/${DEFAULT_BRANCH}" -- rustfmt.toml
  fi
  cargo xtask dummy-template
  cargo +nightly fmt --check

  step "Install RISC-V toolkit (shared by ccid-ci + build)"
  cargo xtask install-toolkit --force --no-verify

  step "ccid-ci: framing unit tests"
  cargo test -p usb-bao1x --lib ccid_framing

  step "ccid-ci: ep_budget unit tests"
  cargo test -p usb-bao1x --lib ep_budget

  step "ccid-ci: hosted compile"
  cargo check -p usb-bao1x --features hosted-baosec,ccid-openpgp

  step "ccid-ci: board compile"
  cargo check -p usb-bao1x --features board-baosec,ccid-openpgp,bao1x --target riscv32imac-unknown-xous-elf

  step "ccid-ci: baosec-ccid image"
  cargo xtask baosec-ccid --no-verify

  step "ccid-ci: ccid-hil image compile"
  cargo xtask ccid-hil --no-verify

  step "build.yml matrix (sequential; reuses target/ for local speed)"
  BUILD_TASKS=(
    bao1x-boot0
    bao1x-boot1
    bao1x-alt-boot1
    bao1x-baremetal-dabao
    dabao
    baosec
    hosted-bao1x-ci
    hosted-ci
    renode-image
  )
  for task in "${BUILD_TASKS[@]}"; do
    step "build.yml: cargo xtask ${task}"
    cargo xtask "${task}" --no-verify
  done

  if [[ "$QUICK" -eq 1 ]]; then
    step "SKIP pddb-renode-ci (--quick)"
    return 0
  fi

  step "pddb-renode-ci: resolve latest betrusted-io/rust (CI parity)"
  TAG="$(curl -fsSL https://api.github.com/repos/betrusted-io/rust/releases/latest | jq -r .tag_name)"
  RUSTC_VER="$(echo "$TAG" | cut -d. -f1,2,3)"
  echo "latest betrusted-io/rust release: ${TAG} -> rustc ${RUSTC_VER}"
  rustup toolchain install "${RUSTC_VER}" --profile minimal
  rustup default "${RUSTC_VER}"

  step "pddb-renode-ci: install-toolkit + build image"
  cargo xtask install-toolkit --force --no-verify
  rm -rf target/*
  cargo xtask pddb-fs-ci --no-verify

  step "pddb-renode-ci: Renode portable"
  RENODE_TARBALL="renode-1.16.1+20260417git9d55b4e69.linux-portable-dotnet.tar.gz"
  curl --fail --location --retry 5 -o /tmp/renode.tar.gz \
    "https://dl.antmicro.com/projects/renode/builds/${RENODE_TARBALL}"
  mkdir -p "$HOME/renode_portable"
  tar -xzf /tmp/renode.tar.gz --strip-components=1 -C "$HOME/renode_portable"
  export PATH="$HOME/renode_portable:$PATH"
  renode --version
  python3 -m pip install -q -r "$HOME/renode_portable/tests/requirements.txt"

  step "pddb-renode-ci: fresh flash backing"
  mkdir -p target/pddb-fs-ci
  python3 -c "open('target/pddb-fs-ci/flash-robot.bin','wb').write(b'\xff'*134217728)"

  step "pddb-renode-ci: renode-test pddb-fs.robot"
  export RENODE_CI_MODE=YES
  renode-test -r "$REPO_ROOT/renode-results" emulation/tests/pddb-fs.robot

  step "pddb-renode-ci: render report"
  LOG=target/pddb-fs-ci/console-robot.log
  python3 tools/renode_report.py "$LOG" --format md -o renode-results/summary.md || true
  python3 tools/renode_report.py "$LOG" -o renode-results/test-report.html || true

  step "ALL HOSTED CI PASSED"
}

if [[ "$INNER" -eq 1 ]] || [[ -f /.dockerenv ]] || [[ -f /run/.containerenv ]]; then
  run_hosted_ci
  exit 0
fi

if ! command -v docker >/dev/null 2>&1; then
  fail "docker not found; install Docker or run with --inner inside a container"
fi

CONTAINER_ENGINE="docker"
if ! docker info >/dev/null 2>&1; then
  if command -v podman >/dev/null 2>&1 && podman info >/dev/null 2>&1; then
    CONTAINER_ENGINE="podman"
    echo "NOTE: docker socket unavailable; using podman"
  else
    fail "docker daemon not accessible (add user to docker group or use podman)"
  fi
fi

step "Pull CI image ${RUST_IMAGE}"
"${CONTAINER_ENGINE}" pull "${RUST_IMAGE}"

step "Launch CI container (${CONTAINER_ENGINE})"
INNER_ARGS=(--inner)
[[ "$QUICK" -eq 1 ]] && INNER_ARGS+=(--quick)

"${CONTAINER_ENGINE}" run --rm \
  --user 0:0 \
  -v "${REPO_ROOT}:/home/baozi/xous-core:Z" \
  -w /home/baozi/xous-core \
  "${RUST_IMAGE}" \
  bash -lc "tools/run-ci-docker.sh ${INNER_ARGS[*]}"
