"""
Shared serial helpers for Bao tools.
"""

from __future__ import annotations
import sys
import time
import serial
from serial.serialutil import SerialException


def open_serial(port: str, baud: int, *, timeout: float = 0.1, reset: bool = False) -> serial.Serial:
    """
    Open a serial port with consistent options and optional short reset pulse.
    - Sets DTR/RTS low by default (helps avoid unwanted resets/power cuts).
    - If reset=True, briefly toggles DTR/RTS to nudge bootloaders.
    """
    try:
        ser = serial.Serial(port, baud, timeout=timeout)
    except Exception as e:
        print(f"[bao] cannot open {port}: {e}", file=sys.stderr)
        sys.exit(1)

    # default: release control lines
    try:
        ser.dtr = False
        ser.rts = False
    except Exception:
        pass

    if reset:
        try:
            ser.dtr = True
            ser.rts = True
            time.sleep(0.05)
            ser.dtr = False
            ser.rts = False
        except Exception:
            pass

    # clear any pending bytes
    try:
        ser.reset_input_buffer()
        ser.reset_output_buffer()
    except Exception:
        pass

    return ser


def safe_close(ser: serial.Serial | None) -> None:
    """Close a serial port, ignoring errors."""
    if ser is None:
        return
    try:
        ser.close()
    except Exception:
        pass
