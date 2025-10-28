from pathlib import Path

_ROOT_MARKERS = ("Cargo.toml", ".git")

def project_root() -> Path:
    p = Path(__file__).resolve()
    for candidate in [p] + list(p.parents):
        for marker in _ROOT_MARKERS:
            if (candidate / marker).exists():
                return candidate
    # Fallback: original behavior (two parents up)
    return Path(__file__).resolve().parents[2]