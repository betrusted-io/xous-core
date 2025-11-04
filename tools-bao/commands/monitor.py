import sys
import time
import logging
import threading
from serial.serialutil import SerialException
from utils.serial_utils import open_serial, safe_close

def _stdin_to_serial(ser, args, stop_event: threading.Event):
    try:
        if getattr(args, "raw", False):
            # Raw mode: read larger chunks for smooth pastes when available
            while not stop_event.is_set():
                chunk = (
                    sys.stdin.buffer.read1(4096)
                    if hasattr(sys.stdin.buffer, "read1")
                    else sys.stdin.buffer.read(1)
                )
                if not chunk:
                    break  # EOF
                try:
                    ser.write(chunk)
                    ser.flush()
                except SerialException:
                    break
                if not getattr(args, "no_echo", False):
                    # Local echo (avoid double-echo if target already echoes)
                    try:
                        sys.stdout.write(chunk.decode(errors="replace"))
                        sys.stdout.flush()
                    except Exception:
                        pass
        else:
            # Line mode: read a full line, normalize line ending
            tx_eol = b"\r\n" if getattr(args, "crlf", False) else b"\n"
            while not stop_event.is_set():
                line = sys.stdin.buffer.readline()
                if not line:
                    break  # EOF
                # Strip any trailing \r or \n to avoid doubling endings
                line = line.rstrip(b"\r\n")
                payload = line + tx_eol
                try:
                    ser.write(payload)
                    ser.flush()
                except SerialException:
                    break
                if not getattr(args, "no_echo", False):
                    try:
                        # Echo what we sent, as a single line locally
                        sys.stdout.write(line.decode(errors="replace") + ("\r\n" if tx_eol == b"\r\n" else "\n"))
                        sys.stdout.flush()
                    except Exception:
                        pass
    except Exception as e:
        logging.debug(f"[bao] stdin writer thread ended: {e}")
    finally:
        stop_event.set()

def cmd_monitor(args) -> None:
    # Open with flow-control / write-timeout if provided
    ser = open_serial(
        args.port,
        args.baud,
        timeout=0.1,
        reset=getattr(args, "reset", False),
        rtscts=getattr(args, "rtscts", False),
        xonxoff=getattr(args, "xonxoff", False),
        dsrdtr=getattr(args, "dsrdtr", False),
        write_timeout=getattr(args, "write_timeout", 1.0),
    )
    outf = None
    if getattr(args, "save", None):
        try:
            outf = open(args.save, "a", encoding="utf-8", buffering=1)  # line-buffered
        except Exception as e:
            logging.error(f"[bao] cannot open --save file: {e}")
            safe_close(ser)
            return

    # Initial line states (optional)
    if getattr(args, "dtr", None) is not None:
        try:
            ser.dtr = bool(args.dtr)
        except Exception:
            pass
    if getattr(args, "rts", None) is not None:
        try:
            ser.rts = bool(args.rts)
        except Exception:
            pass

    # Optional flush on connect
    if not getattr(args, "no_flush", False):
        try:
            ser.reset_input_buffer()
            ser.reset_output_buffer()
        except Exception:
            pass

    # Optional BREAK at start
    if getattr(args, "break_ms", 0) > 0:
        try:
            ser.send_break(duration=args.break_ms / 1000.0)
        except Exception:
            pass

    print(f"[bao] Monitor {args.port} @ {args.baud} — interactive (Ctrl+C to exit)")
    mode = "RAW" if getattr(args, "raw", False) else ("LINE CRLF" if getattr(args, "crlf", False) else "LINE LF")
    echo = "OFF" if getattr(args, "no_echo", False) else "ON"
    ts   = "ON" if getattr(args, "ts", False) else "OFF"
    fc   = ",".join(n for n, on in [
        ("RTS/CTS", getattr(args, "rtscts", False)),
        ("XON/XOFF", getattr(args, "xonxoff", False)),
        ("DSR/DTR", getattr(args, "dsrdtr", False)),
    ] if on) or "none"
    print(f"[bao] RX ts:{ts}  TX:{mode}  Echo:{echo}  Flow:{fc}")

    consecutive_errors = 0
    stop_event = threading.Event()

    # Start stdin→serial writer thread
    writer = threading.Thread(target=_stdin_to_serial, args=(ser, args, stop_event), daemon=True)
    writer.start()

    try:
        while not stop_event.is_set():
            try:
                data = ser.read(4096)
                if data:
                    s = data.decode(errors="replace")
                    if getattr(args, "ts", False):
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
            stop_event.set()
            writer.join(timeout=0.5)
        except Exception:
            pass
        try:
            if outf:
                outf.flush()
                outf.close()
        except Exception:
            pass
        safe_close(ser)


def register(subparsers) -> None:
    m = subparsers.add_parser("monitor", help="Open a serial monitor")
    m.add_argument("-p", "--port", required=True, help="Serial port (e.g., COM5, /dev/ttyUSB0)")
    m.add_argument("-b", "--baud", type=int, default=1000000, help="Baud rate")
    m.add_argument("--ts", action="store_true", help="Show timestamps on received lines")
    m.add_argument("--save", help="Append output to a file")
    m.add_argument("--reset", action="store_true", help="Toggle DTR/RTS on open")
    m.add_argument("--crlf", action="store_true", help="Use CRLF as TX line ending in line mode (default LF)")
    m.add_argument("--raw", action="store_true", help="Send keystrokes immediately (raw byte mode)")
    m.add_argument("--no-echo", action="store_true", help="Do not locally echo typed input")
    m.add_argument("--rtscts",  action="store_true", help="Enable RTS/CTS hardware flow control")
    m.add_argument("--xonxoff", action="store_true", help="Enable XON/XOFF software flow control")
    m.add_argument("--dsrdtr",  action="store_true", help="Enable DSR/DTR hardware flow control")
    m.add_argument("--write-timeout", type=float, default=1.0, help="Write timeout in seconds")
    m.add_argument("--dtr", type=int, choices=[0,1], help="Initial DTR level (0/1)")
    m.add_argument("--rts", type=int, choices=[0,1], help="Initial RTS level (0/1)")
    m.add_argument("--no-flush", action="store_true", help="Do not flush buffers on connect")
    m.add_argument("--break-ms", type=int, default=0, help="Send BREAK for N milliseconds after opening")
    m.set_defaults(func=cmd_monitor)