import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]  # xous-core root (tools-bao -> xous-core)

def cmd_build(args):
    """Run cargo xtask <target> to build a Xous image for Baochip."""
    target = args.target
    cargo_cmd = ["cargo", "xtask", target]
    if args.release:
        cargo_cmd.append("--release")
    if args.extra_args:
        cargo_cmd += args.extra_args.split()

    print(f"[bao] Running: {' '.join(cargo_cmd)}")
    try:
        subprocess.run(cargo_cmd, cwd=ROOT, check=True)
        print(f"[bao] Build for target '{target}' complete.")
    except subprocess.CalledProcessError as e:
        print(f"[bao] Build failed with exit code {e.returncode}", file=sys.stderr)
        sys.exit(e.returncode)
