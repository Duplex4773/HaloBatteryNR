# Halo Battery Next validation

Windows 11 x64, Rust 1.98.0 with the MSVC toolchain. Hardware smoke checks use the
connected Razer DeathAdder V4 Pro; other providers remain hardware-unverified in
this Rust port. The original support labels remain in the Python documentation.

## Polling expansion checkpoint, 1 October 2026

The optional control allowlist adds Viper V3 Pro and Viper Mini Signature Edition
dedicated receivers, Logitech PRO X2 Superstrike and a narrowly scoped MCHOSE
A7 V2 Ultra+ wireless route. MCHOSE requires receiver 3837:100B, paired model
4021 and firmware 5.46.2.4; only the wireless rate nibble changes in its saved
configuration block. Unknown models/firmware and ambiguous receiver routes are
refused. Logitech advertised capabilities are intersected with connection
ceilings: C54D permits up to 8000 Hz, while C53A and direct USB are conservatively
limited to 1000 Hz. Superstrike on C53A is refused. Mini Signature Edition 8K
requires suitable firmware. See the three polling evidence documents for exact
wire facts, limits and primary sources.

Formatting, strict workspace Clippy, the optimized release build and **404 Rust
tests** pass; one optional timing test remains ignored. The upstream audit still
accounts for all **459 IDs**: **366 mapped**, **36 tested intentional differences**
and **57 retired Python details**. New synthetic regressions exercise extended
Razer settling/readback, exact Logitech model/receiver ceilings, MCHOSE full-byte
preservation and paired identity, malformed/late/unstable replies, changed
profiles/configurations, uncertain SETs, cancellation/deadlines, duplicate
collections and enumeration changes during an open session. The app's gaming
guard refuses Apply before enumeration for every polling provider.

The source/import API restrictions pass and the manifest remains
`asInvoker`, `uiAccess=false`. Configuration uses the existing guarded user-mode
HID worker, explicit Apply, bounded operations and no automatic rate writes.
Source and executable privacy checks contain no build-account name or profile
path. These checks do not establish anti-cheat vendor approval or immunity from
policy changes. **No physical polling SET was performed for this expansion**;
all newly added routes remain hardware-unverified locally. The earlier
user-reported DeathAdder V4 Pro wireless results are unchanged.

Native tests retain automatic Windows app appearance, keyboard navigation,
history modes, settings and simulated opt-in/Read/Apply/Restore/no-startup-write
behavior. Forty titlebar-close/reopen cycles pass, including duplicate-launch
behavior, with **45 USER / 107 GDI** handles at cycles 1, 20 and 40. A separate
**15.05-second** animated background simulation with one device and controls
enabled averaged **6.01 MiB** private memory and **0.104%** of one logical core.
Explorer-restart and resume notification replays pass, as does graceful status
flushing. This short smoke check does not re-establish the five-minute CPU target
or replace physical hardware validation.

The portable executable is **2,719,232 bytes (2.59 MiB)**, SHA256
`3CB9B4676BE5BA9B1DC09A8E354B147FB6F75C71D70930F7613E44102FE4F5B2`.

## Automatic appearance correction, 1 October 2026

The user's report exposed two gaps in the earlier appearance validation: the
dashboard read the nonexistent `AppsUsesLightTheme` instead of Windows
`AppsUseLightTheme`, and the native class's white brush could overwrite its
background during nested `BeginPaint` erasure. Both are corrected. High-contrast
detection also now supplies the documented structure size to the native query.

Formatting, strict workspace Clippy, the release build and **380 Rust tests**
pass; one optional timing test remains ignored. A volatile, isolated registry
fixture verifies all four combinations of app/system appearance and missing
values/keys. Fixture value names are independent of production selectors, so
the original spelling error fails the regression. Native palette tests now
exercise client painting with the production class brush, as well as explicit
erasure. These tests never change Windows appearance preferences.

The native dashboard's rendered background matches the machine's actual Windows
app preference at opening and after a settings-change message. Visual inspection
confirms the dark titlebar, background, controls and text. Existing keyboard,
history, settings, simulated polling and repeated titlebar-close checks pass.
Forty dark dashboard close/reopen cycles retained **43 USER / 107 GDI** handles
at cycles 1, 20 and 40; forty Insights cycles retained **14 / 20**. Samples now
wait for child-window teardown after the parent disappears from enumeration,
then synchronize with the monitor. The resource-growth assertion is unchanged.
The portable executable remains **2,705,408 bytes (2.58 MiB)**, SHA256
`556E7E29D72CA2E477BBE1FCC54176E991B750595367B81713557D409D4C69A6`.
Production API/import restrictions and source/release privacy checks pass.

## Earlier dashboard lifecycle, appearance and warning checkpoint, 1 October 2026

Formatting, strict workspace Clippy, the optimized release build and **379 Rust
tests** pass locally; one existing optional timing test remains ignored. The
459-ID upstream audit remains complete at **366 mapped**, **36 tested intentional
differences** and **57 retired Python details**. The executable is **2,705,408
bytes (2.58 MiB)**, SHA256
`AC327F85BF1D490D64D168FA2AF6228B6301D06C8CFD5F24F404F89633D4E1AA`.

The prior portable release reproduced the reopening failure in an isolated
simulation after `WM_SYSCOMMAND/SC_CLOSE`, the title-bar close route. Native
regressions now cover that nested message path, stale handles, external window
destruction, hidden/minimized restoration, tray double-click/menu activation and
cleanup arriving after a replacement window. Forty title-bar close/reopen cycles
pass. A normal duplicate launch opens the original dashboard; a duplicate
`--background` launch exits quietly. Monitoring continues after closure and
graceful exit returns zero.

Dashboard appearance follows Windows app light/dark preferences, independently
of tray appearance. All four pages were rendered in both palettes without
changing system preferences. Native tests check readable dark buttons and combo
arrows, checkbox state, brush release, chart palettes and high-contrast fallback.
Repeated palette changes preserve the same unsaved interval edit and its text.
This is automated native validation, not a screen-reader or multi-monitor audit.

Pixel regressions verify orange at the default **30%** visual boundary, red at
the default **20%** alert boundary, custom thresholds, disabling orange, both
tray themes, charging priority and cached sleeping/stale readings. The actual
Settings Save handler persisted an orange threshold of 35%, then restored 30%.
Notification rules are unchanged. The user confirmed separate device icons;
this change preserves the existing per-device identities.

Existing history, keyboard, settings, device controls, simulated polling and
Insights native checks pass. Across forty warm dashboard cycles USER/GDI handles
stayed at **44/105**; private memory went from **14.75 MiB** to **16.93 MiB**.
Forty warm Insights cycles held handles at **14/18**, with private memory
**5.84–5.87 MiB**. Native-resource tests serialize tests that create WinRT windows
so concurrent fixtures do not contaminate process-wide handle audits.

A **15.05-second** animated background smoke check with one simulated device and
polling controls enabled averaged **6.02 MiB** private memory and **0.104%** of
one logical core. Explorer-recovery and resume messages were replayed and
shutdown flushed state. This short check does not replace the earlier
five-minute measurements below. These checks use invented data and do not
change physical device configuration. Production API/import restrictions and
source/release privacy checks pass.

## Earlier battery-insights checkpoint, 1 October 2026

Formatting, strict workspace Clippy, the optimized release build and **370 Rust
tests** pass locally; one existing optional timing test remains ignored. The
459-ID upstream audit stays complete at **366 mapped**, **36 tested intentional
differences** and **57 retired Python details**. The executable is **2,695,680
bytes (2.57 MiB)**, SHA256
`994ED89B2AD641C364AE6DD014B3CC19ADC3875F1FC3DAF0F83E3B56F03553CA`.

New regressions cover rate/session boundaries, charging through sleep,
unobserved charge inference, malformed/coarse readings, backwards clocks,
percentage jitter, sample/drop thresholds, bounded cycles, legacy databases,
atomic metadata rollback, pruning, shutdown/restart and query-before-flush.
A 43,200-row raw month streams to the same bounded result as the pure builder.
Rate learning accepts fresh readbacks only and never restores evidence from
saved preferences; existing simulation tests still assert no automatic Apply.

Native Insights validation passed Alt+I, two invented rate comparisons, ten
scrollable charge summaries, evidence details, device selection and local
Refresh. Visual inspection confirmed readable details and dated cycle rows.
Forty warm Insights open/close cycles held USER/GDI handles at **14/16** and
private memory at **5.73 MiB**. Existing native history, keyboard, settings,
device controls and simulated polling regressions also passed, with stable
**44/102** handles across forty dashboard cycles.

A **15.01-second** animated background smoke run with one simulated device and
polling controls enabled averaged **6.46 MiB** private memory and **0.416%** of
one logical core. Handles stayed stable and shutdown flushed state. This short
check is not a replacement for the earlier five-minute measurements below.
No physical rate changes or protected game sessions were used by these checks.
Production API/import restrictions and source/release privacy checks pass.

## Earlier tray-preservation checkpoint, 1 October 2026

Formatting, strict workspace Clippy, the optimized release build and **347 Rust
tests** pass locally. The executable is **2,644,480 bytes (2.52 MiB)**, SHA256
`0CF3988FFB3A3F365B7AC6553EE00B2C52B7ED5DA52A1E1A2CF4C52FA7917290`.
Production API/import restrictions and source/release privacy scans pass.

Native UI validation passed keyboard navigation, history view/range selection,
settings, device controls and simulated polling controls. Forty warm dashboard
open/close cycles kept USER/GDI handles at **44/102**; private memory was
**16.32 MiB** at cycle 1 and **17.75 MiB** at cycle 40. Monitoring continued after
dashboard closure, and graceful quit returned zero. These checks used invented
devices and history, without changing physical device configuration.

Recorded Shell-call regressions verify that sleep, wake, theme/settings redraws
and healthy Explorer-recovery notifications retain the same GUID using modify
calls; only a failed recovery modification attempts an add. Hiding/removal and
shutdown still unregister the icon. Razer fake-HID regressions verify wake,
short sleep, sleep beyond five minutes, unknown percentage after expiry, wake
recovery and receiver removal without identity changes. Exclusive-open errors
remain explicit, and backoff never invents an unobserved device.

The original 459-ID coverage audit remains complete: **366 mapped**, **36 tested
intentional differences**, and **57 retired Python implementation details**.
The added difference records keeping known Razer presence after the percentage
cache expires, replacing upstream's empty result. Existing cache-freshness limits
and headset behavior remain unchanged. Recorder tests establish the app's Shell
operations; the user subsequently confirmed that the taskbar placement fix
worked. That user report supplements, rather than follows from, recorder tests.

## Earlier history checkpoint, 1 October 2026

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
