import argparse, re, shutil
from pathlib import Path
from typing import List, Set
from tomlkit import parse as toml_parse, dumps as toml_dumps

APPS_DIR = "apps-dabao"
TEMPLATE_APP = "helloworld"

VALID_NAME_RE = re.compile(r"^[a-z][a-z0-9_-]*$")

def _read(path: Path) -> str:
    with path.open("r", encoding="utf-8") as f:
        return f.read()

def _write(path: Path, content: str):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="\n") as f:
        f.write(content)

def _workspace_members(root: Path) -> List[str]:
    doc = toml_parse(_read(root / "Cargo.toml"))
    ws = doc.get("workspace")
    if not ws or "members" not in ws:
        return []
    return [str(x) for x in ws["members"]]

def _workspace_package_names(root: Path) -> Set[str]:
    names: Set[str] = set()
    for rel in _workspace_members(root):
        pkg_cargo = root / rel / "Cargo.toml"
        if not pkg_cargo.exists():
            continue
        try:
            pkg_doc = toml_parse(_read(pkg_cargo))
            pkg = pkg_doc.get("package")
            if pkg and "name" in pkg:
                names.add(str(pkg["name"]))
        except Exception:
            # ignore malformed members
            pass
    return names

def _add_member_if_missing(root: Path, rel: str) -> bool:
    cargo = root / "Cargo.toml"
    doc = toml_parse(_read(cargo))
    ws = doc.setdefault("workspace", {})
    arr = ws.setdefault("members", [])
    if any(str(x) == rel for x in arr):
        return False
    arr.append(rel)  # tomlkit preserves array style & trailing commas
    _write(cargo, toml_dumps(doc))
    return True

def _copy_template_cargo(template: Path, dest: Path, new_name: str):
    tdoc = toml_parse(_read(template))
    if "package" not in tdoc:
        raise RuntimeError("Template Cargo.toml missing [package]")
    tdoc["package"]["name"] = new_name
    _write(dest, toml_dumps(tdoc))

def _copy_template_src(root: Path, new_dir: Path):
    src_template = root / APPS_DIR / TEMPLATE_APP / "src"
    dest_src = new_dir / "src"
    if src_template.exists():
        shutil.copytree(src_template, dest_src)
    else:
        # Fallback minimal main.rs
        _write(dest_src / "main.rs", """#![no_std]
#![no_main]
use core::panic::PanicInfo;
#[panic_handler] fn panic(_info: &PanicInfo) -> ! { loop {} }
#[no_mangle] pub extern "C" fn main() -> ! { loop {} }
""")

def cmd_app_create(args: argparse.Namespace) -> int:
    root = Path(args.xous_root).resolve()
    name = args.name.strip().lower()

    if not VALID_NAME_RE.match(name):
        print("error: invalid app name. Use lowercase [a-z][a-z0-9_-]*")
        return 2

    apps_dir = root / APPS_DIR
    template_dir = apps_dir / TEMPLATE_APP
    template_cargo = template_dir / "Cargo.toml"
    if not template_cargo.exists():
        print(f"error: template not found at {template_cargo}")
        return 2

    # crate/package name collision check
    existing_pkg_names = _workspace_package_names(root)
    if name in existing_pkg_names:
        print(f'error: package name "{name}" already exists in workspace')
        return 2

    new_dir = apps_dir / name
    if new_dir.exists():
        print(f"error: app directory already exists: {new_dir}")
        return 2

    # create structure
    new_dir.mkdir(parents=True)
    _copy_template_cargo(template_cargo, new_dir / "Cargo.toml", name)
    _copy_template_src(root, new_dir)

    # add to workspace members
    rel_member = f"{APPS_DIR}/{name}"
    _add_member_if_missing(root, rel_member)

    print(f"created: {rel_member}")
    return 0

def register(sub: argparse._SubParsersAction):
    ap = sub.add_parser("app", help="Bao app utilities")
    sp = ap.add_subparsers(dest="app_cmd", required=True)

    ap_create = sp.add_parser("create", help="Create a new Bao app")
    ap_create.add_argument("--xous-root", default=".", help="path to xous-core root")
    ap_create.add_argument("--name", required=True, help="new app name (crate name)")
    ap_create.set_defaults(func=cmd_app_create)
