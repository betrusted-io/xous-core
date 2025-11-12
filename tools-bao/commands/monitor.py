import sys
import os
import contextlib
import logging
import threading
from serial.serialutil import SerialException
from utils.serial_utils import open_serial, safe_close


@contextlib.contextmanager
def _stdin_raw_noecho():
    """Disable local echo & canonical mode so each keystroke is delivered immediately.
    Works on POSIX and Windows; restores terminal settings on exit."""
    if not sys.stdin.isatty():
        yield
        return

    if os.name == "posix":
        import termios, tty
        fd = sys.stdin.fileno()
        old = termios.tcgetattr(fd)
        try:
            new = termios.tcgetattr(fd)
            # Turn off ECHO and ICANON (line buffering); keep CR as CR (no ICRNL)
            new[3] &= ~(termios.ECHO | termios.ICANON)   # lflags
            new[1] |= termios.OPOST                      # oflags: leave output processing on
            new[0] &= ~termios.ICRNL                     # iflags: don't map CR->NL
            termios.tcsetattr(fd, termios.TCSANOW, new)
            tty.setcbreak(fd)  # per-char reads; Ctrl+C still raises KeyboardInterrupt
            yield
        finally:
            termios.tcsetattr(fd, termios.TCSADRAIN, old)
    else:
        # Windows
        import ctypes
        from ctypes import wintypes
        kernel32 = ctypes.windll.kernel32
        hIn = kernel32.GetStdHandle(-10)  # STD_INPUT_HANDLE
        old_mode = wintypes.DWORD()
        if hIn != ctypes.c_void_p(-1).value and kernel32.GetConsoleMode(hIn, ctypes.byref(old_mode)):
            try:
                new_mode = old_mode.value
                ENABLE_ECHO_INPUT = 0x0004
                ENABLE_LINE_INPUT = 0x0002
                # Keep PROCESSED_INPUT so Ctrl+C works as KeyboardInterrupt
                new_mode &= ~(ENABLE_ECHO_INPUT | ENABLE_LINE_INPUT)
                kernel32.SetConsoleMode(hIn, new_mode)
                yield
            finally:
                kernel32.SetConsoleMode(hIn, old_mode)
        else:
            yield


def _stdin_to_serial(ser, args, stop_event: threading.Event):
    """Forward user input to the serial port (raw: per-byte, line: per-line)."""
    try:
        if getattr(args, "raw", False):
            with _stdin_raw_noecho():
                while not stop_event.is_set():
                    b = sys.stdin.buffer.read(1)
                    if not b:
                        break
                    try:
                        ser.write(b)
                        ser.flush()
                    except SerialException:
                        break
                    # Local echo only if explicitly requested
                    if not getattr(args, "no_echo", False):
                        try:
                            sys.stdout.write(b.decode(errors="replace"))
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
                        sys.stdout.write(line.decode(errors="replace") + ("\r\n" if tx_eol == b"\r\n" else "\n"))
                        sys.stdout.flush()
                    except Exception:
                        pass
    except Exception as e:
        logging.debug(f"[bao] stdin writer thread ended: {e}")
    finally:
        stop_event.set()

def cmd_monitor(args) -> None:
    ser = open_serial(
        args.port,
        args.baud,
        timeout=0.1,
    )
    outf = None
    if getattr(args, "save", None):
        try:
            outf = open(args.save, "a", encoding="utf-8", buffering=1)  # line-buffered
        except Exception as e:
            logging.error(f"[bao] cannot open --save file: {e}")
            safe_close(ser)
            return

    print(f"[bao] Monitor {args.port} @ {args.baud} — interactive (Ctrl+C to exit)")
    mode = "RAW" if getattr(args, "raw", False) else ("LINE CRLF" if getattr(args, "crlf", False) else "LINE LF")
    echo = "OFF" if getattr(args, "no_echo", False) else "ON"
    print(f"[bao] TX:{mode}  Echo:{echo}")

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
                    sys.stdout.write(s)
                    if outf:
                        outf.write(s)
                    sys.stdout.flush()
                consecutive_errors = 0
            except SerialException as e:
                consecutive_errors += 1
                logging.warning(f"[bao] Serial error: {e}. Retrying ({consecutive_errors}/3)...")
                if consecutive_errors >= 3:
                    logging.error("[bao] Giving up. Check that no other program is using the port.")
                    break
            # Small yield to avoid a hot loop when idle
            if not stop_event.is_set():
                import time
                time.sleep(0.01)
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
    m.add_argument("--save", help="Append output to a file")
    m.add_argument("--crlf", action="store_true", help="Use CRLF as TX line ending in line mode (default LF)")
    m.add_argument("--raw", action="store_true", help="Send keystrokes immediately (raw byte mode)")
    m.add_argument("--no-echo", action="store_true", help="Do not locally echo typed input")

    # PuTTY-like defaults for direct CLI use
    m.set_defaults(
        raw=True,      # per-keystroke
        no_echo=True,  # device provides echo if any
        crlf=True,     # Enter sends CRLF in line mode
    )

    m.set_defaults(func=cmd_monitor)