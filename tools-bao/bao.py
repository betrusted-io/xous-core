import argparse
import sys
import logging
import traceback

# Subcommand imports
from commands.ports import cmd_ports
from commands.monitor import cmd_monitor
from commands.flash import cmd_flash
from commands.doctor import cmd_doctor
from commands.artifacts import cmd_artifacts

VERSION = "0.1.1"

def main():
    ap = argparse.ArgumentParser(
        prog="bao.py",
        description="Baochip CLI — host-side utilities for Baochip development."
    )
    ap.add_argument("--version", action="version", version=f"%(prog)s {VERSION}", help="Show version and exit.")
    ap.add_argument("-v", "--verbose", action="store_true", help="Enable verbose output (debug logging and tracebacks)")
    sub = ap.add_subparsers(dest="cmd", required=True)

    # artifacts
    a = sub.add_parser("artifacts", help="List newest UF2 images (release only)")
    a.add_argument("--json", action="store_true")
    a.set_defaults(func=cmd_artifacts)

    # ports
    s = sub.add_parser("ports", help="List serial ports")
    s.set_defaults(func=cmd_ports)

    # monitor
    m = sub.add_parser("monitor", help="Open a serial monitor")
    m.add_argument("-p", "--port", required=True, help="Serial port (e.g., COM5, /dev/ttyUSB0)")
    m.add_argument("-b", "--baud", type=int, default=115200, help="Baud rate")
    m.add_argument("--ts", action="store_true", help="Show timestamps on received lines")
    m.add_argument("--save", help="Append output to a file")
    m.add_argument("--reset", action="store_true", help="Toggle DTR/RTS on open")
    m.add_argument("--crlf", action="store_true", help="Use CRLF as TX line ending in line mode (default LF)")
    m.add_argument("--raw", action="store_true", help="Send keystrokes immediately (raw mode)")
    m.add_argument("--no-echo", action="store_true", help="Do not locally echo typed input")
    m.add_argument("--rtscts",  action="store_true", help="Enable RTS/CTS hardware flow control")
    m.add_argument("--xonxoff", action="store_true", help="Enable XON/XOFF software flow control")
    m.add_argument("--dsrdtr",  action="store_true", help="Enable DSR/DTR hardware flow control")
    m.set_defaults(func=cmd_monitor)

    # doctor
    d = sub.add_parser("doctor", help="Check Python environment and ports")
    d.set_defaults(func=cmd_doctor)

    # flash
    f = sub.add_parser("flash", help="Copy UF2 file(s) to a mounted drive")
    f.add_argument("--dest", required=True, help="Mount path of the UF2 boot drive (e.g., D:\\)")
    f.add_argument("files", nargs="+", help="One or more UF2 files to copy (e.g., loader.uf2 xous.uf2 app.uf2)")
    f.set_defaults(func=cmd_flash)

    args = ap.parse_args()

    log_level = logging.DEBUG if getattr(args, "verbose", False) else logging.WARNING
    logging.basicConfig(level=log_level, format="[bao] %(levelname)s: %(message)s")

    try:
        args.func(args)
    except KeyboardInterrupt:
        print("\n[bao] aborted by user.")
        sys.exit(1)
    except Exception as e:
        if getattr(args, "verbose", False):
            print(f"[bao] error: {e}", file=sys.stderr)
            traceback.print_exc()
        else:
            print(f"[bao] error: {e}", file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    main()
