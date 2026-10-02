"""Seed invented sleep/wake observations for isolated native UI validation."""
import json
from pathlib import Path
import sqlite3
import sys
import time


folder = Path(sys.argv[1]).resolve()
if not any(path.name == "validation-local" for path in (folder, *folder.parents)):
    raise SystemExit("History fixtures require an isolated validation-local folder.")
folder.mkdir(parents=True, exist_ok=True)
start = int(time.time()) // 60 * 60 - 18 * 3600
with sqlite3.connect(folder / "history.db") as db:
    db.execute("CREATE TABLE IF NOT EXISTS readings(device TEXT NOT NULL, ts INTEGER NOT NULL, level INTEGER, payload TEXT NOT NULL, PRIMARY KEY(device,ts))")
    db.execute("DELETE FROM readings WHERE device='simulated:mouse'")
    for minute in range(1080):
        if minute < 120:
            level, connection = 80 - minute // 40, "online"
        elif minute < 600:
            level, connection = None, "sleeping"
        elif minute < 720:
            level, connection = 77 - (minute - 600) // 60, "online"
        elif minute < 1020:
            level, connection = None, "stale"
        else:
            level, connection = 75 - (minute - 1020) // 20, "online"
        timestamp = start + minute * 60
        reading = dict(key="simulated:mouse", name="Simulated mouse", source="simulation",
                       timestamp=timestamp, level=level, connection=connection,
                       charging=False, charging_inferred=False, precision="exact",
                       approx=None, kind="mouse", via="", serial=None, container=None)
        db.execute("INSERT INTO readings VALUES(?,?,?,?)",
                   (reading["key"], timestamp, level, json.dumps(reading)))
print("Synthetic awake/sleep/unavailable history seeded; no hardware data used.")
