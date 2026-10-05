# Halo Battery Next validation

Windows 11 x64, Rust 1.98.0 with the MSVC toolchain. Hardware smoke checks use the
connected Razer DeathAdder V4 Pro. Inherited parent battery reports and Rust
hardware checks are labelled separately in [device support](device-support.md).

## Source audit, 6 October 2026

The executable is **3,046,400 bytes (2.91 MiB)**, SHA256
`47B6E0388581521DDD572E3CAA926D7B813DFBCB96EA23099CE5961D622D6F42`.
Formatting, strict Clippy, **581 passing tests** (two optional timings skipped),
release build, production API restrictions, privacy checks and the 459-ID baseline
gate pass. Indexed metadata expiry was additionally benchmarked.

The optimized synthetic native fixture averaged **6.14 → 6.02 MiB** private memory
and **0.200% → 0.139%** of one core in three-minute windows before/after forty
dashboard cycles, with native handle non-growth assertions passing. These are
lifecycle phases, not a before/after code benchmark. An additional production
measurement was blocked by the existing single app instance; it was left running.
See the [source audit](source-audit-20261006.md) for fixes, methods and limits.
Package: `0.1.0-source-audit-20261006`.

## Consumer UI refinement, 5 October 2026

The executable is **3,038,720 bytes (2.90 MiB)**, SHA256
`02EC4F3D7B05E12631EF779C3FD1F81548BBA540AC4D05DB75CA462F58A5899B`. Formatting, strict Clippy, **569 passing workspace tests**
(two optional timing tests ignored), release build, production API restrictions,
privacy checks and the complete 459-ID baseline gate pass.

Both themes, expanded Settings and synthetic History were rendered and inspected.
The optimized native fixture's three-minute samples averaged **7.85 → 8.12 MiB**
private memory and **0.21% → 0.08%** of one core around 40 dashboard open/close
cycles. GDI / USER counts stayed **108 / 52 → 108 / 52**. These are simulated
fixture results, not a fresh hardware benchmark. See the [consumer UI audit](ui-audit-20261005.md)
for limits and resource design. Package: `0.1.0-ui-modern-20261005`.

## Upstream 1.14.0 updates, 5 October 2026

The local executable is **3,033,600 bytes (2.89 MiB)**, SHA256
`03AC446FEBE104A8600ADDEB9F07F9CE5890605658B3DC87453F467347F51D54`.
Formatting, strict Clippy, **568 passing tests** (two optional timing tests ignored),
optimized build, production API restrictions and the unchanged 459-ID baseline
coverage gate pass.

New tests cover 62 catalog rows and the added headset/keyboard/mouse protocols,
2,752 separate parser cases, persistent bounded JBL collection and failures,
sound rules/playback contracts without playing sound, portable-directory
selection and lifecycle cleanup. Existing native dashboard/tray tests pass.
The [review](upstream-1.14.0-review.md) records upstream already-covered fixes and
intentional adaptations; the [142-ID delta](upstream-1.14.0-test-delta.md) retains
coverage limits separately from the archived mapping.

No new physical device commands, hardware verification, manual UI walkthrough or
CPU/private-memory benchmark was performed. Historical three-minute figures below
remain separate checkpoints. The unsigned portable package is prepared locally
as `0.1.0-upstream114-20261005`; prepared release assets/workflows are unchanged.

## Dashboard refinement, 4 October 2026

The separate local UI executable is **2,902,016 bytes (2.77 MiB)**, SHA256
`2AB96887905C5386D1B1EDFDFC483240B0E0F84E68C4C9B2EEDF2939A0898172`.
Formatting, strict workspace Clippy, **528 passing tests** (two optional timing
tests ignored), release build, production API restrictions and the complete
459-ID coverage gate pass.

Native dashboard regressions cover light/dark painting, selected navigation,
scrolling and keyboard focus, fixed footers, More options, retained edits and
plain-language errors with original support-report details. Forty open/close
cycles keep closed-window GDI and USER counts at **22 / 22**, with no growth.
Fonts, page controls, chart resources and loaded Insights are released on close.

Devices, Settings and Insights screenshots use the native test's synthetic data;
the existing History screenshot remains the 2 October release capture. Live
computer inspection was stopped with Escape and was not resumed. The PowerShell
UI validation helpers were updated, parsed and compiled, but not run in this pass.
No physical polling changes or new hardware verification were performed.

CPU and private memory were not remeasured for this UI pass. The historical
three-minute figures below remain separate checkpoints. This build is packaged
as `0.1.0-ui-20261004` (application version 0.1.0), separately from the previously
prepared release assets. See the [UI audit](ui-audit-20261004.md).

## Performance and correctness audit, 3 October 2026

The separate local audit executable is **2,863,104 bytes (2.73 MiB)**, SHA256
`FB88F80B1C9D282C318A7A0F87097E15AF76F6BE6C7D1FF65C4545C96DA22446`.
Formatting, strict workspace Clippy, **524 passing tests** (two optional timing
tests ignored), release build, production API restrictions and the complete
459-ID coverage gate pass.

The audit fixes HID access-failure generations/retries, saturated worker waits,
departed-device caches, explicit Logitech unpairing, bounded history recovery,
latest estimate persistence and final shutdown recovery. Calendar sampling keeps
endpoints/gaps, and History defers resizing queries while streaming paint vertices.

Native dashboard, keyboard, Insights, tray theme and tray polling suites pass.
Forty close/reopen cycles retain stable GDI/USER counts; native History tests
verify 500 resize messages retain data and submit one final query. Simulation
sends no physical rate commands. Settings smoke cleanup now restores the exact
shared Windows startup registration; normal monitoring is restored afterward.

Controlled **three-minute** background runs average **5.64 MiB / 0.122%** one-core
CPU with animation off, and **6.02 MiB / 0.165%** with one animated charging icon.
Corresponding baseline results are **5.70 MiB / 0.061%** and **6.00 MiB / 0.174%**.
RAM is effectively unchanged; no general steady-state CPU improvement is claimed.
The [full audit](performance-audit-20261003.md) records accepted workloads,
excluded runs, historical original comparison and remaining limitations.
The previously prepared unsigned release assets are unchanged; this audit build
is packaged separately and has not been published.

## Startup tooltip checkpoint, 2 October 2026

The portable executable is **2,835,456 bytes (2.70 MiB)**,
SHA256 `5B87049516921E69A45ADE8A68132A39A74D8F5C117170237C18A8FD246425C4`.
Formatting, strict Clippy, **508 tests**, release build, production API restrictions
and the 459-ID coverage gate pass.

Supported online battery mice get one guarded startup rate read when controls are
enabled, independent of saved-rate restoration. The native test checks the actual
tray tooltip before any dashboard exists, with restoration both off and on; it
also checks refresh/sleep retention. Read-only startup creates no Restore previous
value. Saved requests never become evidence. Unknown devices, keyboards, sleeping
devices, disabled controls, cancellation and startup-window expiry are excluded.

Startup requests now wait for local worker-queue capacity before being consumed.
Once dispatched, failed or uncertain transactions are not retried automatically.
Tests cover repeated local backpressure and verify that releasing the queue emits
only the intended single Read or Apply. Restore readback suppresses a duplicate
startup read. Existing workers, waits and connection epochs are reused.

The isolated native tray tests pass for read-only startup, startup restoration,
direct selection, Restore previous and explicit Refresh, with the dashboard closed.
All automated rate changes use simulation; these checks establish no new hardware
verification. There is no recurring rate polling or hover I/O.

A separate read-only hardware smoke received verified DeathAdder V4 Pro readback
with startup restoration disabled and without opening the dashboard or flyout.
It used isolated settings, battery discovery and a GET request; no rate changes
were attempted. This does not measure effective USB reporting frequency.

A **120-second** controlled background run used one simulated charging mouse,
animation off, controls enabled, five dashboard cycles and Explorer/resume event
replays. Private memory averaged **5.54 MiB** (peak **14.08 MiB** while native caches
settled); CPU averaged **0.065% of one logical core**. GDI handles remained 17 and
USER handles went from 18 to 17 across the cycles. This is a separate smoke
measurement, not a replacement for the historical comparison workloads below.

## Standalone repository checkpoint, 2 October 2026

The workspace, build scripts and portable output now live at repository root.
Python application source, tests, build scripts and obsolete assets were removed
from the current tree; its source remains available in upstream Git history and
external reference archives. Original MIT and protocol credits are retained.

The root-layout release executable is **2,831,872 bytes (2.70 MiB)**,
SHA256 `46B4B3D0CA50B25586440324A80B8DBB65B31ECA76DD358D7CFBB0DBF584884C`.
Formatting, strict workspace Clippy, **506 tests**, release build and production
API restrictions pass. Both standalone and external-reference coverage checks
validate all 459 parent test IDs. Regenerated catalog and parser fixtures match
the stored baseline; no provider or runtime behavior changed in this refactor.

Native simulation checks exercised Insights selection, refresh and 40 dashboard
open/close cycles. GDI handles remained 18 and USER handles 21 at cycles 1, 20 and
40; private memory was 6,004,736 / 6,004,736 / 6,012,928 bytes. Six native UI
screenshots use synthetic data; capture checks verify dark/light client palettes
and restore the Windows app-theme preference. No configuration packets were sent
to hardware. Portable documentation links, notices and screenshots are packaged.

These lifecycle snapshots are not a new three-minute CPU benchmark. Earlier
performance measurements below retain their original workloads and build hashes.

## Tooltip retention checkpoint, 2 October 2026

The portable executable is **2,831,872 bytes (2.70 MiB)**,
SHA256 `CD46E22E8071BB31E41780DD04277289BE5C3A1CD5E1346EC082FB251D471D10`. Formatting, strict workspace Clippy,
all **506 behavioral tests**, release build and production API checks pass.

The native regression starts with a confirmed startup rate, performs Refresh,
checks that actionable observations are revoked, and checks the actual tray
notification tooltip still contains its last-confirmed rate. A sleeping-device
update also preserves the label. Missing/stale/replaced identities, explicit
writes, failed readbacks and disabling controls still remove it. This fixes
unrelated Windows device updates or Refresh hiding the mouse's historical rate.
No additional queries, timers, workers or automatic writes are introduced.

## Razer verification labels checkpoint, 2 October 2026

The updated portable executable is **2,832,384 bytes (2.70 MiB)**,
SHA256 `1DD4708DCFD580CB6AE34AF2C2C18696970B5F8CCD31D6C17113EC2CA2FB5E54`.
Formatting, strict workspace Clippy, all **506 behavioral tests** and the release
build pass. App-facing protocol-reference labels now use device terminology.
Wireless DeathAdder V4 Pro support records the user's confirmation of all six
supported rates; the wired model and other devices retain their existing status.
Protocol provenance, credits and upstream test identifiers are retained.
No packet formats or polling transactions changed.

## Startup polling and tray tooltip checkpoint, 2 October 2026

The portable executable is **2,831,872 bytes (2.70 MiB)**,
SHA256 `C6D3B021A6B85829C1B4B90074C891B2FF79FC8654352B50FEE1A7296D974923`.
Formatting, strict workspace Clippy, **505 behavioral tests**, optimized release
build, production API restrictions and source privacy checks pass. Two optional
timing tests remain ignored. All 459 upstream IDs remain accounted for.

Startup restore is separately opt-in and defaults off. Tests cover persisted
settings, permission/connection cancellation, expiry, one attempt only, keyboard
visibility and no-op writes. A simulated native launch restores saved 2000 Hz,
displays its confirmed value in the tray menu with the dashboard closed, and
allows a subsequent explicit 1000 Hz change. The normal direct tray selection,
readback and Restore previous regression also passes. No physical SET is sent
by these tests; hardware support labels remain unchanged. Native Settings confirms
the new checkbox persists independently; theme, keyboard navigation and forty
dashboard close/reopen cycles pass with stable USER/GDI handles.

Tooltip tests validate confirmed evidence, failed/stale readbacks, identity and
generation changes, long Unicode names and the fixed Shell buffer. Hover adds
no queries or timers. Initial Windows watcher inventory events no longer trigger
a refresh/invalidation per installed device; real connection events remain active.
Startup uses existing workers and a bounded queue, with no ongoing enforcement.
The earlier three-minute resource measurement below belongs to its checkpoint;
this build has not been assigned a new CPU or memory benchmark.

## Insights and prediction checkpoint, 2 October 2026

That checkpoint's portable executable was **2,811,392 bytes (2.68 MiB)**,
SHA256 `A947E0A6E805A7EB0AD549D7FF7457500D28AF54F87CDE6736CD32B79DB32AEC`.
Formatting, strict workspace Clippy, **493 behavioral tests**, the optimized
release build and production API restrictions pass; two optional timing tests
remain ignored. No dependencies or GitHub workflows were added.
All 459 upstream IDs remain accounted for: 365 mapped, 37 intentional differences
and 57 obsolete. Long unobserved gaps are now explicitly an intentional difference.

Regressions cover estimator sleep/restart rebasing, unknown charging, exact
precision, recent-drain adaptation, full-charge plateau resets, invalid saved
fits, interrupted projection windows, charge evidence, query-time freshness,
provider-local history, availability boundaries, per-device rate sessions and
late completions across permission/connection changes. Usage charts now exclude
long unobserved gaps and known session boundaries rather than inventing time.

Native Insights validation passes rate/charge selection, coverage explanations,
keyboard navigation, local Refresh and forty close/reopen cycles with stable
USER/GDI handles. The existing direct tray rate-selection/readback/Restore test
also passes with the new completion guards. These tests use simulation; no
physical polling-rate SET was sent and hardware support labels are unchanged.

A **180.36-second** background run with one simulated animated charging mouse,
after forty dashboard cycles, averages **6.11 MiB** private memory (6.23 MiB peak)
and **0.069% of one logical core**. USER/GDI handles do not grow. The run also
replays Explorer restart and resume, confirms graceful shutdown, and checkpoints
the learned state before exit. This is a current target check, not a matched
performance comparison with the older optimization checkpoint.

The audit example checks SQLite integrity, row/payload agreement, rate metadata
and retained evidence without writing or emitting hardware identities. Actual
history and its consistent audit snapshot remain local, outside the repository
and portable package. Previously overwritten observations cannot be recovered
from the remaining rows; no historical measurements were fabricated or repaired.

## Direct tray polling checkpoint, 2 October 2026

At this checkpoint, the portable executable was **2,785,280 bytes (2.66 MiB)**,
SHA256 `44F39252F369A1C43A5293077577BA66161D16D8D8049FC5D03EECF463356E6F`.
Formatting, strict workspace Clippy, **461 behavioral tests**, the optimized
release build and production API restrictions pass. Two optional timing tests
remain ignored; the 459-ID upstream coverage mapping is unchanged.

Supported mouse tray menus offer model-based rate choices immediately, without
an initial Refresh. These choices grant no hardware authorization: execution
retains exact-device and connection checks, GET-before, SET and verified readback.
Opening or hovering the menu performs no device I/O and adds no timers. Tests
cover offline/pending states, identity and generation changes, unsupported
devices, checkmarks based only on observations, and abandoned dashboard writes.

The isolated native-menu validator opens and reopens the submenu without reading
a rate, selects 2000 Hz directly, confirms it, restores the observed previous
1000 Hz and optionally refreshes. The dashboard stays closed throughout. The
submenu matches the system app palette; nested-menu ownership, keyboard routing
and fallback styling have regression coverage. No physical rate was changed.

The release also passes the existing dashboard theme, navigation, settings and
polling regression checks, forty dashboard close/reopen cycles and duplicate
launch recovery. USER/GDI counts are stable across the warm dashboard cycles;
eight ordinary tray popup cycles retain 15 GDI handles. Source and executable
privacy checks pass. GitHub workflows remain disabled.

The resource figures below remain measurements of the earlier optimization
build. This feature build has not had another three-minute CPU/memory comparison;
the new menu performs work only when opened or explicitly used.

## Performance follow-up checkpoint, 2 October 2026

At this checkpoint, the portable executable was **2,767,872 bytes (2.64 MiB)**,
SHA256 `3D2CF85FDCDB3B6616431D448BB72B28C5F7B19120B28C11981064290454266F`.
Formatting, strict workspace Clippy, **451 behavioral tests**, the optimized
release build and production API restrictions pass. Two optional timing tests
remain ignored. All 459 upstream test IDs retain their documented mapping.
Source/executable privacy checks pass, and the original Python README section
is unchanged. No workflows were enabled.

Allocation regression tests cover borrowed SQLite text, unchanged status JSON,
bounded Insights processing, empty notification queues, diagnostics and native
vendor-path parsing. Runtime tests verify skipped empty-provider snapshot bursts
without losing device/error updates or the five-second freshness heartbeat.
UI tests cover every icon-render input, borrowed inventory selection and fixed
UTF-16 buffers. Native palette, mnemonic, reentrancy and lifecycle tests pass.

The release passes dashboard/theme/navigation/settings tests and forty
close/reopen cycles; the normal duplicate launch reopens the original dashboard
while a background duplicate stays quiet. The keyboard suite passes another
forty cycles, Read/Apply 250/Restore, rename, Corsair exclusion and battery-only
selectors/status/history. Its settled private memory is **8.59 MiB**, with no
growth in USER/GDI handles. Eight actual tray popup cycles match system app
appearance and retain **15 GDI handles**. These are simulated device tests;
hardware verification labels are unchanged.

Matched **180-second** animated workloads average **6.46 → 5.66 MiB** private
memory, with CPU effectively unchanged at **0.156% of one logical core**. Hardware
measurements average **6.25 → 6.03 MiB**, but the optimized run includes mouse
sleep while the baseline remained online; its lower CPU figure is not evidence
of a code-only speedup. Both record actual battery levels and meet the engineering
targets. See the [follow-up audit](performance-audit-20261002.md) for complete
before/after figures, allocation evidence, remaining candidates and limitations.

## Keyboard controls and tray theme checkpoint, 1 October 2026

At this checkpoint, the portable executable was **2,794,496 bytes (2.67 MiB)**,
SHA256 `3CDFBF17B0AEF715AAD0AAF603E633C32E8DA5942CB60FA1464B726659715864`.
Formatting, strict workspace Clippy, the optimized release build, production
source/import restrictions and **436 behavioral tests** pass. Two optional timing
tests remain ignored. The 459-ID upstream coverage audit remains complete.
Source and executable privacy checks pass; local paths, hardware identities,
settings, diagnostics and screenshots remain outside committed/package files.

Five exact wired Razer keyboard PIDs offer 125/250/500/1000/2000/4000/8000 Hz
through interface 3 and 91-byte feature reports. Tests enforce one keyboard SET,
same-rate no-write, malformed/BUSY/late reports, uncertainty, readback mismatch,
cancellation, reconnect and visibility epochs. Duplicate physical keyboards and
ambiguous collections remain separate or unavailable. Closing Devices revokes
queued keyboard work even when it is reopened before the command queue drains.
All thirteen recognized Corsair PIDs send zero configuration packets, including
forged requests. Existing mouse support boundaries and transactions are retained.

The connected **Corsair K70 RGB Pro** was passively recognized with the exact
maintained-software-session explanation and no configuration commands. No physical
keyboard polling SET was performed. The five Razer keyboards remain locally
hardware-unverified; simulation does not establish effective reporting frequency,
persistence or anti-cheat approval. See [keyboard evidence](polling-keyboard-evidence.md).

The final release passes the simulated keyboard UI suite: rename, 250-Hz
Read/Apply/Restore, disabled Corsair controls, battery-only History/Insights,
one mouse-only status/history inventory and forty dashboard cycles. USER/GDI
counts at cycles 1/20/40 stay **43/107**, with settled private memory **9.66 MiB**
at 5/10/20 seconds after closing. The earlier native dashboard regression also
passes theme, keyboard navigation, settings and mouse-control checks.

Tray popups follow the dashboard's system app palette even while it is closed,
using documented native owner drawing. High contrast retains native system menus.
Pixel tests cover both palettes, selection and disabled rows; mnemonic tests
preserve native IDs and skip disabled items. Reentrant callbacks use immutable
menu state. Forty unit-level popup lifecycles retain bounded GDI resources; the
actual final-release popup matches system appearance and eight open/cancel cycles
retain **17 GDI handles**. No system appearance settings were changed.

Separate **180-second** final-release windows average **6.21/6.21 MiB** private
memory and **0.260%/0.139%** of one logical core for hardware/no-animation and
simulated charging respectively. The animated run warms keyboard inventory and
forty dashboard cycles, then closes Devices. Both meet the engineering targets.
See the [resource audit follow-up](resource-audit.md#keyboard-and-tray-menu-follow-up)
for comparison conditions, peak memory and limitations. An earlier hardware run
with changed settings was excluded from the background comparison; the sampler
now rejects settings changes and dashboards left open at measurement end.

## Resource audit checkpoint, 1 October 2026

At this audit checkpoint, the optimized executable was **2,722,304 bytes (2.60 MiB)**, SHA256
`4638C934130E688F31624E1D5E1B9D936B3E255458BF09D5B1A78BEAC0B1221B`.
Formatting, strict Clippy, the release build and **410 behavioral tests** pass;
two optional timing tests were run separately. Native interaction and Insights
checks each pass 40 dashboard cycles, with stable GDI handles. Separate
180-second hardware and simulated-animation runs average **6.27 / 5.71 MiB**
private memory and **0.130% / 0.208%** of one logical core. See the
[resource audit](resource-audit.md) for the upstream comparison, compiler
experiment, peak memory, lifecycle settling and measurement limits. Earlier
checkpoints below describe their own builds, not the current executable.

## Dashboard control painting checkpoint, 1 October 2026

Native owner-draw and control-color callbacks could reenter while the dashboard
held its mutable state during creation, updates or painting. Their fallback to
default Windows painting left the device selector blank/white and status labels
with light backgrounds in dark mode. An independent, shared immutable theme now
services these callbacks before borrowing application state. Its owned brushes
remain alive through synchronous callbacks and are released when the dashboard
closes. Closed combo fields use the normal surface/text palette; selected popup
rows retain their highlight. Page rebuilds and polling-control updates pause
redraw until the completed controls can repaint, preventing intermediate white
surfaces during tab changes. Initially hidden dashboards remain hidden.

Formatting, strict workspace Clippy, the optimized release build and **405 Rust
tests** pass, with one existing optional timing test ignored. The complete
459-ID upstream coverage audit is unchanged. Native regressions print real
selected combo, static and edit controls while the parent State is borrowed and
assert both their background pixels and visible text in light/dark modes.
Reentrant background erasure also matches the palette. Separate owner-draw tests
cover selected fields, highlighted popup rows and high-contrast colors. All four
dashboard pages were rendered without changing system appearance preferences;
visual inspection confirms the device field and status strip correction.

The release passes keyboard, history, settings and simulated polling checks.
Forty titlebar-close/reopen cycles, normal/quiet duplicate launches and graceful
exit pass. Dashboard samples retain **45 USER / 107 GDI** at cycles 1, 20 and 40.
Forty Insights cycles retain **20 GDI**, with USER counts **16, 17, 17** and private
memory **5.96 MiB** at those samples. Existing unsaved settings survive theme
changes. Source/import API restrictions and source/executable privacy checks
pass. No physical polling changes were requested during this UI validation.

The portable executable is **2,720,256 bytes (2.59 MiB)**, SHA256
`F720422C06B7DFB09AA85ED780C286488C7E55225C4E3B07D22C8A3787F3E39C`.

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

On **2 October 2026**, the user confirmed all supported wireless polling rates
on the DeathAdder V4 Pro: **125, 500, 1000, 2000, 4000 and 8000 Hz**.
The Rust port labels this **hardware verified by user testing**. This supersedes
the partial 1 October transition report. No identifying captures are retained.
Wired operation, reconnect/power-cycle persistence and effective USB frequency
remain unverified. This is not manufacturer or anti-cheat certification.

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
./tools/validate-native.ps1 -Seconds 180 -Hardware
./tools/validate-native.ps1 -Seconds 180 -Animation -Cycles 40
./tools/validate-ui.ps1
./tools/validate-configuration.ps1
./tools/validate-native.ps1 -Seconds 180 -PollingControls -Cycles 40
./tools/validate-native.ps1 -Seconds 180 -PollingControls -Animation -Cycles 40
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
