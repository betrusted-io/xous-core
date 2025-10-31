"""
Return release UF2 images for flashing.

Scans the release directory for UF2 images:
    <xous-core>/target/riscv32imac-unknown-xous-elf/release/{loader.uf2,xous.uf2,apps.uf2}

Outputs JSON if --json is set:
    {
        "images": [
            { "path": ".../loader.uf2", "role": "loader" },
            { "path": ".../xous.uf2",   "role": "xous"   },
            { "path": ".../apps.uf2",    "role": "app"    }
        ]
    }

Only files that actually exist are included.
"""

import json
import sys
import logging
from typing import List, Dict, Tuple
from utils.common import project_root

TRIPLE: str = "riscv32imac-unknown-xous-elf"
FILENAMES: List[Tuple[str, str]] = [
    ("loader.uf2", "loader"),
    ("xous.uf2",   "xous"),
    ("apps.uf2",    "app"),
]

EXIT_OK = 0
EXIT_NOT_FOUND = 2

def cmd_artifacts(args) -> None:
    """Command to list release UF2 images for flashing."""
    root = project_root()
    release_dir = root / "target" / TRIPLE / "release"

    images: List[Dict[str, str]] = []
    if not release_dir.exists():
        # No error: just report empty so callers can prompt “build first”
        if getattr(args, "json", False):
            print(json.dumps({"images": images}))
        else:
            logging.warning(f"[bao] release dir not found: {release_dir}")
        sys.exit(EXIT_OK)

    for fname, role in FILENAMES:
        p = release_dir / fname
        if p.exists() and p.is_file():
            images.append({"path": str(p), "role": role})

    if getattr(args, "json", False):
        print(json.dumps({"images": images}))
    else:
        if not images:
            logging.warning(f"[bao] no UF2 images found in {release_dir}")
        else:
            for i in images:
                print(f"{i['path']} ({i['role']})")
