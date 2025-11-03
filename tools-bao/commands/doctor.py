import sys
import platform
from serial.tools import list_ports
import serial

def cmd_doctor(_args):
    try:
        ports = list(list_ports.comports())
        print(f"[bao] Python {platform.python_version()} OK; pyserial {serial.__version__} OK; ports found: {len(ports)}")
        for p in ports:
            print(f" - {p.device} {p.description}")
        if not ports and sys.platform != "win32":
            print("   Hint: On Linux/macOS you may need to add your user to the dialout/uucp group or adjust permissions.")
    except Exception as e:
        print(f"[bao] Doctor failed: {e}")


def register(subparsers) -> None:
    d = subparsers.add_parser("doctor", help="Check Python environment and ports")
    d.set_defaults(func=cmd_doctor)