# Halo Battery Next 0.1.0

A Windows 11 x64 Rust/Win32 port of [HaloBattery](https://github.com/HeyOkay/HaloBattery), based on upstream 1.13.0 (`a566a046`). The Rust workspace lives in `rust-win32/` on `main`; the Python implementation remains at the repository root as the protocol reference. The `port/rust-win32` branch also retains the port checkpoint.

Thanks to [HeyOkay](https://github.com/HeyOkay) and all
[original HaloBattery contributors](https://github.com/HeyOkay/HaloBattery/graphs/contributors)
for the original application, protocol research, tests and hardware reports.
The port retains upstream MIT notices and credits to the device-protocol projects.

## Performance compared with upstream

Three-minute Windows measurements, with 60-second battery polling, status export
enabled and dashboards/menus closed:

| Measurement | Original Halo Battery 1.13.0 | Rust port 0.1.0 |
| --- | ---: | ---: |
| Wireless DeathAdder V4 Pro: average private memory | 138.18 MiB | 6.03 MiB |
| Wireless DeathAdder V4 Pro: CPU, one logical core | 0.269% | 0.122% |
| One simulated charging mouse: average private memory | 33.10 MiB | 5.66 MiB |
| One simulated charging mouse: CPU, one logical core | 0.226% | 0.156% |
| Portable executable size | Not measured | 2.64 MiB |

Original figures include observed helper processes. This is unchanged upstream
Python source versus optimized Rust release builds on one machine; a packaged
original release was unavailable. Original figures are retained from the previous
audit. Rust figures use the 2 October optimization build and 40 animated dashboard
warmup cycles. Its hardware run includes mouse sleep, so the CPU figures are not
a like-for-like speedup comparison. The controlled animated before/after run used
about 12% less private memory with unchanged CPU. The fork retains 30-day raw
history and a native dashboard. See the [latest performance audit](docs/performance-audit-20261002.md)
for allocation savings, exact builds and limitations, and the [original resource
audit](docs/resource-audit.md) for Python conditions and compiler experiments.

## Run and build

Run `HaloBatteryNext.exe` to open the dashboard. Closing the window keeps monitoring active. Click a device tray icon, choose Open dashboard, or launch the executable again to reopen it. A duplicate `--background` launch stays quiet. The tray menu offers refresh, device controls and Exit. Settings include Windows startup registration, which starts the executable with `--background`.

The dashboard follows Windows app light/dark appearance and changes while open,
including native controls, history graphics and the tray context menu. Windows high-contrast colors
take priority. Tray appearance remains independently configurable. Unplugged
or hidden devices still follow the existing per-device tray identity rules.

Supported mouse tray icons also offer **Polling rate → desired rate**. Enable
polling controls once in Settings; no preliminary Refresh is needed. The worker
reads and validates the hardware before changing it, then verifies readback.
Opening the submenu performs no hardware I/O. Last-confirmed checkmarks, optional
Refresh and Restore previous are available; the submenu shares the system theme.

Noncharging battery rings turn orange at **30%** by default, then red at the
device's configured low-alert threshold (20% by default). Charging remains green.
Settings → Orange warning % adjusts the visual band; zero disables orange.
Red takes priority if thresholds overlap. Notification thresholds are unchanged.

Configuration, diagnostics, optional `status.json` and SQLite history are stored in `%APPDATA%\HaloBatteryNext`. The app has its own startup entry, singleton mutex and notification identity. It does not import upstream settings. Both applications may run together, but receivers that reject concurrent access report recoverable errors.

Build with Rust and the Visual Studio C++/Windows SDK tools:

```powershell
cd rust-win32
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
./tools/build-rust.ps1
./tools/package-rust.ps1
```

The portable executable statically links HIDAPI's Windows C backend, bundled SQLite and the MSVC CRT. Native Windows system components provide Win32/WinRT/Direct2D. No Python, .NET, webview or asynchronous runtime is needed by the executable.

The release script also removes local build paths from embedded compiler messages.

## Architecture

| Crate | Responsibility |
| --- | --- |
| `hb-core` | Device readings, validated settings, identity precedence, alerts and awake-use estimates; no Windows or UI dependencies |
| `hb-providers` | Vendor protocols and parsers with injected HID and clock contracts |
| `hb-windows` | HID, Bluetooth, WGI/XInput, native resources, startup, theme and gaming detection |
| `hb-storage` | Atomic JSON and bounded, batched SQLite history |
| `halo-battery-next` | One state owner, two HID workers, one WinRT worker, one storage worker and one native UI thread |

Providers return either successful discovery (possibly empty) or an explicit communication error. Errors retain stale device readings instead of treating failures as disconnects. Immutable snapshots cross to the UI; commands cross back to the engine. Queues and worker counts are bounded. Charging HICON frames are cached; the 100 ms timer runs only while an animated charging icon exists. Dashboard graphics are created on demand and released on close. Calendar history selects the requested interval; use-time history streams retained observations to calculate awake time before bounded display sampling.

History retains 30 days, batches writes once per minute and flushes on normal exit. Changed readings are queued immediately; unchanged readings are accepted at most once per minute. Usage estimates need at least 30 minutes of awake discharge and a three-point percentage drop. Coarse readings do not produce estimates.

Tray icons are updated in place across sleep, wake and theme changes. A known
Razer mouse remains represented while its receiver collection is present; its
cached battery level expires after five minutes without removing the icon.
This preserves the registered identity behind the user's notification-area
placement. Unplugging the receiver or hiding the device still removes its icon.

The History page defaults to **Time used**, with 2/8/24-hour use ranges. Estimated
awake time pauses across sleeping, unavailable and charging observations; it does
not track cursor activity. **Calendar time** retains 24-hour/7-day/30-day views
and carries the last known level through missing readings instead of drawing
gaps. Original observations and timestamps remain intact. See
[`docs/history.md`](docs/history.md) for the calculation and display limits.

PlayStation Bluetooth full mode is opt-in. 8BitDo mode switching is disabled. Unknown devices are excluded from command allowlists. Update checking remains disabled pending an independently configured release repository.

The **Insights** page compares observed battery drain by last-confirmed polling
rate and shows recent charge summaries with awake use, consumption and average
drain, newest first. Coverage and freshness explain missing or limited data;
projections require complete intervals between observed battery drops instead
of extrapolating flat or interrupted samples. Recent-use predictions retain
learned drain through sleep while excluding unobserved losses. Only fresh hardware readbacks establish rate
evidence; saved requests never count. Existing history can provide partial
charge summaries. See [`docs/insights.md`](docs/insights.md).

Optional hardware polling-rate controls default off. Enable them in Settings,
select a supported mouse or wired Razer keyboard on Devices, then use Refresh rate, Apply rate or Restore
previous. DeathAdder V4 Pro, Viper V3 Pro and Viper Mini Signature Edition have
dedicated high-rate routes up to 8000 Hz; the Mini requires suitable firmware.
DeathAdder V3 Pro retains conservative legacy support. Superlight 2/DEX and
PRO X2 Superstrike use advertised HID++ rates up to 8000 Hz on receiver C54D,
with software control mode required; legacy C53A and direct USB are limited to
1000 Hz. MCHOSE A7 V2 Ultra+ has an 8000-Hz wireless route restricted to its
100B receiver, exact paired model and firmware 5.46.2.4. Other receivers/models
remain unavailable until their target identity and protocol are established.
Saved selections are never applied automatically. Readback verifies configuration,
not independently measured effective frequency. See `docs/polling-controls.md`.

Wired keyboard controls support Huntsman V2/Tenkeyless and BlackWidow V4/Pro/75%
on their exact allowlisted USB identities. All seven rates from 125 to 8000 Hz,
including 250 Hz, use a single acknowledged SET followed by configured-rate
readback. Hardware behavior remains unverified locally. Known Corsair high-rate
models, including K70 RGB Pro, appear with a reason that polling changes are
unavailable: maintained software sessions are disabled by design. No Corsair
configuration packets, mode changes or heartbeats are sent.

Batteryless keyboards are dashboard-only inventory. They offer rename and, where
supported, polling controls, without a per-device tray icon, battery alert,
history row, Insights estimate or status battery entry. The application's existing
fallback icon remains available. Passive keyboard enumeration runs at most once
every 30 seconds while Devices is visible; closed dashboards do not discover or
query keyboards. Existing mouse controls and battery monitoring are unchanged.
See [`docs/polling-keyboard-evidence.md`](docs/polling-keyboard-evidence.md).

Configuration uses ordinary user-mode HID APIs, with no game-process memory
access, injection, input hooks, input automation or custom drivers. Apply is
blocked when Windows reports gaming/fullscreen/presentation activity. These
restrictions reduce risk but do not establish approval by every anti-cheat
vendor; see `docs/anti-cheat.md`.

## Validation and support

See `docs/provider-parity.md` for provider/test coverage and limitations, and `docs/validation-next.md` for measured size, memory, CPU and native lifecycle checks. Original verified/unverified support labels remain in the upstream README and protocol documentation. Simulated tests do not establish hardware verification. The connected DeathAdder V4 Pro is the initial hardware validation device.

Engineering targets are an executable at most 10 MiB, background private memory at most 30 MiB with one device, and average CPU at most 0.5% of one logical core without animation or 1% with one animated charging icon. Current resource audits use three-minute measurement windows. See [`docs/resource-audit.md`](docs/resource-audit.md) for the optimization audit, compiler comparison and measured comparison with upstream. Measurements, rather than the choice of language alone, determine whether these targets are met.

The upstream MIT license is retained in `LICENSE`. Protocol authors and captures are credited in `docs/protocols.md`; portable packages also include third-party notices.

## Updating the upstream reference

The Python reference remains at the repository root; Rust files live under
`rust-win32/`. This keeps upstream source updates separate from the port.
From the repository root, merge the upstream branch, then refresh the port's
catalog and parser fixtures using a Python environment with the upstream
dependencies installed:

```powershell
git fetch upstream
git merge upstream/main
cd rust-win32
python tools/port_catalog.py
python tools/provider_fixtures.py
cargo fmt --all
python tools/merge-coverage.py
cargo test --workspace --locked
./tools/build-rust.ps1
```

The generators import the Python reference one directory above the Rust
workspace. New products in existing protocol families refresh the command
allowlists, and regression fixtures reveal changed packet interpretation. New
protocols or altered polling behavior still require a reviewed Rust implementation
and transaction tests before release; recompiling alone cannot establish support.
Hardware configuration allowlists also require separate protocol evidence and
review before accepting new models; catalog generation never extends rate-write
permissions automatically.
