import sys, shutil
from pathlib import Path

def _is_dir_writable(p: Path) -> bool:
    try:
        test = p / ".bao_write_test"
        test.write_text("ok", encoding="utf-8")
        test.unlink(missing_ok=True)
        return True
    except Exception:
        return False

def cmd_flash(args):
    dest = Path(args.dest)
    if not dest.exists() or not dest.is_dir():
        print(f"[bao] destination not found or not a directory: {dest}", file=sys.stderr)
        sys.exit(2)
    if not _is_dir_writable(dest):
        print(f"[bao] destination is not writable: {dest}", file=sys.stderr)
        sys.exit(2)

    files = [Path(f) for f in args.files]
    if not files:
        print("[bao] no files to copy", file=sys.stderr)
        sys.exit(2)

    print(f"[bao] Flash destination: {dest}")
    copied = 0
    for src in files:
        if not src.exists() or not src.is_file():
            print(f"[bao] skip (not found): {src}", file=sys.stderr)
            continue
        dst = dest / src.name
        print(f"[bao] copy {src} -> {dst}")
        try:
            shutil.copyfile(src, dst)
            copied += 1
        except Exception as e:
            print(f"[bao] copy failed for {src}: {e}", file=sys.stderr)

    if copied == 0:
        print("[bao] nothing copied", file=sys.stderr)
        sys.exit(1)

    print(f"[bao] copied {copied} file(s)")
    sys.exit(0)
