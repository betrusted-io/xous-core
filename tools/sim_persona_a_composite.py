#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Simulate Persona A host-side composite asserts without USB hardware."""

from __future__ import annotations

import sys
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parent / "ccid_hil"))

from ccid_usb import (  # noqa: E402
    PERSONA_A_EXPECTED_NON_EP0,
    USB_CLASS_CCID,
    USB_CLASS_CDC_COMM,
    USB_CLASS_CDC_DATA,
    USB_CLASS_HID,
    assert_persona_a_composite,
)


class FakeEp:
    def __init__(self, addr: int):
        self.bEndpointAddress = addr


class FakeIface:
    def __init__(self, number: int, cls: int, ep_addrs):
        self.bInterfaceNumber = number
        self.bInterfaceClass = cls
        self.bInterfaceSubClass = 0
        self.bInterfaceProtocol = 0
        self._eps = [FakeEp(a) for a in ep_addrs]

    def __iter__(self):
        return iter(self._eps)


class FakeCfg:
    def __init__(self, ifaces):
        self._ifaces = ifaces

    def __iter__(self):
        return iter(self._ifaces)


class FakeDev:
    def __init__(self, ifaces):
        self._cfg = FakeCfg(ifaces)

    def get_active_configuration(self):
        return self._cfg


def good_persona_a():
    return FakeDev(
        [
            FakeIface(0, USB_CLASS_CCID, [0x01, 0x81, 0x82]),
            FakeIface(1, USB_CLASS_HID, [0x83, 0x02]),
            FakeIface(2, USB_CLASS_HID, [0x84, 0x03]),
        ]
    )


def with_cdc():
    return FakeDev(
        [
            FakeIface(0, USB_CLASS_HID, [0x81, 0x01]),
            FakeIface(1, USB_CLASS_HID, [0x82, 0x02]),
            FakeIface(2, USB_CLASS_CCID, [0x03, 0x83, 0x84]),
            FakeIface(3, USB_CLASS_CDC_COMM, [0x85]),
            FakeIface(4, USB_CLASS_CDC_DATA, [0x04, 0x86]),
        ]
    )


def main() -> int:
    layout = assert_persona_a_composite(good_persona_a())
    assert layout["non_ep0_endpoints"] == PERSONA_A_EXPECTED_NON_EP0
    assert not layout["cdc_interfaces"]
    print(f"sim PASS: good Persona A layout {layout}")

    try:
        assert_persona_a_composite(with_cdc())
    except RuntimeError as exc:
        print(f"sim PASS: CDC layout rejected ({exc})")
    else:
        print("sim FAIL: CDC layout should have been rejected", file=sys.stderr)
        return 1

    print("sim OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
