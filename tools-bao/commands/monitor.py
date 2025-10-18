import sys
import time
from serial.serialutil import SerialException
from utils.serial_utils import open_serial, safe_close


def cmd_monitor(args):
    """Simple serial monitor with optional timestamps."""
    ser = open_serial(args.port, args.baud, timeout=0.1, reset=getattr(args, "reset", False))

    print(f"[bao] Monitor {args.port} @ {args.baud} (Ctrl+C to exit)")
    consecutive_errors = 0

    try:
        while True:
            try:
                data = ser.read(4096)
                if data:
                    s = data.decode(errors="replace")
                    if args.ts:
                        ts = time.strftime("%H:%M:%S")
                        s = "".join(f"[{ts}] {line}\n" for line in s.splitlines())
                    sys.stdout.write(s)
                    sys.stdout.flush()
                consecutive_errors = 0
                time.sleep(0.01)
            except SerialException as e:
                consecutive_errors += 1
                print(f"\n[bao] Serial error: {e}. Retrying ({consecutive_errors}/3)...", file=sys.stderr)
                time.sleep(0.25)
                if consecutive_errors >= 3:
                    print("[bao] Giving up. Check that no other program is using the port.", file=sys.stderr)
                    break
    except KeyboardInterrupt:
        pass
    finally:
        safe_close(ser)
