#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Wait for Dabao, confirm EP0, CCID load-test, dump flight ring + 0x43 bulk-TRB stats.
#
# Uses a pyusb GetSlotStatus loop (not gpg). Background-dumps ring (0x45) and
# bulk-TRB stats (0x43) every 1s with a shared UTC timestamp so logs align.
# Stops pcscd before load/dump so libccid does not block EP0.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Pass through PYTHONPATH if set (for user-installed pyusb). Do not hardcode a home path.
PY=(sudo env "PYTHONPATH=${PYTHONPATH:-}" python3)
BG_RING="flight-ring-latest.log"
FINAL_RING="flight-ring.log"
BG_BULK="bulk-trb-trace.log"
BG_PID=""

cleanup() {
  if [[ -n "${BG_PID}" ]] && kill -0 "${BG_PID}" 2>/dev/null; then
    kill "${BG_PID}" 2>/dev/null || true
    wait "${BG_PID}" 2>/dev/null || true
  fi
}
trap cleanup EXIT

release_host_usb() {
  echo "==> Releasing host USB (stop pcscd / scdaemon)"
  sudo systemctl stop pcscd 2>/dev/null || true
  pkill -u "$(id -un)" scdaemon 2>/dev/null || true
  pkill -u "$(id -un)" gpg-agent 2>/dev/null || true
  sleep 1
}

# One-shot: append a 0x43 sample to BULK_LOG with the given ISO timestamp (or now).
fetch_bulk43() {
  local out="$1"
  local ts="${2:-}"
  "${PY[@]}" -c '
import struct, sys
from datetime import datetime, timezone
import usb.core

out_path = sys.argv[1]
ts = sys.argv[2] if len(sys.argv) > 2 and sys.argv[2] else None
if not ts:
    ts = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")

dev = usb.core.find(idVendor=0x1D50, idProduct=0x6197)
if not dev:
    raise SystemExit("no device")
data = bytes(dev.ctrl_transfer(0xC0, 0x43, 0, 0, 32, timeout=2000))
if len(data) < 32:
    raise SystemExit(f"short 0x43 resp: {len(data)}")
out_a, out_c, in_a, in_c, force, seq, out_enq, out_deq, in_enq, in_deq = struct.unpack_from(
    "<IIIIIIHHHH", data, 0
)
line = (
    f"{ts} out_armed={out_a} out_consumed={out_c} in_armed={in_a} in_consumed={in_c} "
    f"force_prime={force} seq={seq} "
    f"out_enq={out_enq} out_deq={out_deq} in_enq={in_enq} in_deq={in_deq} "
    f"out_depth={out_a - out_c} in_depth={in_a - in_c}\n"
)
with open(out_path, "a", encoding="utf-8") as fh:
    fh.write(line)
print(line, end="")
' "$out" "$ts"
}

dump_pair() {
  # Shared timestamp for ring + 0x43 so post-mortem lines align.
  local ts
  ts="$("${PY[@]}" -c 'from datetime import datetime, timezone; print(datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"))')"
  local ring_out="$1"
  local bulk_out="$2"
  "${PY[@]}" tools/ring_buffer_dump.py -o "${ring_out}.tmp" >/dev/null 2>&1 \
    && mv -f "${ring_out}.tmp" "${ring_out}" \
    || return 1
  # Stamp the ring header line is already inside the dump; also note ts in bulk log.
  fetch_bulk43 "$bulk_out" "$ts" >/dev/null 2>&1 || return 1
  return 0
}

echo "==> Waiting for USB 1d50:6197 ..."
ok=0
for _ in $(seq 1 30); do
  if lsusb -d 1d50:6197 >/dev/null 2>&1; then
    ok=1
    break
  fi
  sleep 1
done
if [[ "$ok" -ne 1 ]]; then
  echo "No device 1d50:6197" >&2
  exit 1
fi
lsusb -d 1d50:6197

echo "==> Waiting for EP0 vendor 0x42 ..."
"${PY[@]}" -c '
import sys, time, usb.core
for i in range(30):
    dev = usb.core.find(idVendor=0x1D50, idProduct=0x6197)
    if not dev:
        time.sleep(1)
        continue
    try:
        data = bytes(dev.ctrl_transfer(0xC0, 0x42, 0, 0, 16, timeout=2000))
        print("EP0 OK", data.hex())
        sys.exit(0)
    except Exception as e:
        print(f"try {i+1}: {type(e).__name__}: {e}")
        time.sleep(1)
print("EP0 never answered", file=sys.stderr)
sys.exit(1)
' || exit 1

release_host_usb

: > "${BG_BULK}"
echo "==> Background ring(0x45)+bulk(0x43) dump every 1s -> ${BG_RING} / ${BG_BULK}"
(
  while true; do
    dump_pair "${BG_RING}" "${BG_BULK}" || true
    sleep 1
  done
) &
BG_PID=$!

echo "==> Load test: pyusb CCID GetSlotStatus loop (up to 120s)"
"${PY[@]}" -c '
import sys, time, usb.core, usb.util

VID, PID = 0x1D50, 0x6197
dev = usb.core.find(idVendor=VID, idProduct=PID)
if not dev:
    print("no device", file=sys.stderr)
    sys.exit(1)

for cfg in dev:
    for intf in cfg:
        num = intf.bInterfaceNumber
        if dev.is_kernel_driver_active(num):
            try:
                dev.detach_kernel_driver(num)
            except Exception as e:
                print(f"detach iface {num}: {e}")

dev.set_configuration()
cfg = dev.get_active_configuration()

ccid = None
for intf in cfg:
    if intf.bInterfaceClass == 0x0B:
        ccid = intf
        break
if ccid is None:
    print("no CCID interface", file=sys.stderr)
    sys.exit(1)

usb.util.claim_interface(dev, ccid.bInterfaceNumber)

ep_out = ep_in = None
for ep in ccid:
    addr = ep.bEndpointAddress
    if usb.util.endpoint_direction(addr) == usb.util.ENDPOINT_OUT:
        ep_out = addr
    else:
        ep_in = addr
if ep_out is None or ep_in is None:
    print("missing bulk endpoints", file=sys.stderr)
    sys.exit(1)

def get_slot_status(seq: int) -> bytes:
    return bytes([0x65, 0, 0, 0, 0, 0, seq & 0xFF, 0, 0, 0])

deadline = time.time() + 120
n = 0
try:
    while time.time() < deadline:
        n += 1
        try:
            dev.write(ep_out, get_slot_status(n), timeout=2000)
            data = bytes(dev.read(ep_in, 64, timeout=2000))
            if n == 1 or n % 20 == 0:
                print(f"ok {n} len={len(data)} head={data[:8].hex()}")
        except Exception as e:
            print(f"FAIL at {n}: {type(e).__name__}: {e}")
            sys.exit(2)
        time.sleep(0.05)
    print(f"completed {n} iterations without failure")
    sys.exit(0)
finally:
    try:
        usb.util.release_interface(dev, ccid.bInterfaceNumber)
    except Exception:
        pass
'
load_rc=$?
echo "Load test exit code: ${load_rc}"

cleanup
BG_PID=""

release_host_usb

echo "==> Final dump -> ${FINAL_RING} + append ${BG_BULK}"
if dump_pair "${FINAL_RING}" "${BG_BULK}"; then
  echo "==> Final dump OK"
  echo "--- last bulk-trb line ---"
  tail -n 1 "${BG_BULK}" || true
else
  echo "==> Final dump FAILED (EP0 likely dead)."
  if [[ -f "${BG_RING}" ]]; then
    echo "    Using last background ring snapshot: ${BG_RING}"
    cp -f "${BG_RING}" "${FINAL_RING}"
    head -n 1 "${FINAL_RING}"
    echo "    ... (tail) ..."
    tail -n 50 "${FINAL_RING}"
    echo "--- last bulk-trb lines ---"
    tail -n 5 "${BG_BULK}" || true
  else
    echo "    No background snapshot either."
    exit 1
  fi
fi
echo "==> Done (load_rc=${load_rc})"
