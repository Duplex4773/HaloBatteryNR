# Halo Battery Next validation

Windows 11 x64, Rust 1.98.0 with the MSVC toolchain. Hardware smoke checks use the
connected Razer DeathAdder V4 Pro; other providers remain hardware-unverified in
this Rust port. The original support labels remain in the Python documentation.

## Local regression checkpoint

Formatting, strict workspace Clippy, the optimized release build and **261 Rust
tests** pass locally. The upstream coverage gate classifies all **459 IDs**:
**367 mapped**, **35 tested intentional differences**, and **57 retired Python
implementation details**, with no partial, manual or unmapped IDs. This is an
explicit coverage audit, not a claim that 459 separate Rust tests passed.

The portable executable is **2,478,080 bytes (2.36 MiB)**. Windows dependency
inspection shows system DLLs only. Build-profile paths and the local account name
have zero occurrences in its embedded strings; source scans also pass. The
release script remaps build paths, and packaging retains required notices.

## Latest five-minute measurements, 30 September 2026

Both runs used one device with the dashboard closed after 40 complete open/close
cycles. CPU percentages describe one logical core.

| Workload | Duration | Average private memory | Peak private memory | Average CPU |
| --- | ---: | ---: | ---: | ---: |
| Razer hardware, animation off | 300.67 s | 6.90 MiB | 7.16 MiB | 0.073% |
| Simulated charging device, animation on | 300.75 s | 6.19 MiB | 6.99 MiB | 0.171% |

Warm USER/GDI handles were 15/15 before and 14/15 after the hardware cycles;
44/102 before and 43/102 after the animated cycles. Both runs exited gracefully,
flushed history and cleared the exported status device list.

These sustained measurements preceded the final history change that records
precision, charging evidence and model metadata changes immediately. After that
change, both 15-second resource smoke checks passed on the final build, followed
by the complete native interaction script. Its 40-cycle run kept USER/GDI handles
at **44/102** at cycles 1, 20 and 40. Private memory before/after those cycles was
15.34/15.65 MiB. Quiet mode, PlayStation opt-in, provider switches, threshold
inheritance, rename/hide/icon controls, theme notification handling and keyboard
navigation passed; quit returned zero.

The fresh hardware probe reported the DeathAdder V4 Pro online with an exact
31% reading and not charging. Simulated animation does not verify physical
charging behavior or other vendor hardware.

## Earlier measurements

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
./tools/build-rust.ps1
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
