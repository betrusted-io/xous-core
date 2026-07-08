#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""CCID USB helpers for host-side transport smoke and HIL tests."""

from __future__ import annotations

import struct
import time
from dataclasses import dataclass
from typing import Optional, Tuple

try:
    import usb.core
    import usb.util
except ImportError as exc:
    raise SystemExit("pyusb is required: pip install pyusb") from exc

BAOSEC_VID = 0x1D50
BAOSEC_PID = 0x6198
DABAO_PID = 0x6197
USB_CLASS_CCID = 0x0B
CCID_HEADER_LEN = 10


def make_get_slot_status(seq: int = 0) -> bytes:
    frame = bytearray(CCID_HEADER_LEN)
    frame[0] = 0x65  # PC_to_RDR_GetSlotStatus
    frame[5] = 0
    frame[6] = seq & 0xFF
    return bytes(frame)


def make_xfr_block(seq: int, payload: bytes) -> bytes:
    if len(payload) > 520:
        raise ValueError("payload too large for CCID wire limit")
    header = bytearray(CCID_HEADER_LEN)
    header[0] = 0x6F  # PC_to_RDR_XfrBlock
    struct.pack_into("<I", header, 1, len(payload))
    header[5] = 0
    header[6] = seq & 0xFF
    return bytes(header) + payload


@dataclass
class CcidEndpoints:
    device: "usb.core.Device"
    configuration: int
    interface: int
    ep_out: int
    ep_in: int


def _ccid_interface(cfg) -> Optional[Tuple[int, object]]:
    for intf in cfg:
        if intf.bInterfaceClass == USB_CLASS_CCID:
            return intf.bInterfaceNumber, intf
    return None


def find_ccid_device(
    vid: int = BAOSEC_VID,
    pid: int = BAOSEC_PID,
    timeout_s: float = 30.0,
) -> CcidEndpoints:
  deadline = time.monotonic() + timeout_s
  last_err: Optional[str] = None
  while time.monotonic() < deadline:
    dev = usb.core.find(idVendor=vid, idProduct=pid)
    if dev is None:
      time.sleep(0.5)
      continue
    try:
      if dev.is_kernel_driver_active(0):
        try:
          dev.detach_kernel_driver(0)
        except (usb.core.USBError, NotImplementedError):
          pass
      dev.set_configuration()
      cfg = dev.get_active_configuration()
      found = _ccid_interface(cfg)
      if found is None:
        last_err = "CCID interface (class 0x0B) not found"
        time.sleep(0.5)
        continue
      if_num, intf = found
      usb.util.claim_interface(dev, if_num)
      ep_out = usb.util.find_descriptor(
        intf,
        custom_match=lambda e: usb.util.endpoint_direction(e.bEndpointAddress)
        == usb.util.ENDPOINT_OUT
        and usb.util.endpoint_type(e.bmAttributes) == usb.util.ENDPOINT_TYPE_BULK,
      )
      ep_in = usb.util.find_descriptor(
        intf,
        custom_match=lambda e: usb.util.endpoint_direction(e.bEndpointAddress)
        == usb.util.ENDPOINT_IN
        and usb.util.endpoint_type(e.bmAttributes) == usb.util.ENDPOINT_TYPE_BULK,
      )
      if ep_out is None or ep_in is None:
        last_err = "CCID bulk endpoints not found"
        time.sleep(0.5)
        continue
      return CcidEndpoints(
        device=dev,
        configuration=cfg.bConfigurationValue,
        interface=if_num,
        ep_out=ep_out.bEndpointAddress,
        ep_in=ep_in.bEndpointAddress,
      )
    except usb.core.USBError as err:
      last_err = str(err)
      time.sleep(0.5)
  raise TimeoutError(
    f"CCID device {vid:04x}:{pid:04x} not ready within {timeout_s}s"
    + (f" (last error: {last_err})" if last_err else "")
  )


def ccid_bulk_roundtrip(
    eps: CcidEndpoints,
    frame: bytes,
    read_timeout_ms: int = 5000,
) -> bytes:
  eps.device.write(eps.ep_out, frame, timeout=read_timeout_ms)
  return bytes(eps.device.read(eps.ep_in, 512, timeout=read_timeout_ms))


def verify_ccid_descriptor(dev: "usb.core.Device") -> dict:
  cfg = dev.get_active_configuration()
  found = _ccid_interface(cfg)
  if found is None:
    raise RuntimeError("CCID interface not found")
  _, intf = found
  ccid_desc = None
  for extra in intf:
    if extra.bDescriptorType == 0x21:
      ccid_desc = extra
      break
  if ccid_desc is None:
    raise RuntimeError("CCID functional descriptor (0x21) not found")
  raw = bytes(ccid_desc)
  bcd_ccid = struct.unpack_from("<H", raw, 2)[0]
  dw_protocols = struct.unpack_from("<I", raw, 6)[0]
  max_msg = struct.unpack_from("<I", raw, 42)[0]
  return {
    "bcd_ccid": bcd_ccid,
    "dw_protocols": dw_protocols,
    "max_message_length": max_msg,
  }
