import re, time, sys
from pathlib import Path
from typing import Optional, Tuple
import serial

BOOKEND_START = "_|TT|_"
BOOKEND_END   = "_|TE|_"

SEMVER_RE = re.compile(r'pub\s+const\s+SEMVER\s*:\s*&\'static\s+str\s*=\s*"([^"]+)"')
TS_RE     = re.compile(r'pub\s+const\s+TIMESTAMP\s*:\s*&\'static\s+str\s*=\s*"([^"]+)"')

def read_local_versions(repo_root: Path) -> Tuple[Optional[str], Optional[str]]:
    """
    Returns (semver, timestamp) parsed from services/xous-ticktimer/src/version.rs.
    """
    f = repo_root / "services" / "xous-ticktimer" / "src" / "version.rs"
    if not f.exists():
        return None, None
    text = f.read_text(encoding="utf-8", errors="ignore")
    semver = (SEMVER_RE.search(text).group(1).strip()
              if SEMVER_RE.search(text) else None)
    ts = (TS_RE.search(text).group(1).strip()
          if TS_RE.search(text) else None)
    return semver, ts

def _between_bookends(buf: str) -> Optional[str]:
    start_idx = buf.find(BOOKEND_START)
    if start_idx == -1:
        return None
    end_idx = buf.find(BOOKEND_END, start_idx + len(BOOKEND_START))
    if end_idx == -1:
        return None
    return buf[start_idx + len(BOOKEND_START): end_idx]

def parse_board_version_blob(buf: str) -> Tuple[Optional[str], Optional[str]]:
    """
    Given full serial output, extract the substring between bookends and split it
    into (semver, timestamp), matching get_version() which returns:
        SEMVER + "\\n" + TIMESTAMP
    Returns (semver, timestamp) or (None, None) if not found.
    """
    blob = _between_bookends(buf)
    if blob is None:
        return None, None
    # normalize line endings and whitespace
    blob = blob.replace("\r\n", "\n").replace("\r", "\n").strip()
    # Expect two lines: first SEMVER, second TIMESTAMP (but be tolerant)
    lines = [ln.strip() for ln in blob.split("\n") if ln.strip() != ""]
    if not lines:
        return None, None
    semver = lines[0]
    ts = lines[1] if len(lines) > 1 else None
    return semver, ts

def query_board_versions(port: str, baud: int = 115200, timeout_s: float = 3.0) -> Tuple[Optional[str], Optional[str]]:
    """
    Open the serial port, send 'ver xous', read for a short window, and return
    (semver, timestamp) parsed from the bookended payload.
    """
    try:
        ser = serial.Serial(port, baud, timeout=0.2)
    except Exception as e:
        print(f"[bao] cannot open {port}: {e}", file=sys.stderr)
        return None, None

    try:
        try:
            ser.reset_input_buffer()
            ser.reset_output_buffer()
        except Exception:
            pass

        ser.write(b"ver xous\r\n")
        ser.flush()

        deadline = time.time() + timeout_s
        chunks = []
        while time.time() < deadline:
            try:
                data = ser.read(4096)
                if data:
                    chunks.append(data.decode(errors="replace"))
                    joined = "".join(chunks)
                    if BOOKEND_START in joined and BOOKEND_END in joined:
                        break
            except Exception:
                break
            time.sleep(0.02)
    finally:
        try:
            ser.close()
        except Exception:
            pass

    buf = "".join(chunks) if chunks else ""
    return parse_board_version_blob(buf)