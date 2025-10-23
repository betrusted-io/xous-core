import re
import time
import logging
from pathlib import Path
from typing import Optional, Tuple
import serial


BOOKEND_START: str = "_|TT|_"
BOOKEND_END: str = "_|TE|_"

SEMVER_RE = re.compile(r'pub\s+const\s+SEMVER\s*:\s*&\'static\s+str\s*=\s*"([^"]+)"')
TS_RE     = re.compile(r'pub\s+const\s+TIMESTAMP\s*:\s*&\'static\s+str\s*=\s*"([^"]+)"')

def read_local_versions(repo_root: Path) -> Tuple[Optional[str], Optional[str]]:
    f = repo_root / "services" / "xous-ticktimer" / "src" / "version.rs"
    if not f.exists():
        logging.warning(f"[bao] version.rs not found: {f}")
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

def query_board_versions(port: str, baud: int = 115200, timeout_s: float = 3.0, cmd: bytes = b"ver xous\r\n") -> Tuple[Optional[str], Optional[str]]:
    try:
        ser = serial.Serial(port, baud, timeout=0.2)
    except Exception as e:
        logging.error(f"[bao] cannot open {port}: {e}")
        return None, None

    try:
        with ser:
            try:
                ser.reset_input_buffer()
                ser.reset_output_buffer()
            except Exception:
                pass

            ser.write(cmd)
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
        pass

    buf = "".join(chunks) if chunks else ""
    return parse_board_version_blob(buf)