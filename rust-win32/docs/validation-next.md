# Halo Battery Next validation

Windows 11 x64, Rust 1.98.0 with the MSVC toolchain. Hardware smoke checks use the
connected Razer DeathAdder V4 Pro; other providers remain hardware-unverified in
this Rust port. The original support labels remain in the Python documentation.

## Measurements recorded on 30 September 2026

The initial optimized executable was **2,384,384 bytes (2.27 MiB)**. `dumpbin
/dependents` showed Windows system DLLs only: HIDAPI, SQLite and the MSVC CRT are
linked into the executable. The portable package retains upstream and dependency
notices.

A 300.65-second background hardware run, with animation disabled and status export
enabled, averaged **6.55 MiB private memory** and **0.229% of one logical core**.
Peak private memory was 6.76 MiB. This passes the proposed 30 MiB / 0.5% targets
for the measured device and machine; it is not a guarantee for every device mix.

Opening the dashboard initializes process-wide DirectWrite/COM/IME resources.
The first cold lifecycle run gained seven USER and GDI handles and therefore
failed its original cold-versus-warm leak assertion. A subsequent 40-cycle native
run compared complete warm cycles: GDI remained 102 and USER remained 47 at
cycles 1, 20 and 40, with one simulated animated device. Thirty cached charging
icons account for the higher animated handle baseline. The validator now retains
the cold sample and checks for growth after one complete warmup cycle.

An updated **2,427,392-byte (2.31 MiB)** executable completed a 300.73-second run
with one simulated animated charging device: **6.47 MiB average private memory**,
**6.96 MiB peak**, and **0.151% of one logical core**. The dashboard was closed
during sampling after 40 open/close cycles. Warm GDI handles remained 102; USER
handles changed from 44 to 43. Graceful exit cleared the status device list and
set `running=false`. This passes the animated 30 MiB / 1% targets for this run.

Resource measurements describe the tested executable and workload; subsequent
protocol corrections require a fresh release build. CPU while physically charging
the Razer device and resource use with multiple devices are not yet measured.

## Reproduce

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --release --locked
python tools/merge-coverage.py --check
./tools/validate-native.ps1 -Seconds 300 -Hardware
./tools/validate-native.ps1 -Seconds 300 -Animation -Cycles 40
./tools/validate-ui.ps1
./tools/package-rust.ps1
```

The native validator uses an isolated data folder under ignored
`validation-local/`, samples process private bytes and process CPU divided by
wall time (one logical core), opens/closes Devices/History/Settings, and verifies
graceful shutdown and the status-file contract. The UI smoke additionally checks
Tab and Alt+D/H/S navigation, persisted settings and device controls, and captures
the app's own windows. Native icon/chart tests check resource ownership.

Explorer recovery and resume are exercised by replaying `TaskbarCreated` and
`PBT_APMRESUMEAUTOMATIC`. These checks exercise the application's handlers;
they do not claim an actual Explorer process restart or machine suspend.
Cross-monitor DPI migration, live taskbar/theme changes and physical reconnect
behavior need additional manual hardware validation. The manifest declares
PerMonitorV2 awareness and the window handles DPI and settings notifications.

See [coverage-summary.md](coverage-summary.md) for the exact upstream test audit.
Passing parser fixtures and fake-HID scenarios do not establish full provider
parity or hardware verification. The explicit complete-parity gate remains
`python tools/merge-coverage.py --check --require-complete`.
