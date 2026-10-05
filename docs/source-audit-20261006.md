# Source audit — 6 October 2026

This pass reviewed the five Rust workspace crates and the release/validation
tools, concentrating on monitoring and scheduling, device identity and HID
transactions, notification transitions, recovery, history and estimates, native
UI ownership and repainting. Existing upstream 1.14.0 and dashboard changes were
retained. No new dependency, background thread or configuration packet was added.

## Findings and fixes

| Area | Verified problem | Change |
|---|---|---|
| Alerts | A held full-charge alert could appear after the battery had discharged, or when its current level was unknown. | Discard obsolete full alerts; wait for a known level before delivery. Quiet-game and delivery-failure cases have regressions. |
| Bluetooth | After a failed full snapshot, the next cached result could look successful and clear the failure without recovery. | Retain the failure until a successful snapshot; retry after 15 seconds or a connection event. |
| Provider recovery | Centurion feature discovery remained cached across connection invalidation, even when the control path was reused. | Clear that protocol cache on invalidation. Absent-path pruning was already present. |
| History recovery | New samples could be recorded before an older failed batch, allowing an older same-second sample to replace a newer one. | Drain the bounded recovery tail before new observations. |
| Insights | Invalid session text became an unknown session; adjacent damaged rows could be treated as continuous usage. | Break continuity and count unreadable rows instead. Valid legacy rows without session metadata remain supported. |
| Calendar history | Serialized records were not checked against the selected device and SQL timestamp in every sampling path. | Reject mismatched identities, timestamps and percentages outside 0–100 at every sampling budget. |
| Configuration/export | Concurrent saves shared a fixed temporary filename. | Use exclusive, uniquely named temporary files; failed replacement preserves the previous file and cleans up only its own temporary file. |
| Background updates | Unchanged cached readings and repeated identical errors unnecessarily republished snapshots. | Compare provider state first; retain the existing five-second freshness heartbeat and immediate meaningful updates. |
| Retention | Minute-based cleanup rescanned all retained usage metadata to repair orphans. | Add a timestamp index and delete expired rows directly each minute; retain full orphan repair at startup. |
| Device names | Control characters could produce multiline or truncated native labels. | Use the same bounded single-line normalization when loading and saving names; blank restores the detected name. |
| Native controls | Native background erasure could precede complete custom painting, contributing to flashes. Scroll-repeat state also survived hide/disable. | Skip redundant erasure for fully painted controls and cancel scrolling when hidden or disabled. High-contrast native rendering remains available. |
| History feedback | The page did not clearly report loading; keyboard-only inventories had a misleading empty battery selector. | Show a loading message and “No battery-powered devices”. |

These are corrections backed by source paths and regression tests, rather than
claims that all possible defects have been eliminated. Hardware allowlists,
rate-write verification, uncertain-SET handling and anti-cheat boundaries remain
unchanged. This pass did not change physical device settings.

## Validation and performance

Formatting, strict workspace Clippy, **581 workspace tests**, the optimized
release build, production source/import restrictions and privacy scans pass.
Two optional timing tests are skipped by the normal suite; the retention timing
test was additionally run explicitly and passed. Source/build checks found no
local account identifiers in the scanned text files or executable.

The portable executable is **3,046,400 bytes (2.91 MiB)**, with SHA256
`47B6E0388581521DDD572E3CAA926D7B813DFBCB96EA23099CE5961D622D6F42`.
Package: `0.1.0-source-audit-20261006`, unsigned.

The complete 459-ID upstream baseline gate passes: 363 mapped, 39 intentional
differences and 57 obsolete Python-specific cases. Two quiet-game entries now
explicitly document discarding an obsolete full-charge alert. No ID was removed.

| Optimized native fixture, no animation | Before 40 dashboard cycles | After 40 dashboard cycles |
|---|---:|---:|
| Measurement window | 180.06 s | 180.07 s |
| Average private memory | 6.14 MiB | 6.02 MiB |
| Peak private memory | 14.05 MiB | 6.08 MiB |
| Average CPU, one logical core | 0.200% | 0.139% |

This compares lifecycle phases of the audit's optimized test fixture, **not old
versus new application builds**. The fixture included the background fixes;
subsequent label, erase-paint and calendar validation changes were checked in the
final 581-test suite. Closed-dashboard GDI/USER handle non-growth assertions
passed across forty cycles. It meets the 30 MiB / 0.5% unanimated targets for this
synthetic workload, but is not a production executable or hardware benchmark.
An additional animated production measurement could not start because another
application instance was running; that instance was left running. No new animated
or physical-device resource figure is claimed.

The separate retention benchmark seeded **432,000 retained rows** (ten devices,
thirty days at one sample/minute), then timed seven executions with **no rows
expired**. Median metadata cleanup was **85.6981 ms** for the previous merged
orphan scan and **0.0068 ms** for indexed expiry. The older correlated query was
188.721 ms. Full orphan repair still runs at startup. These timings isolate one
SQL statement; they exclude initial index creation and actual expiry deletion.

Resource checks use synthetic data and isolated folders. They do not inspect or
alter personal history, stop a running installation, or establish new hardware
verification. Native tests cover dark/light control painting and forty dashboard
open/close cycles. Real cross-monitor DPI changes, physical reconnects, actual
Explorer restarts and vendor-specific hardware behavior remain manual checks.

The new SQLite timestamp index adds disk space and a small amount of write work
in exchange for eliminating a full retained-metadata scan every minute. Existing
bounded SQLite cache sizes are unchanged. Timing that query separately does not
establish an equivalent whole-application CPU improvement.

## Reproduce

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test -p hb-storage --release --locked prune_thirty_day_sql_timing -- --ignored --nocapture --test-threads=1
python tools/merge-coverage.py --check --require-complete
./tools/build-rust.ps1
./tools/validate-configuration.ps1
```

To repeat the isolated UI fixture's two three-minute windows:

```powershell
$env:HALO_MEASURE_UI_RESOURCES = '1'
try {
    cargo test -p halo-battery-next --release --locked native_dashboard_reopens_after_nested_close_and_external_destruction -- --nocapture --test-threads=1
} finally {
    Remove-Item Env:HALO_MEASURE_UI_RESOURCES
}
```

For production measurements, close the normal app first and run
`./tools/validate-native.ps1 -Seconds 180 -Animation -Cycles 40`.
Do not compare separate workloads or noisy runs as evidence of a code speedup.
