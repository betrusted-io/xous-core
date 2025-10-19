"""
Return release UF2 images for flashing:

<xous-core>/target/riscv32imac-unknown-xous-elf/release/{loader.uf2,xous.uf2,app.uf2}

JSON:
  { "images": [
      { "path": ".../loader.uf2", "role": "loader" },
      { "path": ".../xous.uf2",   "role": "xous"   },
      { "path": ".../app.uf2",    "role": "app"    }
    ] }

Only files that actually exist are included.
"""

import json
import sys
from pathlib import Path
from typing import List, Dict

TRIPLE = "riscv32imac-unknown-xous-elf"
FILENAMES = [
    ("loader.uf2", "loader"),
    ("xous.uf2",   "xous"),
    ("app.uf2",    "app"),
]

def cmd_artifacts(args):
    root = Path(__file__).resolve().parents[2]
    release_dir = root / "target" / TRIPLE / "release"

    images: List[Dict[str, str]] = []
    if not release_dir.exists():
        # No error: just report empty so callers can prompt “build first”
        print(json.dumps({"images": images}) if getattr(args, "json", False)
              else f"[bao] release dir not found: {release_dir}")
        sys.exit(0)

    for fname, role in FILENAMES:
        p = release_dir / fname
        if p.exists() and p.is_file():
            images.append({"path": str(p), "role": role})

    if getattr(args, "json", False):
        print(json.dumps({"images": images}))
    else:
        if not images:
            print(f"[bao] no UF2 images found in {release_dir}")
        else:
            for i in images:
                print(f"{i['path']} ({i['role']})")
