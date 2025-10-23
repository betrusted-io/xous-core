import os
import sys
import shutil
import logging
from pathlib import Path

def _is_dir_writable(p: Path) -> bool:
    try:
        test = p / ".bao_write_test"
        test.write_text("ok", encoding="utf-8")
        test.unlink(missing_ok=True)
        return True
    except Exception:
        return False

def cmd_flash(args) -> None:
    dest = Path(args.dest)
    if not dest.exists() or not dest.is_dir():
        logging.error(f"[bao] destination not found or not a directory: {dest}")
        sys.exit(2)
    if not _is_dir_writable(dest):
        logging.error(f"[bao] destination is not writable: {dest}")
        sys.exit(2)

    files = [Path(f) for f in args.files]
    if not files:
        logging.error("[bao] no files to copy")
        sys.exit(2)

    logging.info(f"[bao] Flash destination: {dest}")
    copied = 0
    for src in files:
        if not src.exists() or not src.is_file():
            logging.warning(f"[bao] skip (not found): {src}")
            continue
        if src.suffix.lower() != ".uf2":
            logging.warning(f"[bao] skip (not a .uf2): {src}")
            continue
        dst = dest / src.name
        logging.info(f"[bao] copy {src} -> {dst}")
        try:
            tmp = dest / f".{src.name}.tmp"
            shutil.copyfile(src, tmp)
            os.replace(tmp, dst)
            copied += 1
        except Exception as e:
            logging.error(f"[bao] copy failed for {src}: {e}")

    if copied == 0:
        logging.error("[bao] nothing copied")
        sys.exit(1)

    print(f"[bao] copied {copied} file(s)")
    sys.exit(0)
