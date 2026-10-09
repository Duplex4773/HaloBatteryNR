# Halo Battery Next

A lightweight Windows 11 x64 battery monitor and device configuration app, written
in Rust with native Win32 controls and Direct2D graphics. Each battery device gets
its own system-tray icon. One portable executable runs without Python, .NET or a
webview runtime.

This is an independent rewrite of [HaloBattery](https://github.com/HeyOkay/HaloBattery),
based on upstream 1.13.0 (`a566a046`), with reviewed updates through
[1.14.0](https://github.com/HeyOkay/HaloBattery/releases/tag/v1.14.0) (`0e383bb`). It carries forward device protocols, provider
behavior, tests and hardware reports through a separately implemented runtime.
The source tree contains the Rust app and its development tools; the original
Python application is maintained externally as a reference.

## Features

- **Devices, History, Insights and Settings:** native dashboard, keyboard navigation,
  per-monitor DPI, automatic Windows app light/dark appearance and themed tray menus.
  Grouped Settings and scrollable pages keep navigation and the Settings Save
  footer visible. Advanced options holds less-used settings; device labels, polling
  feedback and battery-life summaries use plain language.
- **28 battery provider families:** mice, wireless keyboards, headsets and controllers
  over HID, Bluetooth, XInput and Windows.Gaming.Input. Per-device rename, hide,
  icon selection, alert thresholds and provider switches.
  New support includes Cloud III S Wireless, PRO X 2 LIGHTSPEED, Nova Elite,
  five additional Razer wireless keyboards and G-Wolves models with their own receivers.
- **Thirty-day history:** Time used is the default view; sleeping and unavailable
  periods pause estimated awake time. Calendar views carry the last known level
  through missing readings. Original observations remain intact.
- **Battery insights:** conservative remaining-use predictions, observed charge
  summaries and drain comparisons by hardware-confirmed polling rate, with evidence
  coverage and freshness. Awake time measures availability rather than input activity.
- **Optional fullscreen boost:** supported mice can automatically switch to a
  higher polling rate during Windows-reported fullscreen gaming, then restore
  the previous rate on the desktop. Off by default, with separate per-device
  choices, transition delays and no continuous rate enforcement.
  [Setup and limits](docs/fullscreen-boost.md).
- **Optional polling controls:** read, apply and restore for allowlisted Razer,
  Logitech and MCHOSE mice and five wired Razer keyboards. Supported mouse tray
  menus select a rate directly; tooltips retain the last confirmed rate without
  querying on hover. Optional startup restore applies saved selections once.
  With controls enabled, a one-time startup read fills mouse tooltips without
  opening the dashboard; sleeping or inaccessible devices may need Refresh later.
- **Batteryless keyboards:** appear on Devices without battery tray icons, alerts,
  history or estimates. Recognized Corsair models, including K70 RGB Pro, explain
  why polling changes are unavailable under the one-time-only configuration policy.
- **Battery warnings:** configurable orange band at 30% by default, then red at the
  low-alert threshold. Charging animation, full-charge alerts and gaming suppression.
  Optional Windows low-battery sounds also work during games and repeat on fresh
  readings after five minutes; sound is off by default.
- **Portable settings:** create `portable.txt` beside the executable to keep data
  in its adjacent `HaloBatteryNext-data` folder. An unwritable folder falls back
  to the usual app-data location. Existing settings are not imported.
- **Stable monitoring:** tray placement persists through mouse sleep; closing the
  dashboard leaves monitoring active. Connection events, bounded workers, cached
  graphics and batched history keep background work small.
  JBL reports are collected through a persistent read-only connection without
  delaying other device polls or adding a device thread.

See [supported battery devices](docs/device-support.md),
[polling models and limits](docs/polling-controls.md) and the
[application guide](docs/application.md). DeathAdder V4 Pro wireless polling is
**hardware verified by user testing** at all six supported rates: 125, 500, 1000,
2000, 4000 and 8000 Hz. Parent-app battery verification is labelled separately;
it does not establish polling verification.
See the [1.14.0 port review](docs/upstream-1.14.0-review.md) for implemented changes,
existing equivalents and verification limits.

The [6 October source audit](docs/source-audit-20261006.md) fixes recovery,
notification and history edge cases, reduces repeated background work, and
improves native control repainting and History feedback.
The [Insights and allocation follow-up](docs/insights-performance-audit-20261006.md)
reduces routine snapshot and history-save allocations, and corrects Insights
consumption accounting and evidence descriptions.
The [background CPU follow-up](docs/cpu-audit-20261006.md) reduces repeated HID
discovery scans and tray appearance checks without slowing battery refreshes.

Polling controls default off and use ordinary user-mode HID access: no drivers,
elevation, game-process access, input interception or continuous rate enforcement.
These boundaries do not claim universal anti-cheat approval.

## Screenshots

Native UI with **synthetic device and history data**, rendered from the
5 October 2026 consumer UI update. Screenshots demonstrate the interface,
not hardware verification.

| Devices | History |
| --- | --- |
| ![Devices in dark mode](docs/screenshots/devices-dark.png) | ![Time-used history in dark mode](docs/screenshots/history-dark.png) |

| Settings | Insights |
| --- | --- |
| ![Settings in dark mode](docs/screenshots/settings-dark.png) | ![Battery Insights in dark mode](docs/screenshots/insights-dark.png) |

[Light Devices](docs/screenshots/devices-light.png) ·
[Light Settings](docs/screenshots/settings-light.png) ·
[Light History](docs/screenshots/history-light.png) ·
[Light Insights](docs/screenshots/insights-light.png) ·
[Advanced settings](docs/screenshots/settings-advanced-dark.png) ·
[Capture instructions](docs/screenshots/README.md)

## Performance compared with the original

The latest portable executable is **2.93 MiB**. The [6 October CPU audit](docs/cpu-audit-20261006.md)
compared the previous Rust build (`70e7c4a`) with the optimized build (`7a7bc11`)
using the same simulated-device test and **three-minute windows**, with the
dashboard closed and charging animation off:

| Measurement | Previous Rust build | Latest Rust build |
| --- | ---: | ---: |
| CPU, one logical core | 0.165% | **0.043%** |
| Average private memory | 6.30 MiB | **6.32 MiB** |
| Peak private memory | 14.14 MiB | 14.20 MiB |

That window used approximately **74% less CPU**, with essentially unchanged
memory. A second latest-build window after 40 dashboard open/close cycles measured
**0.095% CPU and 6.50 MiB average private memory**, with no native-handle growth.
These simulated results demonstrate tray-work savings; they do not guarantee a
fixed improvement on every device or replace a hardware benchmark.

An earlier three-minute run on the same day compared the latest upstream source
with the **previous** Rust executable. Status export was off and both used a
60-second battery interval:

| Measurement | Upstream 1.14.0, simulated mouse | Rust `70e7c4a`, real devices |
| --- | ---: | ---: |
| Average private memory | 33.38 MiB | 6.59 MiB |
| Peak private memory | 34.33 MiB | 6.66 MiB |
| CPU, one logical core | 0.095% | 0.113% |

Rust used approximately **80% less private memory** in that run. The workloads
differed: upstream's simulation disabled hardware polling and animation, while
Rust retained its real-device settings. This does not establish a CPU advantage
for either implementation, and the Rust figures precede the latest optimization.

### Earlier measurements

The [3 October audit](docs/performance-audit-20261003.md) addresses access-failure
retry loops, bounded history recovery, departed-device caches and History resizing.
The [4 October UI audit](docs/ui-audit-20261004.md) records the dashboard changes,
native resource checks and their limits. The [5 October consumer UI audit](docs/ui-audit-20261005.md)
covers the current appearance, native controls and open/close resource validation.

Local Windows measurements used **three-minute windows**, a 60-second battery
refresh interval and status export enabled, with dashboards and menus closed.
Original figures include observed helper processes. These are historical audit
checkpoints, not new measurements of every subsequent build.

| Measurement | Original Halo Battery 1.13.0 | Rust port 0.1.0 |
| --- | ---: | ---: |
| Wireless DeathAdder V4 Pro: average private memory | 138.18 MiB | 6.03 MiB |
| Wireless DeathAdder V4 Pro: CPU, one logical core | 0.269% | 0.122% |
| Simulated animated charging mouse: average private memory | 33.10 MiB | 5.66 MiB |
| Simulated animated charging mouse: CPU, one logical core | 0.226% | 0.156% |
| Executable size at optimization checkpoint | Not measured | 2.64 MiB |

The native runtime and cached graphics substantially reduced memory on the tested
machine while adding history and a dashboard. These compare unchanged upstream
Python source with optimized Rust builds, rather than two packaged releases.
The Rust hardware run includes mouse sleep, so its CPU figure is not a
like-for-like speedup comparison. See the [performance audit](docs/performance-audit-20261002.md)
and [original resource audit](docs/resource-audit.md) for workloads, compiler
experiments, exact builds and limitations.

## Build and run

Install Rust stable with the MSVC toolchain, Visual Studio C++ build tools and the
Windows SDK. Run from this repository root:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
.\tools\build-rust.ps1
.\tools\package-rust.ps1
```

The portable output is `dist/HaloBatteryNext-0.1.0-windows-x64/` and its ZIP beside
it. Run `HaloBatteryNext.exe` to open the dashboard; `--background` starts quiet
monitoring. Close the window to keep monitoring, reopen from a tray icon or by
launching the executable again, and use tray **Exit** to stop it.

Settings, SQLite history, diagnostics and optional status export live under
`%APPDATA%\HaloBatteryNext`. Startup registration uses `--background`. The app has
separate settings and notification identity from the parent application.
Update checking is disabled until an independent release repository is configured.

The portable executable statically links HIDAPI, SQLite and the MSVC CRT. Build
scripts optimize for size and strip local compiler paths. Builds and validation
run locally; GitHub workflows remain disabled for this fork.

## Development and device updates

| Path | Purpose |
| --- | --- |
| `crates/core` | Platform-independent models, settings, alerts and estimates |
| `crates/providers` | Device catalogs, protocols and injected transport tests |
| `crates/windows` | Native HID, Bluetooth, controller and Windows integration |
| `crates/storage` | Atomic configuration and bounded SQLite history |
| `crates/app` | Native UI, tray and bounded application workers |
| `tools` | Rust build, package, validation and external-reference generators |
| `docs` | Support tables, protocol credits, evidence and screenshots |

Python is needed only for selected development tools. Generators accept an
external reference through `--upstream PATH` or `HALO_BATTERY_UPSTREAM`; ordinary
Rust builds and coverage checks use committed catalogs and fixtures. Future
upstream product changes require a reviewed port, not recompilation alone.
See [contributing](CONTRIBUTING.md), [provider parity](docs/provider-parity.md),
[release steps](docs/releasing.md) and the [changelog](CHANGELOG.md).

## Credits and license

Thank you to [HeyOkay](https://github.com/HeyOkay) and the
[original HaloBattery contributors](https://github.com/HeyOkay/HaloBattery/graphs/contributors)
for the application, protocol research, tests, diagnostics and hardware reports.
Upstream's [MIT license](LICENSE) is retained. Protocol sources and their
contributors remain credited in [protocol notes](docs/protocols.md) and the
polling evidence documents. Portable packages include dependency notices.
