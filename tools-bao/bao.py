"""
Baochip CLI — entry point for host-side utilities.

Usage examples:
  python tools-bao/bao.py ports
  python tools-bao/bao.py monitor -p COM3 -b 115200 --ts
  python tools-bao/bao.py build --target dabao
"""

import argparse
import sys

# Subcommand imports
from commands.ports import cmd_ports
from commands.monitor import cmd_monitor
from commands.flash import cmd_flash
from commands.doctor import cmd_doctor
from commands.build import cmd_build

def main():
    ap = argparse.ArgumentParser(prog="bao.py")
    sub = ap.add_subparsers(dest="cmd", required=True)

    # ports
    s = sub.add_parser("ports", help="List serial ports")
    s.add_argument("-v", "--verbose", action="store_true")
    s.set_defaults(func=cmd_ports)

    # monitor
    m = sub.add_parser("monitor", help="Open a serial monitor")
    m.add_argument("-p", "--port", required=True)
    m.add_argument("-b", "--baud", type=int, default=115200)
    m.add_argument("--ts", action="store_true", help="Show timestamps")
    m.add_argument("--save", help="Append output to a file")
    m.add_argument("--reset", action="store_true", help="Toggle DTR/RTS on open")
    m.set_defaults(func=cmd_monitor)

    # build (Xous via cargo xtask)
    b = sub.add_parser("build", help="Build Xous image for Baochip via cargo xtask")
    b.add_argument("--target", required=True, help="Bao target (e.g. dabao, baosec)")
    b.add_argument("--release", action="store_true", help="Use release build")
    b.add_argument("--extra-args", help="Extra args to pass to cargo xtask")
    b.set_defaults(func=cmd_build)

    # doctor
    d = sub.add_parser("doctor", help="Check Python environment and ports")
    d.set_defaults(func=cmd_doctor)

    # flash (stub)
    f = sub.add_parser("flash", help="Flash a built image (stub)")
    f.add_argument("-p", "--port", required=True)
    f.add_argument("-b", "--baud", type=int, default=115200)
    f.add_argument("--addr", type=lambda x: int(x, 0), default=0x0)
    f.add_argument("--file", help="Binary to flash")
    f.set_defaults(func=cmd_flash)

    args = ap.parse_args()
    try:
        args.func(args)
    except KeyboardInterrupt:
        print("\n[bao] aborted by user.")
        sys.exit(1)
    except Exception as e:
        print(f"[bao] error: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
