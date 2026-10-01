"""Isolated upstream benchmark harness; never modifies the upstream application.

Run on Windows with Python and the upstream requirements installed. --validate inspects source and
prints the contract without importing or starting the app. Only run one HID app
at a time. The synthetic scenario replaces provider discovery and the HID change
signature; rendering, animation, menus, history and ordinary worker loops remain
the real upstream implementation. The razer scenario keeps only its existing
battery provider. The hardware scenario keeps all original enabled providers,
Bluetooth/WGI discovery and the native change signature. Hardware tray icons
show connected device names briefly; configuration and logs remain isolated.
This harness implements no polling-rate commands. Use --status and the same
animation flag as the native validator for default-provider comparisons.
"""
from __future__ import annotations

import argparse
import ast
import importlib.machinery
import importlib.util
import io
import json
import math
import os
import sys
from pathlib import Path
import threading
import uuid


PROJECT = Path(__file__).resolve().parent.parent
PRIVATE = PROJECT / "validation-local"
UPSTREAM = PROJECT.parent
SOURCE = UPSTREAM / "halo_battery.pyw"


def literal(tree, name):
    for node in tree.body:
        if isinstance(node, ast.Assign) and any(
            isinstance(target, ast.Name) and target.id == name
            for target in node.targets
        ):
            return ast.literal_eval(node.value)
    raise ValueError(f"Upstream declaration missing: {name}")


def contract(args):
    tree = ast.parse(SOURCE.read_text(encoding="utf-8"), str(SOURCE))
    defaults = literal(tree, "DEFAULTS")
    labels = literal(tree, "PROVIDER_LABELS")
    config = dict(defaults)
    config.update(
        interval=60,
        animation=args.animation,
        status_file=args.status,
        update_check=False,
        fluent_menu=args.fluent_menu,
        playstation_full_mode=False,
    )
    if args.scenario != "hardware":
        config.update(
            bluetooth=False,
            notify=False,
            full_alert=False,
            quiet_fullscreen=False,
            disabled_providers=sorted(name for name in labels if name != "razer"),
        )
    else:
        # These original defaults match the native validator's Settings::default().
        # Keep make_providers and App.change_signature untouched for this scenario.
        assert defaults["bluetooth"] and defaults["quiet_fullscreen"]
        assert defaults["disabled_providers"] == []
    assert defaults["interval"] == 60
    assert all(key in defaults for key in config)
    compile(SOURCE.read_text(encoding="utf-8"), str(SOURCE), "exec")
    # Exercise the real upstream config parser in isolation, without importing
    # application modules or creating a Windows/HID session.
    functions = [node for node in tree.body if isinstance(node, ast.FunctionDef)
                 and node.name in ("_valid_setting", "load_config")]
    namespace = {"DEFAULTS": defaults, "LIMITS": literal(tree, "LIMITS"),
                 "CONFIG_PATH": "in-memory benchmark config", "json": json,
                 "open": lambda *a, **kw: io.StringIO(json.dumps(config))}
    exec(compile(ast.Module(body=functions, type_ignores=[]), str(SOURCE), "exec"), namespace)
    assert namespace["load_config"]() == config
    return config


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scenario", choices=("synthetic", "razer", "hardware"), default="synthetic")
    parser.add_argument("--duration", type=float, default=180)
    parser.add_argument("--appdata-root", type=Path)
    parser.add_argument("--animation", action=argparse.BooleanOptionalAction, default=True)
    parser.add_argument("--status", action=argparse.BooleanOptionalAction, default=False)
    parser.add_argument("--fluent-menu", action=argparse.BooleanOptionalAction, default=True)
    parser.add_argument("--validate", action="store_true")
    args = parser.parse_args()
    if not math.isfinite(args.duration) or args.duration <= 0:
        parser.error("duration must be finite and positive")
    config = contract(args)
    if args.validate:
        print(json.dumps({"scenario": args.scenario, "duration_seconds": args.duration,
                          "config": config, "launch": False}, indent=2))
        return

    root = (args.appdata_root or PRIVATE / f"python-resource-{uuid.uuid4()}").resolve()
    if root == PRIVATE.resolve() or not root.is_relative_to(PRIVATE.resolve()):
        parser.error("appdata-root must be a new child of validation-local")
    data = root / "HaloBattery"
    if root.exists():
        parser.error("appdata-root must not already exist; use a fresh directory")
    data.mkdir(parents=True)
    (data / "config.json").write_text(json.dumps(config, indent=2), encoding="utf-8")
    os.environ["APPDATA"] = str(root)

    # Resolve upstream sibling modules independently of the caller's working directory.
    sys.path.insert(0, str(UPSTREAM))
    loader = importlib.machinery.SourceFileLoader("halo_battery_benchmark", str(SOURCE))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    module = importlib.util.module_from_spec(spec)
    sys.modules[loader.name] = module
    loader.exec_module(module)
    module.flyout.enable_dpi_awareness()

    if args.scenario == "synthetic":
        class SyntheticProvider:
            name = "razer"
            pending = False

            def poll(self):
                return [module.DeviceStatus(
                    key="benchmark:synthetic-mouse", name="Benchmark mouse",
                    level=73, charging=True, online=True, source="razer", kind="mouse",
                )]

            def diagnostics(self):
                return ["Synthetic benchmark fixture; no HID calls"]

        module.make_providers = lambda: [SyntheticProvider()]
        module.App.change_signature = lambda self: (frozenset(), None)
    elif args.scenario == "razer":
        module.make_providers = lambda: [module.RazerProvider()]

    app = module.App()
    assert app.cfg == config
    timer = threading.Timer(args.duration, app.quit)
    timer.daemon = True
    (root / "launcher.json").write_text(json.dumps({
        "pid": os.getpid(), "scenario": args.scenario,
        "duration_seconds": args.duration, "config": config,
        "harness": "SourceFileLoader + App.run; skips main registry/startup migration",
    }, indent=2), encoding="utf-8")
    timer.start()
    try:
        app.run()
    finally:
        timer.cancel()
        if not app.stop_evt.is_set():
            app.quit()


if __name__ == "__main__":
    main()
