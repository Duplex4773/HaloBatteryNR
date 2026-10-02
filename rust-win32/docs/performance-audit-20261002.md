# Performance follow-up — 2 October 2026

This audit reviewed the runtime, providers, Windows discovery, storage, core
models, dashboard and tray rendering after the keyboard/menu checkpoint. It
removes repeated work while preserving provider transactions, polling intervals,
notification rules, one-time configuration controls and default charging animation.
The [previous audit](resource-audit.md) contains the original Python comparison
and compiler experiment; this report measures the additional changes separately.

## Implemented savings

| Area | Change and retained behavior |
| --- | --- |
| Runtime | Successful empty provider completions with no previous devices/errors no longer rebuild/export a battery snapshot. Unrelated commands also avoid exports. Device/error/settings/lifecycle changes still publish immediately, with a monotonic five-second freshness/status heartbeat. |
| Notifications and core | Skip empty held-notification work and its extra gaming-state query. Reuse the already resolved readings during state cleanup, borrow identity/name strings, and build tooltip text directly. Actual notification delivery and configuration commands retain their gaming guards. |
| Status export | Serialize borrowed typed fields directly into JSON instead of building a cloned JSON value tree and device vector. All upstream-compatible fields, hidden-device filtering, nulls, timestamps and shutdown behavior remain intact. |
| History and Insights | Parse borrowed SQLite text instead of copying each payload/session string; move the previous Insights observation instead of cloning it. Stop the calendar query's branch-selection count at the display limit plus one. Full reading validation, corrupt-row continuity breaks and sampling rules remain intact. |
| Providers | Skip unused collection grouping for protocols that only inspect one collection. Format bounded diagnostic replies into one string, and stop before formatting when the diagnostic cap is reached. Group-dependent vendor rules, packet acceptance and timing are unchanged. |
| Windows HID | Filter native UTF-16 paths by vendor before allocating decoded path strings. Preserve marker precedence, Bluetooth vendor parsing and collection recovery. |
| Dashboard and tray | Borrow inventories/tray views where possible; clone only the selected device. Compare typed icon state without formatting a signature on every update. Fill fixed UTF-16 buffers directly. Reuse the Windows stock DC brush for temporary paint fills within saved DC state; retain owned brushes where Windows requires a persistent lifetime. |

No dependencies were upgraded. Storage now declares the workspace's existing
Serde dependency directly. The release profile remains size optimization, fat
LTO, one codegen unit, stripped symbols and static CRT. No compiler switch,
animation reduction, polling slowdown, new background timer or feature removal
was needed.

## Deterministic allocation checks

These are instrumented debug tests of specific operations, not whole-process
memory measurements. Counts include Rust allocator allocation/reallocation
requests, excluding SQLite's C allocations and Windows-owned memory. Inputs are
constructed before counting. The reference operations preserve the previous
implementation's behavior and their outputs are compared with the new code.

| Operation | Previous/reference | Optimized |
| --- | ---: | ---: |
| 64-device status export, including atomic file replacement | 1,950 allocations; 137,089 requested bytes | 17 allocations; 33,280 requested bytes |
| 1,440-row Insights query, owned versus borrowed SQLite text | 14,407 allocations; 580,562 requested bytes | 11,528 allocations; 170,491 requested bytes |
| Formatting one 32-byte diagnostic reply | At least 34 allocations | 1 allocation; 0 when already at the cap |
| 43,200 owned observations fed into Insights | Previously cloned each preceding discharge reading | 3 allocations / 1,120 bytes for bounded summary growth |
| 1,000 empty held-notification checks | Previously resolved/cloned current readings | 0 allocations |

Status export uses about **99% fewer allocation calls** and **76% fewer requested
bytes** in this 64-device fixture. Borrowed SQLite text saves about **71% of
requested Rust heap bytes** in its fixture. These percentages are workload-specific;
they do not mean the application's resident/private memory drops by those amounts.

The snapshot regression also confirms that a burst of 25 successful empty
providers produces no extra snapshot/export after the initial publication,
while new readings, failures, recovery and disconnects still publish immediately.
Backward wall-clock changes cannot postpone the monotonic freshness heartbeat.

## Measurement method

Both release builds use the same host, compiler, profile and process-tree sampler.
Baseline: commit `68fc678`, **2,794,496 bytes**, SHA256
`3CDFBF17B0AEF715AAD0AAF603E633C32E8DA5942CB60FA1464B726659715864`.
Optimized: **2,767,872 bytes**, SHA256
`3D2CF85FDCDB3B6616431D448BB72B28C5F7B19120B28C11981064290454266F`.
The executable is 26,624 bytes smaller (about 0.95%).

Measurements use **180-second windows**, private committed bytes and CPU as a
percentage of **one logical core**. Each application runs alone with isolated
settings/history, a 60-second battery interval, status export enabled and its
dashboard/menu closed during sampling. Hardware runs use the wireless
DeathAdder V4 Pro, no animation and two dashboard warmup cycles. Animated runs
use a simulated charging mouse, configuration-only keyboard fixtures and forty
dashboard warmup cycles. Keyboard controls remain disabled during resource tests.
Startup and interaction are outside the CPU window. Settings changes or an open
dashboard invalidate a run. No builds or other app benchmarks run concurrently.

Hardware histories confirm real battery readings in both runs, but also reveal
a confound: the baseline mouse stayed online, while the optimized run transitioned
to sleeping. Therefore the hardware CPU difference cannot be attributed to the
code changes. The synthetic animated runs control device state more closely.

The sampler checks every 200 ms and can miss very short-lived helper processes.
Private memory includes transient growth before allocator/Windows reclamation;
it is not working set. These short local runs have no confidence intervals and
do not establish a universal CPU improvement. Raw logs, local paths and physical
device identifiers remain in ignored local validation output.

## Measured results

| Workload / metric | Before this audit | Optimized |
| --- | ---: | ---: |
| Hardware: average private memory | 6.25 MiB | 6.03 MiB |
| Hardware: peak private memory | 14.98 MiB | 14.49 MiB |
| Hardware: CPU, one logical core | 0.339% | 0.122% |
| Animated charging: average private memory | 6.46 MiB | 5.66 MiB |
| Animated charging: peak private memory | 14.97 MiB | 14.25 MiB |
| Animated charging: CPU, one logical core | 0.156% | 0.156% |
| Portable executable | 2.67 MiB | 2.64 MiB |

The controlled animated run used about **12.4% less average private memory**,
with effectively unchanged CPU. Hardware average private memory was about 3.5%
lower, but its awake/sleep difference prevents attributing the CPU reduction to
optimization. All observed values remain below the size, private-memory and CPU
engineering targets; they are observations on this device mix, not worst-case bounds.

The animated warmup retained **104 GDI handles** before and after forty cycles,
versus 107 for the baseline. Hardware warmup retained 17 versus 20 baseline GDI
handles. USER counts did not grow. Both runs survived replayed Explorer restart
and resume events and flushed shutdown status cleanly.

For context, the original Python reference from the previous audit is retained
below; it was **not rerun on 2 October**. It includes observed helper processes,
and its simulated workload does not include Rust's keyboard fixtures/dashboard.

| Measurement | Original Python 1.13.0 reference | Current Rust port |
| --- | ---: | ---: |
| Hardware: average private memory | 138.18 MiB | 6.03 MiB |
| Hardware: CPU, one logical core | 0.269% | 0.122% (includes mouse sleep) |
| Simulated charging: average private memory | 33.10 MiB | 5.66 MiB |
| Simulated charging: CPU, one logical core | 0.226% | 0.156% |

These support a substantial memory advantage for the Rust port on the tested
machine. They do not establish a precise cross-language CPU speedup.

## Further opportunities

- Time-used history still performs two streaming passes with full reading
  deserialization. A borrowed full-schema parser could reduce allocations further,
  but must preserve escaped strings, required-field validation and corrupt-row
  continuity breaks. A partial schema would risk changing historical estimates.
- HID enumeration repeats the native path census for vendor cache refreshes.
  Sharing one census needs explicit generation/TTL and short-list recovery rules;
  this audit preserves the current reconnect and inaccessible-collection behavior.
- WinRT watcher startup could be deferred for disabled integrations. That needs
  additional enable/disable, resume and missed-event lifecycle validation.
- History chart geometry/text caching could reduce open-window repaint work,
  at the cost of additional retained memory and DPI/theme/size invalidation. Closed
  dashboards already release graphics and history data.

These are future candidates, not known unbounded leaks. The five-second freshness
heartbeat and 100 ms charging animation remain intentional costs. Lowering their
cadence would change visible behavior rather than remove duplicate work.

## Reproduction

Formatting, strict workspace Clippy, **451 tests**, release build, production
API restrictions and the complete 459-ID coverage mapping pass. Two optional
timing tests remain ignored. Native dashboard and keyboard suites each pass forty
open/close cycles, including duplicate-launch reopening, selection and simulated
Read/Apply/Restore. Keyboard fixtures stay out of battery history/status. Eight
actual tray popup cycles follow the system app palette and retain 15 GDI handles.
Source/binary privacy checks pass. See [validation details](validation-next.md).

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test -p hb-core -p hb-storage --test performance_regressions --locked -- --nocapture
python tools/merge-coverage.py --check --require-complete
./tools/build-rust.ps1
./tools/validate-configuration.ps1
$sampler = (Get-Command python).Source
./tools/validate-native.ps1 -Seconds 180 -Hardware -Cycles 2 -SamplerPython $sampler
./tools/validate-native.ps1 -Seconds 180 -Animation -Keyboards -Cycles 40 -SamplerPython $sampler
./tools/validate-ui.ps1
./tools/validate-keyboards-ui.ps1
./tools/validate-tray-menu.ps1
```

Use `-Executable` and `-Label` to run the preserved baseline independently.
Close the normal application before isolated validation and restart it afterward.
