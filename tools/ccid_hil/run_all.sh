#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Run the CCID hardware-in-the-loop suite on a Linux USB host (e.g. Raspberry Pi).
#
# Persona A: CCID images have no debug/provisioning CDC. Debug is UART/xous-log.
# This harness does NOT capture UART yet (OPEN). USB steps only.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
HIL="${ROOT}/tools/ccid_hil"
OUT="${CCID_HIL_OUT:-/tmp/ccid-hil-out}"
VID="${CCID_VID:-1d50}"
PID="${CCID_PID:-6198}"
TIMEOUT="${CCID_WAIT_TIMEOUT:-60}"
STRESS="${CCID_HIL_STRESS:-100}"

mkdir -p "${OUT}"
export PYTHONPATH="${HIL}"

echo "CCID HIL output: ${OUT}"
echo "Target device: ${VID}:${PID}"

if [[ "${CCID_HIL_PROVISION:-0}" == "1" ]]; then
  echo "NOTE: CCID_HIL_PROVISION=1 is obsolete under Persona A (no USB CDC provision)." \
    | tee "${OUT}/02-provision-note.log"
  echo "HIL-02 now asserts CDC absence; ignoring legacy env." \
    | tee -a "${OUT}/02-provision-note.log"
fi

"${HIL}/wait_device.sh" | tee "${OUT}/00-wait.log"

python3 "${HIL}/test_enumerate.py" --vid "0x${VID}" --pid "0x${PID}" --timeout "${TIMEOUT}" \
  | tee "${OUT}/01-enumerate.log"

# HIL-02: Persona A surface check (no CDC / no USB PIN provision path)
python3 "${HIL}/test_provision.py" --vid "0x${VID}" --pid "0x${PID}" --timeout "${TIMEOUT}" \
  | tee "${OUT}/02-provision.log"

python3 "${HIL}/test_echo.py" --vid "0x${VID}" --pid "0x${PID}" --timeout "${TIMEOUT}" \
  | tee "${OUT}/03-echo.log"

python3 "${HIL}/test_echo.py" --vid "0x${VID}" --pid "0x${PID}" --timeout "${TIMEOUT}" --stress "${STRESS}" \
  | tee "${OUT}/04-stress.log"

echo "CCID HIL suite PASS" | tee "${OUT}/summary.log"
echo "OPEN: UART debug capture not wired into this harness (device logs are on DUART)." \
  | tee -a "${OUT}/summary.log"
