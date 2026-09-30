"""Merge auditable upstream-to-Rust coverage without importing hardware providers.

Status meanings:
  mapped: the referenced regression asserts the upstream behavior or native equivalent
  partial: parser/helper evidence only, or some upstream assertions remain unverified
  obsolete: the Python-only implementation was retired (explanation required)
  intentional_difference: an explicit, tested safety/identity policy replaces upstream
  manual: manual evidence exists, but no automated equivalence is claimed
  not_mapped: no evidence link yet

Use --check in CI to validate links and detect a stale generated inventory/summary.
Use --require-complete as the parity gate; partial/manual/not_mapped fail that gate.
Only the standard library is required. No Python reference modules are imported.
"""
from __future__ import annotations
import argparse
import ast
from collections import Counter, defaultdict
import json
from pathlib import Path
import re
import sys
import warnings

ROOT = Path(__file__).resolve().parents[1]
INVENTORY = ROOT / "docs/provider-test-inventory.json"
SUMMARY = ROOT / "docs/coverage-summary.md"
STATUSES = {"mapped", "partial", "obsolete", "intentional_difference", "manual", "not_mapped"}
RANK = {"mapped": 6, "intentional_difference": 5, "obsolete": 4, "partial": 3, "manual": 2, "not_mapped": 1}
COMPLETE = {"mapped", "obsolete", "intentional_difference"}

def read_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8-sig"))

def discover_reference_ids():
    ids = set()
    for path in sorted((ROOT / "tests").glob("test_*.py")):
        with warnings.catch_warnings():
            warnings.simplefilter("ignore", SyntaxWarning)
            tree = ast.parse(path.read_text(encoding="utf-8-sig"), filename=str(path))
        classes = {n.name: n for n in tree.body if isinstance(n, ast.ClassDef)}
        def methods(node, seen=None):
            seen = set() if seen is None else seen
            if node.name in seen:
                return set()
            seen.add(node.name)
            names = {n.name for n in node.body if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef)) and n.name.startswith("test_")}
            for base in node.bases:
                if isinstance(base, ast.Name) and base.id in classes:
                    names.update(methods(classes[base.id], seen))
            return names
        for name, cls in classes.items():
            for method in methods(cls):
                ids.add(f"{path.stem}.{name}.{method}")
    return ids

def validate_target(target: str, allow_implementation=False):
    if target.endswith(".ps1"):
        path=(ROOT/target).resolve()
        if not path.is_relative_to(ROOT) or not path.is_file() or not re.search(r"\bthrow\b",path.read_text(encoding="utf-8-sig"),re.IGNORECASE):
            raise ValueError(f"invalid automation regression script: {target}")
        return "automation"
    if not target or "::" not in target:
        raise ValueError(f"missing regression target: {target!r}")
    file, *parts = target.split("::")
    path = (ROOT / file).resolve()
    if not path.is_relative_to(ROOT) or path.suffix != ".rs" or not path.is_file():
        raise ValueError(f"invalid Rust source path: {target}")
    test = parts[-1]
    source = path.read_text(encoding="utf-8-sig")
    pattern = r"#\s*\[\s*test\s*\]\s*(?:#\s*\[[^\]]+\]\s*)*(?:pub\s+)?(?:async\s+)?fn\s+" + re.escape(test) + r"\s*\("
    if not re.search(pattern, source):
        if allow_implementation and re.search(r"\bfn\s+"+re.escape(test)+r"\s*\(",source):
            return "implementation_only"
        raise ValueError(f"target is not an actual #[test] function: {target}")
    for module in parts[:-1]:
        if not re.search(r"\bmod\s+" + re.escape(module) + r"\s*\{", source):
            raise ValueError(f"target module not found: {target}")
    return "rust_test"

def validate_mapping(row, ids, path):
    id = row.get("upstream_test")
    if id not in ids:
        raise ValueError(f"{path.name}: unknown upstream ID {id}")
    status = row.get("status", "not_mapped")
    if status not in STATUSES:
        raise ValueError(f"{id}: unknown status {status}")
    raw_targets = row.get("rust_tests") or ([row["rust_test"]] if row.get("rust_test") else [])
    targets=[part.strip() for target in raw_targets for part in target.split(";") if part.strip()]
    if targets:
        row["rust_test"]=targets[0]
        row["rust_tests"]=targets
    if status in {"mapped", "intentional_difference"} and not targets:
        raise ValueError(f"{id}: {status} requires a Rust test link")
    kinds=[]
    for target in targets:
        kinds.append(validate_target(target,allow_implementation=status=="partial"))
    if targets:
        row["target_kinds"]=dict(zip(targets,kinds))
    if status in {"partial", "obsolete", "intentional_difference", "manual"} and not row.get("notes", "").strip():
        raise ValueError(f"{id}: {status} needs an explanation")
    # Parser fixtures cannot establish transaction ordering, safe collection selection,
    # cancellation, error retention, or physical-device identity.
    if status == "mapped" and targets and all("parser_parity.rs" in t for t in targets):
        if row.get("scope") not in {"parser", "catalog"}:
            raise ValueError(f"{id}: parser-only evidence cannot claim provider I/O parity; use partial or explicit scope=parser")
    return targets

def merge():
    original = read_json(INVENTORY)
    rows = {r["upstream_test"]: r for r in original}
    if len(rows) != len(original):
        raise ValueError("duplicate IDs in reference inventory")
    discovered = discover_reference_ids()
    if set(rows) != discovered:
        missing = sorted(discovered - set(rows))
        extra = sorted(set(rows) - discovered)
        raise ValueError(f"reference inventory does not match AST discovery: missing={missing}, extra={extra}")
    evidence = defaultdict(list)
    sources = sorted((ROOT / "docs").glob("coverage_mapping_*.json"))
    for path in sources:
        data = read_json(path)
        if not isinstance(data, list):
            raise ValueError(f"{path.name}: expected a list")
        seen = set()
        for entry in data:
            if entry["upstream_test"] in seen:
                raise ValueError(f"{path.name}: duplicate mapping {entry['upstream_test']}")
            seen.add(entry["upstream_test"])
            targets = validate_mapping(entry, discovered, path)
            evidence[entry["upstream_test"]].append((path.name, entry, targets))
    result = []
    for id in sorted(rows):
        row = {k:v for k,v in rows[id].items() if k not in {"status", "rust_test", "rust_tests", "notes", "mapping_sources", "scope", "target_kinds", "additional_evidence"}}
        entries = sorted(evidence[id], key=lambda e:(-RANK[e[1]["status"]],e[0]))
        if entries:
            primary = entries[0][1]
            row["status"] = primary["status"]
            row["rust_test"] = primary.get("rust_test") or None
            row["rust_tests"] = sorted({t for _,_,targets in entries for t in targets})
            row["notes"] = primary.get("notes", "")
            if len(entries)>1:
                row["additional_evidence"]=[dict(source=source,status=entry["status"],test_targets=targets,notes=entry.get("notes","")) for source,entry,targets in entries[1:]]
            row["mapping_sources"] = [path for path,_,_ in entries]
            row["target_kinds"]={target:kind for _,entry,_ in entries for target,kind in entry.get("target_kinds",{}).items()}
            if primary.get("scope"):
                row["scope"] = primary["scope"]
        elif row.get("parser_evidence"):
            validate_target(row["parser_evidence"]["rust_test"])
            row.update(status="partial",rust_test=row["parser_evidence"]["rust_test"],notes="Captured parser-call evidence only. Original provider I/O, cache, identity, and error assertions are not established by this fixture.")
        else:
            row.update(status="not_mapped",rust_test=None)
        result.append(row)
    return result, sources

def summary(rows, sources):
    counts = Counter(r["status"] for r in rows)
    incomplete = [r for r in rows if r["status"] not in COMPLETE]
    lines = ["# Upstream behavior coverage", "", f"The pinned reference contains **{len(rows)} upstream test IDs**. This inventory distinguishes full regression evidence from parser/helper coverage and retired Python implementation details. Mapped regression links are validated as actual Rust `#[test]` functions or native automation scripts containing failure assertions. Partial implementation pointers are explicitly labeled and do not count as regression tests. Link existence alone does not prove every assertion in a Python test is equivalent.", "", "| Status | IDs | Meaning |", "| --- | ---: | --- |"]
    meanings = {"mapped":"Automated regression asserts the original behavior or its native equivalent.","intentional_difference":"Tested policy deliberately replaces the original behavior; rationale is recorded per ID.","obsolete":"Python implementation retired; no claim of automated native equivalence.","partial":"Some parser/helper evidence exists; original behavior is not fully established.","manual":"Manual evidence only; automated regression remains outstanding.","not_mapped":"No usable regression link yet."}
    for status in ["mapped","intentional_difference","obsolete","partial","manual","not_mapped"]:
        lines.append(f"| {status} | {counts[status]} | {meanings[status]} |")
    lines += ["",f"**{len(incomplete)} IDs remain incomplete for automated parity.** Parser fixture counts are not a substitute for safe transaction, timeout, identity, and recovery tests. Hardware-free I/O tests do not claim physical hardware validation. Only the attached Razer hardware was available for hardware smoke checks; GameSir/WGI and other vendors use simulated/pure report evidence.","","Inputs: " + ", ".join(f"`docs/{p.name}`" for p in sources) + ".", "", "Run `python tools/merge-coverage.py` to regenerate, `python tools/merge-coverage.py --check` to validate generated evidence, and add `--require-complete` to enforce no partial/manual/unmapped IDs. The tool uses Python's standard library and does not import the original app or query hardware.", "", "## Incomplete IDs", ""]
    if not incomplete:
        lines.append("None. Retired implementation details and intentional differences remain identified separately above.")
    else:
        for row in incomplete:
            lines.append(f"- `{row['upstream_test']}` — {row['status']}: {row.get('notes','No equivalent test linked.')}")
    return "\n".join(lines) + "\n"

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check",action="store_true")
    parser.add_argument("--require-complete",action="store_true")
    args=parser.parse_args()
    try:
        rows,sources=merge()
        encoded=json.dumps(rows,indent=2,ensure_ascii=False)+"\n"
        report=summary(rows,sources)
        if args.check:
            for path,expected in [(INVENTORY,encoded),(SUMMARY,report)]:
                if not path.exists() or path.read_text(encoding="utf-8-sig") != expected:
                    raise ValueError(f"generated file is stale: {path.relative_to(ROOT)}; run python tools/merge-coverage.py")
        else:
            INVENTORY.write_text(encoded,encoding="utf-8")
            SUMMARY.write_text(report,encoding="utf-8")
        counts=Counter(r["status"] for r in rows)
        print(f"Validated {len(rows)} upstream IDs: " + ", ".join(f"{status}={counts[status]}" for status in sorted(STATUSES)))
        if args.require_complete and any(r["status"] not in COMPLETE for r in rows):
            raise ValueError("coverage parity is incomplete; see docs/coverage-summary.md")
    except (ValueError,OSError,KeyError,TypeError,json.JSONDecodeError) as error:
        print(f"Coverage validation failed: {error}",file=sys.stderr)
        return 1
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
