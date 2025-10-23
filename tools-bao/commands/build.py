import subprocess
import sys
import logging
from utils.common import project_root

def cmd_build(args) -> None:
    """
    Run cargo xtask <target> to build a Xous image for Baochip.
    Exits with the cargo process exit code on failure.
    """
    target = args.target
    cargo_cmd = ["cargo", "xtask", target]
    if args.release:
        cargo_cmd.append("--release")
    if args.extra_args:
        cargo_cmd += args.extra_args.split()

    logging.info(f"[bao] Running: {' '.join(cargo_cmd)}")
    proc = subprocess.run(cargo_cmd, cwd=project_root())
    if proc.returncode == 0:
        print(f"[bao] Build for target '{target}' complete.")
    else:
        logging.error(f"[bao] Build failed with exit code {proc.returncode}")
        sys.exit(proc.returncode)
