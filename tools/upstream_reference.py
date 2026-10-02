"""Resolve an explicit external Python reference for Rust development tools.

Use --upstream PATH or HALO_BATTERY_UPSTREAM. The reference must be outside
this repository. Archive reference: upstream 1.13.0, revision
a566a046da5984f687d2bc973c6db92a171d60a2. No parent-directory fallback is used.
"""
from __future__ import annotations

import os
import ast
import re
from pathlib import Path

PINNED_REVISION = "a566a046da5984f687d2bc973c6db92a171d60a2"
REPOSITORY = Path(__file__).resolve().parents[1]
PROVIDER_FILES = (
    "__init__", "am_infinity", "astro", "asus", "audeze", "barracuda",
    "base", "blackshark", "bluetooth", "corsair", "eightbitdo",
    "hidlist", "gwolves", "hyperx", "hyperx_alpha2", "hyperx_cloud3", "jbl",
    "keychron", "lamzu", "lofree", "logitech", "mchose", "nintendo", "playstation",
    "pulsar", "razer", "steelseries", "wgi", "wlmouse", "xinput",
)


def add_upstream_argument(parser):
    parser.add_argument("--upstream", type=Path,
                        help="external Python reference directory (or HALO_BATTERY_UPSTREAM)")


def reference_version(root):
    """Read version provenance without importing or starting the Python app."""
    tree = ast.parse((root / "halo_battery.pyw").read_text(encoding="utf-8"))
    for node in tree.body:
        if isinstance(node, ast.Assign) and any(
            isinstance(target, ast.Name) and target.id == "VERSION"
            for target in node.targets
        ):
            value = ast.literal_eval(node.value)
            if isinstance(value, str) and re.fullmatch(r"[0-9A-Za-z.+-]+", value):
                return value
    raise ValueError("external reference has no valid VERSION declaration")


def resolve_upstream(parser, explicit=None, *, required=True, providers=False, tests=False):
    value = explicit or os.environ.get("HALO_BATTERY_UPSTREAM")
    if not value:
        if not required:
            return None
        parser.error("provide --upstream PATH or HALO_BATTERY_UPSTREAM with an external reference")
    root = Path(value).expanduser().resolve()
    if root.is_relative_to(REPOSITORY):
        parser.error("the Python reference must be outside the Rust repository")
    if not root.is_dir():
        parser.error("the external reference directory does not exist")
    expected = ["halo_battery.pyw", "requirements.txt", "flyout.py", "history.py",
                "icons.py", "updates.py", "winevents.py"]
    if providers:
        expected += [f"providers/{name}.py" for name in PROVIDER_FILES]
    if tests:
        expected += ["tests/test_notifications.py", "tests/test_hide_rename.py"]
    missing = [name for name in expected if not (root / name).is_file()]
    if missing:
        parser.error("external reference is missing expected files: " + ", ".join(missing))
    if tests and not any((root / "tests").glob("test_*.py")):
        parser.error("external reference has no tests/test_*.py files")
    return root
