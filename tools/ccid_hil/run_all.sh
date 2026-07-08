#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Run the CCID hardware-in-the-loop suite on a Linux USB host (e.g. Raspberry Pi).

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
HIL="${ROOT}/tools/ccid_hil"
OUT="${CCID_HIL_OUT:-/tmp/ccid-hil-out}"
VID="${CCID_VID:-1d50}"
PID="${CCID_PID:-6198}"
TIMEOUT="${CCID_WAIT_TIMEOUT:-60}"
RUN_PROVISION="${CCID_HIL_PROVISION:-0}"
STRESS="${CCID_HIL_STRESS:-100}"

mkdir -p "${OUT}"
export PYTHONPATH="${HIL}"

echo "CCID HIL output: ${OUT}"
echo "Target device: ${VID}:${PID}"

"${HIL}/wait_device.sh" | tee "${OUT}/00-wait.log"

python3 "${HIL}/test_enumerate.py" --vid "0x${VID}" --pid "0x${PID}" --timeout "${TIMEOUT}" \
  | tee "${OUT}/01-enumerate.log"

if [[ "${RUN_PROVISION}" == "1" ]]; then
  python3 "${HIL}/test_provision.py" --vid "0x${VID}" --pid "0x${PID}" --timeout "${TIMEOUT}" \
    | tee "${OUT}/02-provision.log"
else
  echo "Skipping provisioning test (set CCID_HIL_PROVISION=1 on factory-reset device)" \
    | tee "${OUT}/02-provision.log"
fi

python3 "${HIL}/test_echo.py" --vid "0x${VID}" --pid "0x${PID}" --timeout "${TIMEOUT}" \
  | tee "${OUT}/03-echo.log"

python3 "${HIL}/test_echo.py" --vid "0x${VID}" --pid "0x${PID}" --timeout "${TIMEOUT}" --stress "${STRESS}" \
  | tee "${OUT}/04-stress.log"

echo "CCID HIL suite PASS" | tee "${OUT}/summary.log"
