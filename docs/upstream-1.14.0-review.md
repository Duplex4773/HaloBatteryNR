# Upstream 1.14.0 port review — 5 October 2026

Baseline: HaloBattery 1.13.0, `a566a046da5984f687d2bc973c6db92a171d60a2`.
Reviewed current upstream: 1.14.0,
`0e383bb560c04f9d9ac4b163b04941c6ade4cb85`.
The [endpoint comparison](https://github.com/HeyOkay/HaloBattery/compare/a566a046da5984f687d2bc973c6db92a171d60a2...0e383bb560c04f9d9ac4b163b04941c6ade4cb85)
changes 27 files. Older commits introduced through merge ancestry are not counted
as additional post-baseline work.

Thank you to HeyOkay, the original contributors, protocol authors and hardware
reporters. The original MIT notice and protocol credits remain in the source and
portable package. The Python implementation remains an external reference.

## Implementation decisions

| Upstream change | Rust result |
| --- | --- |
| Razer wireless keyboards | Add ten receiver/cable IDs for DeathStalker V2 Pro / TKL and BlackWidow V3 Mini / V4 Mini / V4 Tenkeyless HyperSpeed. Preserve transaction IDs, preferred interfaces and read-only battery behavior. |
| G-Wolves model-specific receivers | Add 48 receiver/cable rows and old/new protocol selection. Keep physical-device identity rules; matching names alone do not merge devices. |
| Cloud III S Wireless | Add both dongle IDs and output-report battery/charging exchanges, including replies arriving on another collection. |
| Logitech PRO X 2 LIGHTSPEED | Add Centurion feature discovery, matched replies, cached feature indices, invalidation and firmware fallback. |
| SteelSeries Nova Elite | Add direct and split status frames, output collection recovery and spare-battery exclusion. |
| JBL background reader | Keep up to eight read-only sessions open; existing workers drain at most 32 reports per collection once per second. No per-device or additional worker thread. Failures use a 15-second cooldown and typed errors. |
| Optional low-battery sound | Default off; asynchronous Windows battery sounds, per-device thresholds, critical sound at ≤5%, five-minute cooldown, no sound timer. Quiet gaming does not suppress it. |
| Portable mode | `portable.txt` selects adjacent `HaloBatteryNext-data`. Writability is tested once at launch. An app-specific subfolder prevents sharing the parent's config; explicit `--data-dir` wins. |
| Bluetooth snapshots defeated the HID interval | Already covered by independent Rust provider deadlines and provider-local snapshot application. Bluetooth results do not reschedule every HID provider. |
| Hide during an update could resurrect a tray icon | Already covered by one UI owner, commands and latest-setting checks when synchronizing tray icons. |
| Charging graphics rendered both colours | Rust already renders only the active colour and reuses charging frames. It keeps one colour to limit memory, rather than adding a second 30-frame cache for theme flips. |
| Python menu, packaging and reference-layout changes | Retain native Rust controls, standalone root layout, static portable executable and locally run checks. Workflows and update checking remain disabled. |

Battery support now covers **28 provider families and 309 catalog rows**: 62 new
rows from this update. Battery additions do not enlarge the polling write
allowlists or allow commands to unknown devices. User-mode HID access and existing
configuration safeguards remain unchanged; no anti-cheat certification is claimed.

JBL's one-second servicing is bounded and only scheduled with open collections.
It drains input without sending a query, including while games run; with no JBL
collection there is no additional servicing cadence. An explicit one-shot probe
can listen for up to ten seconds on first open. Power and battery frames are
processed in order; ON alone never makes an old level fresh. Shutdown, suspension,
generation changes and disabling the provider release persistent handles.

Low-battery sound reuses fresh scheduled readings and allocates no cooldown
metadata while off. It is coalesced across verified connection aliases, stops
while charging/unavailable/hidden, and tolerates clock resets. The five-minute
repeat occurs at the next fresh reading, without more hardware checks.

Portable mode deliberately does not move existing settings/history or import the
parent's config. A failed portable request tries to save `portable-issue.txt` in
the fallback folder; failure to save that optional note does not prevent startup.
Support reports include the actual data folder and remain local files.

## Verification

Formatting, strict Clippy, **568 passing workspace tests** (two optional timing
tests ignored), optimized release build and production API restrictions pass.
The unchanged complete 459-ID baseline gate passes. Added tests cover protocol
packets, matching/cross-collection replies, wrong IDs, failure retention,
cancellation/deadlines, cached feature invalidation, collection/report limits,
sound rules and portable-directory selection. Separate fixtures compare **2,752
parser cases** with the external 1.14.0 source. Native dashboard lifecycle,
theme/keyboard and tray regressions remain in the workspace run.

The [test delta](upstream-1.14.0-test-delta.md) inventories 142 new upstream IDs
separately. Their relevant Rust evidence does not claim a complete per-assertion
mapping of all **601** parent IDs. Generator changes protect the frozen baseline
from silently becoming an incomplete latest-version mapping.

All new Rust hardware tests use invented identities and simulated transports.
Parent verified reports are inherited only for the connections listed in
[device support](device-support.md). No physical commands, new local hardware
verification or manual UI walkthrough were performed during this update.

The portable executable is **3,033,600 bytes (2.89 MiB)**, below the 10 MiB target,
SHA256 `03AC446FEBE104A8600ADDEB9F07F9CE5890605658B3DC87453F467347F51D54`.
No fresh CPU/private-memory measurements were taken; historical three-minute
measurements retain their original builds and workloads. Bounded code paths and
successful simulated tests do not establish physical receiver resource use.
The local package is separate from the prepared release assets and has not been
published.
