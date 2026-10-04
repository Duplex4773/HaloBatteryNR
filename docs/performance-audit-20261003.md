# Performance and correctness audit — 3 October 2026

This audit starts from `9a22bc0` and reviews the Rust runtime, Windows integration,
providers, storage, core models, native dashboard and tray. The changes address
specific failure cases and repeated work. They retain provider packets, hardware
allowlists, ordinary battery refresh intervals, charging animation, native theme
behavior and the one-time polling-control policy.

## Confirmed findings and changes

| Priority | Finding | Change and evidence |
| --- | --- | --- |
| High | Failed HID opens advanced the global connection generation. Successful sibling readings and unrelated in-flight providers were then rejected; stale completions could be retried immediately. | Access failures expire the affected vendor's enumeration cache without inventing a connection transition. Stale completions respect the provider delay unless an explicit refresh is already scheduled. Regression tests cover sibling results, cancellation, actual epoch changes and recovery. |
| High | A failed SQLite flush retained its batch, but subsequent observations kept extending the buffer past its intended 4,096 limit. | Flush before admitting a record at capacity; failure leaves pacing and identity state unchanged. A persistent-write-failure regression verifies the hard limit and recovery of queued records. |
| Medium | A failed multi-device history batch discarded its unprocessed tail. | The storage worker retains the latest unprocessed observation per device, up to 512 devices, and retries during minute maintenance and shutdown. Prolonged outages can coalesce intermediate observations; this is bounded recovery, not lossless unlimited buffering. |
| Medium | Shutdown did not retry an initially unavailable database or save deferred estimate state; recovery could also replay an older deferred checkpoint after a newer save. | Periodic maintenance and shutdown share a bounded reopen/flush path. Each incoming estimate supersedes its deferred predecessor; successful saving clears it. Tests cover final database recovery and failed-to-newer checkpoint ordering. |
| Medium | Full HID/WinRT worker queues left overdue providers in the wait deadline, causing a 20 ms retry/wake loop. | Exclude blocked queues from the deadline calculation; completions, commands, connection events and the existing freshness heartbeat still wake the owner. A mixed-queue scheduler regression checks independent availability and suspended state. |
| Medium | Native watcher events invalidated HID results but only scheduled Bluetooth/XInput recovery. | Schedule HID providers as well, with the existing event debounce and quiet-mode cadence. Tests cover ordinary and quiet scheduling without postponing an earlier due time. |
| Medium | Several provider protocol hints, last readings and Bluetooth levels retained departed identities indefinitely. | Clean hints and readings after successful discovery, preserve established sleep/grace rules and retain caches on enumeration failure. Churn tests cover 1,000 obsolete entries, paired sleeping Bluetooth nodes and Razer long-sleep identity. |
| Medium | An explicit Logitech empty-slot response retained the former unit's sleeping reading. | Forget the old slot unit immediately; a scripted unpair/re-pair test exercises the existing replacement identity queries. |
| Medium | Calendar downsampling could omit the actual first/last observation, especially on a flat plateau. Extreme public time intervals could overflow arithmetic. | Reserve observed endpoints, deterministically select extrema/gaps and calculate spans safely. Tests cover small point budgets, flat/varied data, unknown gaps and extreme intervals. |
| Medium | Dragging the History window submitted database queries for every resize message and repeatedly cleared the visible series. | Paint the existing series during the native sizing loop and request a new downsample once it ends. Minimize does not query. A native callback test sends 500 resize messages and verifies one final query and retained data during the drag. |
| Low | Chart painting allocated a second buffer containing up to twice the sampled point count. Closed-dashboard updates also copied identity lists and created an unused startup font. | Stream chart vertices with constant auxiliary memory, compare borrowed identity iterators and create the dashboard font on demand. Existing step/unknown/sleep/usage chart tests verify rendering semantics. |
| Low | The isolated Settings smoke saved the global Windows startup registration using its test executable path. | Save and restore the exact registry value and type in test cleanup, including failure cleanup. The user's original registration is restored after native validation. |

The fixed chart iterator has no heap-backed vertex collection. Previously that
buffer reserved `(2 × sample_count + 1) × size_of(HistoryVertex)` bytes per paint;
this is an allocation saving, not an equivalent reduction in resident RAM.
Charts and native resources still release on dashboard close. No new recurring
timer, service, asynchronous runtime or per-device thread is introduced.

## Resource measurements

Matched baseline/audited measurements use **180-second windows**. The baseline is
the previously validated portable binary from `9a22bc0`; the audited binary uses
the same locked dependencies, compiler and release profile. The sampler records
private committed memory and CPU as a percentage of one logical core, excluding
its own work. Builds/tests run outside the accepted sampling windows.

Baseline: **2,835,456 bytes (2.70 MiB)**, SHA256
`5B87049516921E69A45ADE8A68132A39A74D8F5C117170237C18A8FD246425C4`.
Audited: **2,863,104 bytes (2.73 MiB)**, SHA256
`FB88F80B1C9D282C318A7A0F87097E15AF76F6BE6C7D1FF65C4545C96DA22446`.
The corrective code adds 27,648 bytes, about 0.98%; no dependency or compiler
profile was changed. The local audit package still reports application version
0.1.0 and is separate from the previously prepared release.

Each run uses isolated settings/history, one simulated charging battery device,
60-second battery refresh, status export enabled, polling controls disabled and
the dashboard/menu closed. Animation-off runs use two dashboard warmup cycles;
animation-on runs use forty. Explorer and resume checks replay their Windows
messages rather than restarting Explorer or physically suspending the machine.
Startup and interaction are outside the measured CPU window.

| Workload / metric | Before this audit | Audited build |
| --- | ---: | ---: |
| Animation off: average private memory | 5.70 MiB | 5.64 MiB |
| Animation off: peak private memory | 14.09 MiB | 13.94 MiB |
| Animation off: CPU, one logical core | 0.061% | 0.122% |
| Animated charging: average private memory | 6.00 MiB | 6.02 MiB |
| Animated charging: peak private memory | 14.43 MiB | 14.34 MiB |
| Animated charging: CPU, one logical core | 0.174% | 0.165% |
| Portable executable | 2.70 MiB | 2.73 MiB |

Steady background RAM is effectively unchanged. Idle CPU was higher in this run;
the animated difference is small. These short single windows have no confidence
intervals and establish **no general steady-state CPU or RAM improvement**. They
do confirm that this tested workload remains comfortably below the 10 MiB
executable, 30 MiB private-memory and 0.5%/1% one-core CPU targets. The strongest
changes address denied access, saturated queues, write failures, device churn and
interactive resize work, which ordinary healthy idle runs do not reproduce.

One initial baseline animation window overlapped a follow-up build/test run and
was excluded and repeated. An intermediate audited idle result was also excluded
after final recovery fixes; the table uses only the final executable. Native
Windows watchers remain active during simulation, and their live event workload
can vary between runs. No physical battery or polling-rate validation was added.

For context, the original Python reference below comes from the
[earlier audit](resource-audit.md); it was **not rerun here**. It includes observed
helper processes and does not include Rust's dashboard warmup. These figures
support a substantial memory advantage on this host, rather than a precise
cross-language CPU speedup.

| Simulated charging metric | Original Python 1.13.0, historical | Audited Rust port |
| --- | ---: | ---: |
| Average private memory | 33.10 MiB | 6.02 MiB |
| CPU, one logical core | 0.226% | 0.165% |

Explorer/resume message replays and clean status/history shutdown pass in all
accepted runs. Animated resource warmup holds 104 GDI handles before/after forty
cycles in the baseline and 104/105 in the audited run; USER counts fall 48 to 47
in both. A one-handle difference after event replay is separate from the stable
cycle counts in the native UI suites below. The sampler checks every 200 ms and
can miss very short-lived activity. These measurements are not worst-case bounds.

## Remaining opportunities and limitations

- Time-used history still makes two streaming passes and fully deserializes each
  retained reading. A validated borrowed representation could reduce transient
  allocations, provided malformed data still breaks continuity.
- Native HID enumeration still performs vendor-scoped censuses. Sharing a census
  needs tested generation, TTL and inaccessible-collection recovery semantics.
- WinRT watchers can be started more selectively when integrations are disabled;
  enable/disable and resume behavior would need lifecycle coverage.
- The learned estimator expires old device entries during load, rather than
  continuously during a long-running process. Its sample count is bounded per
  device, but many distinct historic device keys can still accumulate. A running
  retention/cap policy needs to preserve useful estimates and clock-change rules.
- Replacing a Logitech receiver slot between observations without ever seeing an
  empty-slot response can leave cached unit identity until an identity refresh.
  This needs an explicit refresh policy and matching hardware validation.
- The estimator rejects readings older than its recorded wall timestamp. A real
  backward wall-clock correction can therefore pause learning until time catches
  up. Distinguishing a clock correction from delayed provider data requires an
  engine-level clock boundary; the audit preserves rejection of stale reports.

These remaining items are recorded openly rather than presenting the audit as a
proof that the project has no bugs or possible optimizations. Simulated runs use
isolated settings/history; the original startup registration and running monitor
are restored after validation. Hardware verification labels and protocol/license
credits remain intact.

## Validation and reproduction

Formatting, strict workspace Clippy, **524 passing tests** (two optional timing
tests ignored), the release build, production API restrictions and the complete
459-ID upstream coverage gate pass. Native suites pass system appearance, History
axis/range selection, keyboard navigation, per-device settings, keyboard-only
battery exclusion, simulated polling transactions, Insights selection and
dashboard reopen behavior. Dashboard, keyboard and Insights suites each exercise
forty close/reopen cycles. GDI/USER counts stay stable at cycles 1, 20 and 40:
104/49 for the general dashboard and 18/18 for Insights. The tray theme suite
retains 14 GDI handles across eight popup cycles. Rate tests send no configuration
packets to physical hardware.

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python tools/merge-coverage.py --check --require-complete
./tools/build-rust.ps1
./tools/validate-configuration.ps1
./tools/validate-native.ps1 -Seconds 180 -Cycles 2 -SamplerPython python
./tools/validate-native.ps1 -Seconds 180 -Animation -Cycles 40 -SamplerPython python
./tools/validate-ui.ps1
./tools/validate-keyboards-ui.ps1
./tools/validate-insights-ui.ps1
./tools/validate-tray-menu.ps1
./tools/validate-tray-polling.ps1
```

Run native suites with other Halo Battery Next instances closed, then restore
normal monitoring. Raw measurements, screenshots, paths and process identities
stay in ignored `validation-local`. No GitHub workflow was enabled and no release
was published. The previously prepared unsigned release assets remain separate.
