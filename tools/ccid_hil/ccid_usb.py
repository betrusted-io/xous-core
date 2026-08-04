#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""CCID USB helpers for host-side transport smoke and HIL tests."""

from __future__ import annotations

import struct
import time
from dataclasses import dataclass
from typing import List, Optional, Tuple

try:
    import usb.core
    import usb.util

    _PYUSB_ERR: Optional[ImportError] = None
except ImportError as exc:  # allow sim/tooling without pyusb for layout asserts
    usb = None  # type: ignore
    _PYUSB_ERR = exc


def _require_pyusb() -> None:
    if _PYUSB_ERR is not None:
        raise SystemExit("pyusb is required: pip install pyusb") from _PYUSB_ERR

BAOSEC_VID = 0x1D50
BAOSEC_PID = 0x6198
DABAO_PID = 0x6197
USB_CLASS_CCID = 0x0B
USB_CLASS_HID = 0x03
USB_CLASS_CDC_COMM = 0x02
USB_CLASS_CDC_DATA = 0x0A
CCID_HEADER_LEN = 10

# Persona A expected unidirectional non-EP0 slots on Corigine (CRG_EP_NUM=8).
# Per class (see tools/check_ep_budget.py and source cites therein):
#   CCID 3, FIDO 2, NKRO 2 => 7; no CDC.
PERSONA_A_EXPECTED_NON_EP0 = 7
PERSONA_A_MAX_NON_EP0 = 8


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
  _require_pyusb()
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
  # USB CCID 1.1: dwMaxCCIDMessageLength is at offset 44 (after bNumClockSupported /
  # bNumDataRatesSupported). Older firmware omitted those bytes and used offset 42.
  max_msg = struct.unpack_from("<I", raw, 44)[0]
  return {
    "bcd_ccid": bcd_ccid,
    "dw_protocols": dw_protocols,
    "max_message_length": max_msg,
  }


def list_cdc_interfaces(dev: "usb.core.Device") -> List[dict]:
  """Return CDC COMM/DATA interfaces for the active configuration (should be empty on Persona A)."""
  cfg = dev.get_active_configuration()
  out: List[dict] = []
  for intf in cfg:
    cls = intf.bInterfaceClass
    if cls in (USB_CLASS_CDC_COMM, USB_CLASS_CDC_DATA):
      out.append(
        {
          "number": intf.bInterfaceNumber,
          "class": cls,
          "subclass": intf.bInterfaceSubClass,
          "protocol": intf.bInterfaceProtocol,
        }
      )
  return out


def summarize_composite(dev: "usb.core.Device") -> dict:
  """Count interfaces and non-EP0 endpoints on the active configuration."""
  cfg = dev.get_active_configuration()
  by_class: dict = {}
  non_ep0 = 0
  hid_ifaces = 0
  ccid_ifaces = 0
  for intf in cfg:
    cls = intf.bInterfaceClass
    by_class[cls] = by_class.get(cls, 0) + 1
    if cls == USB_CLASS_HID:
      hid_ifaces += 1
    if cls == USB_CLASS_CCID:
      ccid_ifaces += 1
    for ep in intf:
      if (ep.bEndpointAddress & 0x0F) != 0:
        non_ep0 += 1
  return {
    "by_class": by_class,
    "hid_interfaces": hid_ifaces,
    "ccid_interfaces": ccid_ifaces,
    "cdc_interfaces": list_cdc_interfaces(dev),
    "non_ep0_endpoints": non_ep0,
  }


def assert_persona_a_composite(dev: "usb.core.Device") -> dict:
  """Fail if layout is incompatible with Persona A CCID images (no CDC; CCID+HID; EP budget).

  Expected roughly: CCID (0x0B) + two HID (FIDO+NKRO), unidirectional non-EP0 == 7, none > 8.
  Interface indices are not hardcoded — CDC removal can shift numbers.
  """
  layout = summarize_composite(dev)
  if layout["cdc_interfaces"]:
    raise RuntimeError(
      f"Persona A violation: CDC interface(s) present {layout['cdc_interfaces']}"
    )
  if layout["ccid_interfaces"] < 1:
    raise RuntimeError("Persona A: expected at least one CCID interface (0x0B)")
  if layout["hid_interfaces"] < 2:
    raise RuntimeError(
      f"Persona A: expected >=2 HID interfaces (FIDO+NKRO), got {layout['hid_interfaces']}"
    )
  n = layout["non_ep0_endpoints"]
  if n > PERSONA_A_MAX_NON_EP0:
    raise RuntimeError(
      f"Persona A: non-EP0 endpoint count {n} exceeds Corigine CRG_EP_NUM={PERSONA_A_MAX_NON_EP0}"
    )
  if n != PERSONA_A_EXPECTED_NON_EP0:
    # Soft mismatch: some hosts count differently; still fail so HIL catches drift.
    raise RuntimeError(
      f"Persona A: expected {PERSONA_A_EXPECTED_NON_EP0} non-EP0 endpoints "
      f"(CCID3+FIDO2+NKRO2), got {n}; layout={layout}"
    )
  return layout
