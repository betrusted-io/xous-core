#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Wait until a baosec CCID device is visible on USB.

set -euo pipefail

VID="${CCID_VID:-1d50}"
PID="${CCID_PID:-6198}"
TIMEOUT="${CCID_WAIT_TIMEOUT:-60}"
DEADLINE=$((SECONDS + TIMEOUT))

while (( SECONDS < DEADLINE )); do
  if lsusb -d "${VID}:${PID}" >/dev/null 2>&1; then
    echo "Device ${VID}:${PID} present"
    exit 0
  fi
  sleep 1
done

echo "Timeout waiting for USB device ${VID}:${PID}" >&2
exit 1
