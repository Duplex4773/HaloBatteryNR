"""Generate separate parser parity evidence for the upstream 1.14.0 delta; no I/O."""
import argparse
import importlib
import json
import random
import sys
from pathlib import Path
from upstream_reference import add_upstream_argument, resolve_upstream, reference_version

parser = argparse.ArgumentParser(description=__doc__)
add_upstream_argument(parser)
args = parser.parse_args()
reference = resolve_upstream(parser, args.upstream, providers=True)
if reference_version(reference) != "1.14.0":
    parser.error("the delta fixture reference must be version 1.14.0")
sys.path.insert(0, str(reference))
rng = random.Random(1140)
cases = []
specs = [
    ("gwolves", "parse_old", [0, 0xa1, 2, 0x8f, 0, 1, 73], 6),
    ("hyperx_cloud3s", "parse", [0xc, 2, 3, 1, 0, 6, 73], 6),
    ("steelseries_elite", "parse", [7, 0xb7, 73, 100, 2], 2),
    ("steelseries_elite", "parse_reply", [1, 0xb0, 0, 0, 1, 0, 31, 100, 0, 0, 0, 0, 0, 0, 0, 2], 6),
    ("logitech_centurion", "parse_battery", [73, 0, 3], 0),
    ("logitech_centurion", "parse_legacy", [0x51, 0xb, 0, 0, 0, 0, 0, 0, 4, 0, 73, 0, 2], 10),
    ("logitech_centurion", "payload_of", [0x51, 4, 0, 3, 0x11, 0], 1),
]
for module, name, template, value_offset in specs:
    parse = getattr(importlib.import_module("providers." + module), name)
    inputs = [template[:n] for n in range(len(template) + 1)]
    for value in range(256):
        data = template.copy()
        data[value_offset] = value
        inputs.append(data)
    for _ in range(128):
        data = template.copy()
        data[rng.randrange(len(data))] = rng.randrange(256)
        inputs.append(data)
    for data in inputs:
        result = parse(data)
        if module == "gwolves" and result == (None, None):
            result = None
        cases.append({"parser": module + "." + name, "data": data, "result": result})
output = Path(__file__).resolve().parents[1] / "crates/providers/tests/upstream114-fixtures.json"
output.write_text(json.dumps(cases, indent=2) + "\n", encoding="utf-8")
print(f"Generated {len(cases)} delta cases from upstream 1.14.0")
