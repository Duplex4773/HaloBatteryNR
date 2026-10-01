# Halo Battery Next validation

Windows 11 x64, Rust 1.98.0 with the MSVC toolchain. Hardware smoke checks use the
connected Razer DeathAdder V4 Pro; other providers remain hardware-unverified in
this Rust port. The original support labels remain in the Python documentation.

## History checkpoint, 1 October 2026

Formatting, strict workspace Clippy, the optimized release build and **342 Rust
tests** pass locally. The optional synthetic timing test also passes when run
explicitly. The executable is **2,642,944 bytes (2.52 MiB)**, SHA256
`A7D3415C8F9BE075C7A70E79C65EA40BDA18BBDD3D9255A331BC952342DF86CF`.
Production API/import restrictions and source/release privacy scans pass.

History now defaults to estimated awake-use time, with 2/8/24-hour ranges;
calendar time remains available with 24-hour/7-day/30-day ranges. Sleep, stale,
unknown and charging observations pause use time. Calendar history carries the
last known level through missing observations and can seed the selected interval
from a retained earlier sample. Display coordinates remain separate from real
observation timestamps, and graph changes do not modify stored readings or the
discharge estimator. Tests cover boundary seeding, malformed levels, sleep/wake,
charging, empty history, bounded sampling, and timing independent of graph width.

A synthetic 43,200-row history (one reading per minute over 30 days) queried in
**87.34 ms** in the optimized storage test on this machine. The two indexed
streaming passes retain bounded display memory; the result is a local latency
measurement, not a guarantee for every history database.

Native UI validation used an invented 18-hour history containing awake, sleeping
and unavailable periods. Both graphs rendered continuously; the use axis
compressed it to about five hours. Default mode, switching views/ranges,
independent range selections, keyboard navigation and existing device controls
passed. Forty complete warm dashboard cycles kept USER/GDI handles at **44/102**;
private memory was 16.14 MiB at cycle 1 and 17.72 MiB at cycle 40. Graceful quit
returned zero. No physical device configuration was used by these checks.
A 15.03-second animated background smoke run averaged 6.01 MiB private memory
and 0.104% of one logical core, with stable handles and graceful shutdown. The
earlier five-minute measurements below precede this history revision.

## Earlier polling-control checkpoint, 1 October 2026

Formatting, strict workspace Clippy, the optimized release build and **325 Rust
tests** pass locally. The original 459-ID coverage audit remains complete. The
new regressions cover explicit control requests, exact packets, paired-unit and
collection ambiguity, stale/late responses, partial writes, onboard mode,
connection changes, full queues, disable/suspend/resume races and shutdown.
Polling writes also fail closed for unknown or unavailable Windows Shell state;
this restriction leaves the battery notification suppression policy unchanged.

The executable is **2,614,784 bytes (2.49 MiB)**, SHA256
`3BD2FA8B4B8A2392EA1B11FF404F1E260C51198C5DD513C69EAFF08DB867A7EA`.
The production API/import gate
passes: no process-memory access, injection, input hooks, synthetic input or
service installation imports; the manifest remains `asInvoker`. Privacy scans
find no local account or profile path in source or release strings.

Native interaction validation on the final executable passed opt-in, read1000,
Apply8000, Refresh, Restore1000, preference-preserving rename, restart with a
saved8000 selection while the simulated device remains1000, and disable. All
rate-changing native automation uses the isolated simulation. Keyboard/theme
checks and 40 open/close cycles passed; warm USER/GDI handles stayed **44/102**.
Private memory across complete warm cycles was 14.69 MiB at cycle 1 and 16.12 MiB
at cycle 40, remaining below the 30 MiB target during dashboard interaction.

During the earlier automated hardware probe, the DeathAdder receiver was
discoverable, but its battery query returned a device timeout. That probe did not
attempt a physical polling SET. Superlight2/DEX polling and independently measured
USB frequency remain **unverified locally**; reference hardware captures and
synthetic tests do not change those labels. No protected game or anti-cheat
session was used by the automated checks to claim compatibility.

### User-reported DeathAdder V4 Pro wireless validation

On **1 October 2026**, the user reported that polling changes worked wirelessly
in the sequence **1000 Hz → 8000 Hz → 125 Hz → 2000 Hz**. Record this as
**user-reported working** for those wireless configuration transitions. No
identifying device details or captures are retained. Wired operation, 500/4000
Hz, persistence across reconnect/power cycle, effective USB reporting frequency
and anti-cheat compatibility were not established by this report. Upstream
verified/unverified support labels remain unchanged.

### Five-minute polling-control measurements

One simulated device, polling controls enabled, dashboard closed after 40
open/close cycles. CPU percentages describe one logical core.

| Workload | Duration | Average private memory | Peak private memory | Average CPU |
| --- | ---: | ---: | ---: | ---: |
| Animation off | 300.72 s | 6.20 MiB | 6.46 MiB | 0.109% |
| Animation on | 300.64 s | 6.48 MiB | 6.65 MiB | 0.187% |

Warm USER/GDI handles changed from 14/15 to 13/15 without animation, and from
44/102 to 43/102 with animation. Both runs exited gracefully and flushed status.
All proposed resource targets pass for these workloads. The measured executable
SHA256 was `D123F99629E3B286BAC239BBED74D2A9F1C3746F0FFCF4F7C25064E0DE7D0E1C`.
These sustained runs precede the final conservative Shell-state restriction,
which only changes permission checks on explicitly requested configuration.
They do not measure physical polling writes or effective USB frequency.
The final release then passed a 15.04-second animated smoke run with polling
controls enabled: 6.25 MiB private memory and 0.104% of one logical core, stable
native handles and graceful shutdown. This short check is not a five-minute
resource measurement.

## Previous battery-port regression checkpoint

Before polling controls, formatting, strict workspace Clippy, the optimized
release build and **261 Rust tests** passed locally. The upstream coverage gate classifies all **459 IDs**:
**367 mapped**, **35 tested intentional differences**, and **57 retired Python
implementation details**, with no partial, manual or unmapped IDs. This is an
explicit coverage audit, not a claim that 459 separate Rust tests passed.

The portable executable is **2,478,080 bytes (2.36 MiB)**. Windows dependency
inspection shows system DLLs only. Build-profile paths and the local account name
have zero occurrences in its embedded strings; source scans also pass. The
release script remaps build paths, and packaging retains required notices.

## Earlier five-minute measurements, 30 September 2026

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
./tools/validate-configuration.ps1
./tools/validate-native.ps1 -Seconds 300 -PollingControls -Cycles 40
./tools/validate-native.ps1 -Seconds 300 -PollingControls -Animation -Cycles 40
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
