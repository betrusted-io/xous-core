import sys
import time
import logging
from serial.serialutil import SerialException
from utils.serial_utils import open_serial, safe_close


def cmd_monitor(args) -> None:
    ser = open_serial(args.port, args.baud, timeout=0.1, reset=getattr(args, "reset", False))
    outf = None
    if getattr(args, "save", None):
        try:
            outf = open(args.save, "a", encoding="utf-8", buffering=1)  # line-buffered
        except Exception as e:
            logging.error(f"[bao] cannot open --save file: {e}")
            safe_close(ser)

            return
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
                    if outf:
                        outf.write(s)
                    sys.stdout.flush()
                consecutive_errors = 0
                time.sleep(0.01)
            except SerialException as e:
                consecutive_errors += 1
                logging.warning(f"[bao] Serial error: {e}. Retrying ({consecutive_errors}/3)...")
                time.sleep(0.25)
                if consecutive_errors >= 3:
                    logging.error("[bao] Giving up. Check that no other program is using the port.")
                    break
    except KeyboardInterrupt:
        pass
    finally:
        try:
            if outf:
                outf.flush()
                outf.close()
        except Exception:
            pass
        safe_close(ser)
