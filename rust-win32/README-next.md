# Halo Battery Next 0.1.0

A Windows 11 x64 Rust/Win32 port of [HaloBattery](https://github.com/HeyOkay/HaloBattery), based on upstream 1.13.0 (`a566a046`). The Python implementation remains in this checkout as the protocol reference. Development uses the `port/rust-win32` branch.

## Run and build

Run `HaloBatteryNext.exe` to open the dashboard. Closing the window keeps monitoring active. The tray menu offers refresh, device controls and Exit. Settings include Windows startup registration, which starts the executable with `--background`.

Configuration, diagnostics, optional `status.json` and SQLite history are stored in `%APPDATA%\HaloBatteryNext`. The app has its own startup entry, singleton mutex and notification identity. It does not import upstream settings. Both applications may run together, but receivers that reject concurrent access report recoverable errors.

Build with Rust and the Visual Studio C++/Windows SDK tools:

```powershell
cd rust-win32
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --release --locked
./tools/package-rust.ps1
```

The portable executable statically links HIDAPI's Windows C backend, bundled SQLite and the MSVC CRT. Native Windows system components provide Win32/WinRT/Direct2D. No Python, .NET, webview or asynchronous runtime is needed by the executable.

## Architecture

| Crate | Responsibility |
| --- | --- |
| `hb-core` | Device readings, validated settings, identity precedence, alerts and awake-use estimates; no Windows or UI dependencies |
| `hb-providers` | Vendor protocols and parsers with injected HID and clock contracts |
| `hb-windows` | HID, Bluetooth, WGI/XInput, native resources, startup, theme and gaming detection |
| `hb-storage` | Atomic JSON and bounded, batched SQLite history |
| `halo-battery-next` | One state owner, two HID workers, one WinRT worker, one storage worker and one native UI thread |

Providers return either successful discovery (possibly empty) or an explicit communication error. Errors retain stale device readings instead of treating failures as disconnects. Immutable snapshots cross to the UI; commands cross back to the engine. Queues and worker counts are bounded. Charging HICON frames are cached; the 100 ms timer runs only while an animated charging icon exists. Dashboard graphics are created on demand and released on close. History queries select only the requested interval and downsample in SQLite.

History retains 30 days, batches writes once per minute and flushes on normal exit. Changed readings are queued immediately; unchanged readings are accepted at most once per minute. Usage estimates need at least 30 minutes of awake discharge and a three-point percentage drop. Coarse readings do not produce estimates.

PlayStation Bluetooth full mode is opt-in. 8BitDo mode switching is disabled. Unknown devices are excluded from command allowlists. Update checking remains disabled pending an independently configured release repository.

## Validation and support

See `docs/provider-parity.md` for provider/test coverage and limitations, and `docs/validation-next.md` for measured size, memory, CPU and native lifecycle checks. Original verified/unverified support labels remain in the upstream README and protocol documentation. Simulated tests do not establish hardware verification. The connected DeathAdder V4 Pro is the initial hardware validation device.

Engineering targets are an executable at most 10 MiB, background private memory at most 30 MiB with one device, and five-minute average CPU at most 0.5% of one logical core without animation or 1% with one animated charging icon. Measurements, rather than the choice of language alone, determine whether these targets are met.

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
cargo build --release --locked
```

The generators import the Python reference one directory above the Rust
workspace. New products in existing protocol families refresh the command
allowlists, and regression fixtures reveal changed packet interpretation. New
protocols or altered polling behavior still require a reviewed Rust implementation
and transaction tests before release; recompiling alone cannot establish support.
