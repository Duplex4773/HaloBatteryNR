"""External Windows process-tree resource sampler; no target process mutation.

Uses only Python stdlib. Reports private committed bytes (not working set), and
kernel+user CPU summed over each observed PID/creation-time identity. Processes
that start and exit between samples are invisible, so brief-child CPU may be
undercounted. Run this same sampler for Python and Rust comparisons.
"""
from __future__ import annotations

import argparse
import ctypes
from ctypes import wintypes as w
import json
import math
from pathlib import Path
import statistics
import time


class ProcessEntry(ctypes.Structure):
    _fields_ = [("dwSize", w.DWORD), ("cntUsage", w.DWORD), ("th32ProcessID", w.DWORD),
                ("th32DefaultHeapID", ctypes.c_size_t), ("th32ModuleID", w.DWORD),
                ("cntThreads", w.DWORD), ("th32ParentProcessID", w.DWORD),
                ("pcPriClassBase", w.LONG), ("dwFlags", w.DWORD),
                ("szExeFile", w.WCHAR * 260)]


class MemoryCounters(ctypes.Structure):
    _fields_ = [("cb", w.DWORD), ("PageFaultCount", w.DWORD)] + [
        (name, ctypes.c_size_t) for name in (
            "PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage",
            "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage", "QuotaNonPagedPoolUsage",
            "PagefileUsage", "PeakPagefileUsage", "PrivateUsage")]


def ticks(ft):
    return (ft.dwHighDateTime << 32) | ft.dwLowDateTime


class Native:
    def __init__(self):
        self.k = ctypes.WinDLL("kernel32", use_last_error=True)
        for name, args, result in (
            ("CreateToolhelp32Snapshot", [w.DWORD, w.DWORD], w.HANDLE),
            ("Process32FirstW", [w.HANDLE, ctypes.POINTER(ProcessEntry)], w.BOOL),
            ("Process32NextW", [w.HANDLE, ctypes.POINTER(ProcessEntry)], w.BOOL),
            ("OpenProcess", [w.DWORD, w.BOOL, w.DWORD], w.HANDLE),
            ("CloseHandle", [w.HANDLE], w.BOOL),
            ("GetProcessTimes", [w.HANDLE] + [ctypes.POINTER(w.FILETIME)] * 4, w.BOOL),
            ("K32GetProcessMemoryInfo", [w.HANDLE, ctypes.POINTER(MemoryCounters), w.DWORD], w.BOOL),
        ):
            fn = getattr(self.k, name)
            fn.argtypes, fn.restype = args, result

    def processes(self):
        handle = self.k.CreateToolhelp32Snapshot(2, 0)
        if handle == ctypes.c_void_p(-1).value:
            raise ctypes.WinError(ctypes.get_last_error())
        rows = {}
        try:
            entry = ProcessEntry()
            entry.dwSize = ctypes.sizeof(entry)
            ok = self.k.Process32FirstW(handle, ctypes.byref(entry))
            while ok:
                rows[int(entry.th32ProcessID)] = int(entry.th32ParentProcessID)
                ok = self.k.Process32NextW(handle, ctypes.byref(entry))
        finally:
            self.k.CloseHandle(handle)
        return rows

    def query(self, pid):
        handle = self.k.OpenProcess(0x1000 | 0x10, False, pid)
        if not handle:
            return None
        try:
            created, exited, kernel, user = (w.FILETIME() for _ in range(4))
            if not self.k.GetProcessTimes(handle, ctypes.byref(created), ctypes.byref(exited),
                                         ctypes.byref(kernel), ctypes.byref(user)):
                return None
            memory = MemoryCounters()
            memory.cb = ctypes.sizeof(memory)
            if not self.k.K32GetProcessMemoryInfo(handle, ctypes.byref(memory), memory.cb):
                return None
            return {"pid": pid, "created_ticks": ticks(created),
                    "cpu_ticks": ticks(kernel) + ticks(user), "private_bytes": memory.PrivateUsage}
        finally:
            self.k.CloseHandle(handle)


def accumulate(records, row, started_ticks):
    identity = (row["pid"], row["created_ticks"])
    if identity not in records:
        records[identity] = {"pid": row["pid"], "created_ticks": row["created_ticks"],
                             "first_cpu_ticks": 0 if row["created_ticks"] >= started_ticks
                             else row["cpu_ticks"], "last_cpu_ticks": row["cpu_ticks"]}
    else:
        records[identity]["last_cpu_ticks"] = row["cpu_ticks"]


def descendants(rows, initial):
    selected = set(initial)
    while True:
        new = {pid for pid, parent in rows.items() if parent in selected}
        if new <= selected:
            return selected & rows.keys()
        selected |= new


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pid", type=int)
    parser.add_argument("--duration", type=float, default=180)
    parser.add_argument("--interval", type=float, default=0.2)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        assert descendants({1: 0, 2: 1, 3: 2, 4: 99}, {1}) == {1, 2, 3}
        records = {}
        for row in ({"pid": 1, "created_ticks": 1, "cpu_ticks": 20},
                    {"pid": 1, "created_ticks": 1, "cpu_ticks": 30},
                    {"pid": 1, "created_ticks": 20, "cpu_ticks": 7}):
            accumulate(records, row, 10)
        assert len(records) == 2
        assert sum(r["last_cpu_ticks"] - r["first_cpu_ticks"] for r in records.values()) == 17
        print("PASS: ancestry, PID reuse and child startup CPU accounting; no app launched")
        return
    if (not args.pid or args.pid <= 0 or not args.output
            or not math.isfinite(args.duration) or args.duration <= 0
            or not math.isfinite(args.interval) or args.interval < 0.05):
        parser.error("positive --pid and --output required; finite positive duration and interval >=0.05")
    private = (Path(__file__).resolve().parent.parent / "validation-local").resolve()
    output = args.output.resolve()
    if not output.is_relative_to(private):
        parser.error("output must remain under private validation-local")
    native = Native()
    root = native.query(args.pid)
    if root is None:
        parser.error("target PID is not accessible")
    # FILETIME starts at 1601; Unix timestamp starts at 1970.
    started_ticks = int((time.time() + 11644473600) * 10_000_000)
    start = time.perf_counter()
    root_identity = (args.pid, root["created_ticks"])
    records, known, samples = {}, {root_identity}, []
    root_gone = False
    try:
        while time.perf_counter() - start < args.duration:
            all_rows = native.processes()
            live_known = set()
            for pid, created in known:
                if pid in all_rows:
                    row = native.query(pid)
                    if row and row["created_ticks"] == created:
                        live_known.add(pid)
            selected = descendants(all_rows, live_known)
            total_private = root_private = 0
            count = 0
            for pid in selected:
                row = native.query(pid)
                if row is None:
                    continue
                identity = (pid, row["created_ticks"])
                # Reject reuse of the root PID during a sample.
                if pid == args.pid and identity != root_identity:
                    continue
                known.add(identity)
                accumulate(records, row, started_ticks)
                total_private += row["private_bytes"]
                count += 1
                if identity == root_identity:
                    root_private = row["private_bytes"]
            root_gone = args.pid not in live_known
            if root_gone and count == 0:
                break
            samples.append({"elapsed_seconds": time.perf_counter() - start,
                            "private_bytes": total_private, "root_private_bytes": root_private,
                            "live_processes": count})
            remaining = args.duration - (time.perf_counter() - start)
            if remaining > 0:
                time.sleep(min(args.interval, remaining))
    except KeyboardInterrupt:
        pass
    elapsed = time.perf_counter() - start
    cpu = sum(max(0, r["last_cpu_ticks"] - r["first_cpu_ticks"])
              for r in records.values()) / 10_000_000
    memory = [sample["private_bytes"] for sample in samples]
    result = {"target_pid": args.pid, "duration_seconds": elapsed,
              "interval_seconds": args.interval, "sample_count": len(samples),
              "tree_private_bytes_average": statistics.fmean(memory) if memory else None,
              "tree_private_bytes_peak": max(memory) if memory else None,
              "tree_cpu_seconds": cpu, "tree_cpu_one_core_percent": cpu / elapsed * 100,
              "observed_process_identities": len(records), "root_exited": root_gone,
              "limitations": ["Children born and exited between samples are invisible; CPU can be undercounted.",
                              "Average/peak private bytes use successfully queried live processes at each sample.",
                              "Root CPU is a measured delta; newly created children include observed lifetime CPU."],
              "processes": list(records.values()), "samples": samples}
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(json.dumps({key: value for key, value in result.items()
                      if key not in ("samples", "processes")}, indent=2))


if __name__ == "__main__":
    main()
