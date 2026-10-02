# Resource audit — 2026-10-01

The [2 October follow-up](performance-audit-20261002.md) records the current
runtime, allocation and paint optimizations with matched before/after builds.
The measurements below describe their original checkpoints.

The original optimization audit below reduces repeated work without changing provider packets, discovery
filters, transport precedence, polling intervals, notification behavior or the
default charging animation. A later keyboard/menu checkpoint is recorded at the
end of this report; the optimization measurements below retain their original
build identities.
Measurements use **180-second windows**, with application startup and dashboard
interaction outside the CPU sampling window.

## Implemented changes

| Component | Finding and change |
| --- | --- |
| Core | Deduplicate borrowed readings and clone only final winners. Compare trimmed identities without allocating uppercase strings. Fit discharge estimates over borrowed samples and an optional endpoint, instead of copying the sample history. |
| Provider layer | Build each provider's ordered vendor list once, instead of scanning the static device catalog and allocating a tree on every poll. |
| Windows HID | Reuse one aligned native interface-detail buffer during each census; parse vendor IDs and normalize paths without temporary strings. Native handles retain owned cleanup. |
| Bluetooth | Update cached connection state in place; remove obsolete link-state entries when devices leave the successful discovery snapshot. |
| Runtime | Publish diagnostics only when their content changes, retaining dirty updates on queue backpressure. Move snapshots directly to the UI when status export is off. |
| Dashboard and tray | Cache tray-theme probes across short snapshot bursts, with immediate invalidation for theme redraw/settings/Explorer recovery. Update only changed control text and device-choice labels. Skip unchanged dashboard palettes. Release history data on page exit/close and discard late results while History is inactive. |
| Charts | Reuse render-target brushes and resize the Direct2D target only when its dimensions change. Graphics still release when the dashboard closes. |
| Storage | Prepare the two insert statements once per batch. Stream history with only the previous timestamp/awake state, avoiding a second owned reading per row. Replace correlated metadata orphan checks with an ordered merge over existing key indexes. |

The remaining architecture already limits worker counts and queue lengths: two
HID workers, one WinRT worker, one storage worker, and one native UI thread.
Maintenance waits are event-driven and bounded; disabled and suspended providers
are excluded from overdue scheduling. There are no per-device threads or
PowerShell subprocesses in the Rust application. SQLite caches remain 512 KiB
for writes and 256 KiB for read-only queries; pending history is capped at 4,096
observations. Queries stream retained observations and bound displayed results
to the selected interval and visible width. Insights retain bounded summaries.

Thirty charging frames remain pre-rendered. Their 100 ms timer exists only while
an animated icon exists; other icons update only when rendered state changes.
Device GUIDs and icon registration remain stable across sleep/wake. Dashboard
resources are created on demand. No new closed-dashboard repaint timer was added.

## History benchmark

The bundled SQLite benchmark seeded ten devices with 43,200 minute samples each
(432,000 retained rows). Seven repetitions of an unchanged-data prune produced:

| Operation | Before | After | Interpretation |
| --- | ---: | ---: | --- |
| Metadata retention/orphan check, median | 172.5 ms | 69.7 ms | About 60% less elapsed work in this benchmark |
| One 30-day use-history query | 451.5 ms | 417.3 ms | One observational run; not a statistical speedup claim |

These timings used the debug test build and are separate from background CPU
measurements. The new prune plan merges existing covering `(device, ts)` indexes
instead of performing a correlated lookup for every metadata row. It still
repairs arbitrary orphans, including externally deleted readings and metadata
outside the retained interval, and keeps the inclusive 30-day boundary. There is
no schema migration, new index or changed pruning cadence. A two-pass, 43,200-row
history query avoids 86,400 full-reading clones.

## Measurement conditions

The host ran Windows build 26200 on an AMD Ryzen 9 9950X3D with 32 logical
processors. CPU percentages here mean **one logical core**, calculated as
accumulated kernel plus user CPU seconds divided by elapsed seconds times 100;
they are not percentages of the whole 32-thread machine. Memory is private
committed bytes, not working set, and MiB means 1,048,576 bytes.

Rust used `rustc 1.98.0`, LLVM 22.1.8, target `x86_64-pc-windows-msvc`, static CRT,
fat LTO, one codegen unit and stripped symbols. The original was unchanged
upstream 1.13.0 source, with Python 3.13.15, HIDAPI 0.15.0, Pillow 12.3.0 and
pystray 0.19.5. No packaged upstream executable was available; executable size
cannot be compared to the original's source-file size.

Only one tray application runs at a time. Each gets isolated configuration and
history, a 60-second battery interval, status export enabled, update checking
disabled and closed dashboard/menu/flyout. Rust's normal charging animation
remains enabled for the synthetic test. Native validation warms the dashboard,
then measures with it closed. The original source launcher bypasses only its
main-entry startup/registry migration; the actual application loops, rendering
and storage run normally.

The external sampler includes the target's observed process tree, including
Python's launcher child and any PowerShell helpers. It samples every 200 ms,
uses PID plus creation time to handle reuse, and excludes its own CPU. A process
that starts and exits between samples is invisible, and CPU after the last
observed child sample can be missed. The initial pre-audit Rust synthetic run
used five-second memory samples and an equivalent process CPU delta. These are
short local measurements, not confidence intervals or a guarantee for all
device mixes. The synthetic original replaces discovery with one fake provider;
Rust simulation still creates platform watcher objects. Hardware measurements
are reported separately.

## Measured comparison

The optimization-audit executable is **2,722,304 bytes (2.60 MiB)**, SHA256
`4638C934130E688F31624E1D5E1B9D936B3E255458BF09D5B1A78BEAC0B1221B`.
The pre-audit baseline was commit `1327e22`, 2,720,256 bytes, SHA256
`F720422C06B7DFB09AA85ED780C286488C7E55225C4E3B07D22C8A3787F3E39C`.
The original source reference was upstream `a566a046` (1.13.0).

| Workload and metric | Original Python | Optimization-audit Rust port |
| --- | ---: | ---: |
| Wireless DeathAdder V4 Pro: average private memory | 138.18 MiB | 6.27 MiB |
| Wireless DeathAdder V4 Pro: peak private memory | 143.50 MiB | 14.84 MiB |
| Wireless DeathAdder V4 Pro: CPU, one logical core | 0.269% | 0.130% |
| One simulated charging mouse: average private memory | 33.10 MiB | 5.71 MiB |
| One simulated charging mouse: peak private memory | 34.02 MiB | 14.16 MiB |
| One simulated charging mouse: CPU, one logical core | 0.226% | 0.208% |
| Portable executable size | Not measured | 2.60 MiB |

All four final comparison windows lasted 180 seconds. Hardware discovery and
ordinary alerts were enabled, with animation off in that workload. The initial
final Rust attempt lacked an initial mouse reading and was excluded. After
physically waking the mouse, the accepted run retained one device through normal
idle/sleep behavior and exited gracefully. Neither hardware run changed polling
rate. Original hardware accounting observed four process identities, including
Python's launcher/application and PowerShell helpers; the synthetic original
observed two. The final Rust animated run followed 40 dashboard cycles, while the
original synthetic run did not open a flyout. Peaks include brief allocations
after dashboard closure; they are not sustained background memory levels.

The accepted hardware run used about **95.5% less private memory** and **51.6%
less CPU** than the original application in this local comparison. Animated CPU
was similar. These differences include the applications' architecture, native
discovery and runtime dependencies; they do not isolate the language itself.
The 2.60 MiB executable, both measured average/peak memory levels below 30 MiB,
0.130% hardware CPU and 0.208% animated CPU meet the engineering targets for
these workloads (10 MiB, 30 MiB, 0.5% without animation and 1% with animation).

| Closed-dashboard Rust metric | Before audit | Final audited build |
| --- | ---: | ---: |
| Hardware average private memory | 6.08 MiB | 6.27 MiB |
| Hardware CPU, one logical core | 0.130% | 0.130% |
| Animated average private memory | 6.00 MiB | 5.71 MiB |
| Animated CPU, one logical core | 0.208% | 0.208% |

Background CPU stayed essentially flat; these short runs do not establish a
large reduction from the audit itself. The original Rust implementation was
already small. Hardware runs used two warm dashboard cycles; the final animated
run used 40 rather than two, and the animated baseline's five-second memory
sampler could miss brief peaks. Small memory changes cannot be assigned to code
alone. Confirmed gains are less repeated allocation/control work and faster
history maintenance under the seeded benchmark.

## Dashboard lifecycle and remaining limits

The final native interaction run passed **40 close/reopen cycles**, keyboard
navigation, all four pages, settings persistence, per-device controls, duplicate
launches and simulated polling read/apply/restore. GDI/USER handles remained
107/45 at cycles 1, 20 and 40. Private memory rose from 5.61 MiB at cycle 1 to
16.36 MiB immediately after cycle 40, then settled to 8.02 MiB at five/ten seconds
and 8.07 MiB at twenty seconds with seeded history. A separate 40-cycle Insights
run held 20 GDI handles, changed from 16 to 17 USER handles and used 6.02–6.05 MiB.
Chart tests also cover owned brush reuse and resizing.

History buffers now release when leaving History or closing the dashboard, and
late query results cannot repopulate inactive pages. DirectWrite/COM initialization
and allocator caches can retain process-wide memory after graphics close. An
earlier 120-cycle stress run settled at 6.19, 6.46 and 6.78 MiB after its three
batches. Stable native handles and owned-resource review found no accumulating
chart resource leak, but this small memory growth does not prove zero heap leaks.

Theme tests matched the actual Windows app theme; simulated palettes and native
control painting tests also passed. Explorer recovery and resume checks replay
`TaskbarCreated` and `PBT_APMRESUMEAUTOMATIC`, rather than physically restarting
Explorer or suspending the machine. Multiple-device resource scaling, physical
charging, cross-monitor DPI movement and other vendors need further hardware
validation. No provider protocol, access restrictions or verified support label
was expanded by these tests.

## Compiler choice

Rust code is already compiled by LLVM. The MSVC target supplies Microsoft's
Windows ABI, linker, SDK and C/C++ tools for native dependencies. Changing to a
different Rust distribution is not required to obtain an optimizing compiler.
Cargo's `z` setting favors size and disables loop vectorization; `3` enables all
optimizations. Both experiment builds used the same audited source, before the
final history-buffer cleanup, with the same LTO, CRT, portability settings and
two warm dashboard cycles. The final executable remains the same size. See the official
[Cargo profile documentation](https://doc.rust-lang.org/cargo/reference/profiles.html)
and [MSVC target documentation](https://doc.rust-lang.org/rustc/platform-support/windows-msvc.html).

| Audited release setting | Executable | Animated average private memory | Animated CPU, one core |
| --- | ---: | ---: | ---: |
| `opt-level = "z"` | 2,722,304 bytes (2.60 MiB) | 6.14 MiB | 0.217% |
| `opt-level = 3` | 3,965,440 bytes (3.78 MiB) | 6.24 MiB | 0.208% |

Keep `z`. The 0.009-percentage-point CPU difference in one short run is
inconclusive; the speed-focused executable is 45.7% larger and uses slightly
more private memory in this run. No measurement supports changing compiler
distribution. Revisit profile settings only with a representative workload and
repeatable benefit, especially if future features introduce CPU-heavy work.

CPU-specific instructions and panic-abort were not enabled: the portable build
must work on other processors, and native callbacks deliberately catch unwind
failures. The toolchain and locked dependencies remain unchanged.

## Architectural comparison with upstream

| Area | Original Python application | Rust fork |
| --- | --- | --- |
| Tray lifecycle | pystray thread per device icon | All tray callbacks on one UI thread |
| Charging animation | Cached frames; shared animation thread wakes every 100 ms | Cached native frames; timer stops when no animated icons exist |
| Connection discovery | Scheduled HID/XInput signature checks; PowerShell Bluetooth watcher and WGI queries | Native connection notifications/WinRT events, with scheduled battery refresh and recovery |
| History | JSON discharge-estimator samples, capped at 400 per device | Bounded estimator plus SQLite raw 30-day history, charts and rate/cycle insights |
| Interface | Tray menu and Tk-based flyout | Native dashboard, tray controls and on-demand Direct2D charts |
| Deployment | Python dependencies or a separately packaged Python build | One portable executable, native libraries statically linked |

The fork performs additional history/dashboard work, so this is a comparison of
applications with matched battery/animation settings rather than a language
microbenchmark. Lower resource use does not imply better hardware compatibility;
upstream's verified/unverified device labels remain unchanged.

## Validation and reproduction

Formatting, strict workspace Clippy and **410 behavioral tests** pass. All 459
upstream test IDs have an explicit mapping or documented disposition. Two
optional history timing tests are excluded from normal tests and were run
explicitly for this audit. Parser simulations do not change hardware support
verification labels.

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python tools/merge-coverage.py --check --require-complete
cargo test -p hb-storage prune_thirty_day_sql_timing -- --ignored --nocapture
cargo test -p hb-storage usage_thirty_day_stream_timing -- --ignored --nocapture
./tools/build-rust.ps1
./tools/validate-native.ps1 -Seconds 180 -Animation -Cycles 2 -SamplerPython PYTHON_PATH
./tools/validate-native.ps1 -Seconds 180 -Hardware -Cycles 2 -SamplerPython PYTHON_PATH
./tools/validate-ui.ps1
./tools/validate-insights-ui.ps1
./tools/validate-configuration.ps1
```

See [the comparison harness documentation](https://github.com/Duplex4773/HaloBatteryNR/blob/main/rust-win32/tools/benchmark-comparison.md) for isolated
upstream launcher and sampler commands. Raw measurements and local hardware
identities stay in ignored `validation-local`; only aggregate results belong in
this report. No account/profile paths, personal device aliases or settings are
included in source or the portable package.
## Keyboard and tray menu follow-up

The later keyboard/menu implementation uses the same optimized Rust profile and
locked dependencies. Its executable is **2,794,496 bytes (2.67 MiB)**,
SHA256 `3CDFBF17B0AEF715AAD0AAF603E633C32E8DA5942CB60FA1464B726659715864`.
Configuration-only keyboard metadata uses existing HID workers and cached
enumeration. It is discovered only while Devices is visible; closed-dashboard
monitoring performs no keyboard-only discovery or rate queries. The configured
rate is never automatically enforced. Popup fonts/brushes exist only during a
native menu; dashboard/tray callback painting uses immutable palette snapshots.

The final binary was sampled in separate 180-second windows on the same host,
with 60-second battery refresh, status export on, polling controls off and the
dashboard/menu closed. Hardware used one wireless DeathAdder V4 Pro with real
numeric history samples; its connected K70 was recognized in a separate passive
probe. The animated run used one simulated charging mouse plus Razer/Corsair
configuration fixtures, warmed keyboard inventory and forty dashboard cycles,
then closed the window. Keyboard metadata is not a battery device. The original
Python values below retain the earlier audit's 180-second reference measurements;
they were not rerun for the keyboard implementation.

| Workload and metric | Original Python reference | Current Rust keyboard/menu build |
| --- | ---: | ---: |
| Wireless DeathAdder V4 Pro: average private memory | 138.18 MiB | 6.21 MiB |
| Wireless DeathAdder V4 Pro: peak private memory | 143.50 MiB | 14.90 MiB |
| Wireless DeathAdder V4 Pro: CPU, one logical core | 0.269% | 0.260% |
| One simulated charging mouse: average private memory | 33.10 MiB | 6.21 MiB |
| One simulated charging mouse: peak private memory | 34.02 MiB | 14.54 MiB |
| One simulated charging mouse: CPU, one logical core | 0.226% | 0.139% |
| Portable executable size | Not measured | 2.67 MiB |

The current build meets the 10-MiB executable, 30-MiB background private-memory,
0.5%-unanimated and 1%-animated CPU engineering targets on this host. CPU varies
across short windows: the latest hardware result is close to the Python reference,
so it does not establish a repeatable CPU improvement. The previous Rust checkpoint
remains above with its original numbers. Private memory remains substantially
lower here. These figures do not measure keyboard hardware rate effectiveness,
multiple-device scaling or resource use during a protected game.

The final animated warmup holds **107 GDI handles** before/after forty dashboard
cycles (USER 44 → 43). The separate seeded keyboard UI suite holds **43 USER /
107 GDI** at cycles 1, 20 and 40; private memory settles to **9.66 MiB** after close.
Eight actual-popup cycles hold **17 GDI handles**, after the first menu warmup.
Forty additional popup unit lifecycles and palette/mnemonic tests pass. Stable
native handles and settling do not prove the absence of every heap leak.

Formatting, strict Clippy, production source/import restrictions and **436 tests**
pass, with two optional timing tests ignored. The 459-ID upstream mapping remains
complete. Corsair discovery is passive and sends no configuration packet; the
five Razer keyboard routes remain hardware-unverified. An earlier resource run
whose settings changed was discarded. The harness now rejects settings changes
or a dashboard left open at measurement end. No identifying local data is included
in this report or the portable package.

```powershell
./tools/validate-keyboards-ui.ps1
./tools/validate-tray-menu.ps1
./tools/validate-native.ps1 -Seconds 180 -Hardware -Cycles 2 -SamplerPython PYTHON_PATH
./tools/validate-native.ps1 -Seconds 180 -Animation -Keyboards -Cycles 40 -SamplerPython PYTHON_PATH
```
