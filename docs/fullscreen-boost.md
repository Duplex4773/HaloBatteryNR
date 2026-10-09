# Automatic fullscreen boost

For supported battery mice, enable polling-rate changes in Settings, then open
Devices and enable **Boost this mouse in fullscreen games**. Choose a supported
rate above 1000 Hz and click **Save boost settings**. Each mouse is opt-in;
existing installations leave this feature off. Wired keyboards retain manual
controls and do not gain background discovery.

The boost group is below the manual polling controls; scroll down on Devices.
The corrected layout includes this group in the scroll range and updates that
range after hardware replies. Native tests verify the switch, selector, Save
button and help text can all be scrolled into view in both themes.

The app checks Windows' public Shell notification state every five seconds only
when the feature is configured or a restoration is pending. It uses the existing
workers and wakeups. No new thread, process monitoring, hook or recurring HID
rate query is added. Startup rate checks/restoration finish before boosting.

- Ten seconds of stable Windows-reported Direct3D fullscreen permits a boost.
- Fifteen seconds of a stable desktop permits restoration. Brief Alt-Tab changes
  do not immediately switch rates. Sampling and worker availability add latency.
- The current rate is read first. Equal or higher rates are left alone. Every
  write reads the rate again and verifies the result afterwards.
- Restoration only writes if the mouse still reports the app's boost rate.
  A different rate set elsewhere is left alone. Manual changes take priority.
- Failed or uncertain writes are not repeatedly retried in the same fullscreen
  session. No background enforcement corrects rate changes made by other tools.

The conservative trigger is `QUNS_RUNNING_D3D_FULL_SCREEN`. It can miss borderless
games and can include other exclusive fullscreen Direct3D applications. Busy,
presentation, locked and unknown states authorize neither boosting nor restoring.
See [Microsoft's state definitions](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/ne-shellapi-query_user_notification_state).

Keep the app running for restoration. Disabling only the per-device boost allows
an already verified boost to restore when the desktop is stable. Disabling all
polling controls, exiting, suspension, reconnects or identity changes revoke
automatic work and can leave the hardware at its current rate. The previous rate
is held in memory, not saved as a new startup preference. Check or manually apply
your preferred rate afterwards; a failed/uncertain change also needs a manual check.

Ordinary user-mode HID and the existing exact device allowlists are retained.
There are no drivers, elevation, input interception, synthetic input, game-process
access or mode changes. These boundaries do not guarantee anti-cheat approval;
see [implementation restrictions](anti-cheat.md). Automated tests use simulated
hardware and do not establish physical fullscreen-transition verification.

## Local validation — 9 October 2026

- Formatting, strict workspace Clippy and 604 tests passed; two optional timing
  tests remain ignored. The upstream coverage map still accounts for all 459 IDs.
- Tests cover opt-in settings, entry/exit delays, brief Alt-Tab, cancellation,
  manual override, already-higher rates, reconnect/identity changes and uncertain
  writes. Razer, Logitech and MCHOSE tests reject changed rates before a SET.
- Native checks cover saving the option, retaining it through device edits,
  batteryless keyboard exclusion, both themes and 40 dashboard open/close cycles.
- The optimized portable executable is 3,102,720 bytes (2.96 MiB). Source,
  executable and package privacy checks passed. Production imports contain none
  of the checked process-memory, injection, input-hook or synthetic-input APIs.
- The user verified the DeathAdder V4 Pro switching from 1000 to 2000 Hz during
  fullscreen use and reverting to 1000 Hz after minimizing, on 9 October 2026.
  This confirms that transition on the tested setup, not every game or device.
  Automated validation itself performed no real HID rate writes.
- Before the follow-up below, a 180-second optimized native fixture sample after 40 dashboard cycles averaged
  0.130% of one logical core and 6.16 MiB private memory (6.33 MiB peak). This used
  a simulated mouse, closed dashboard, no charging animation and boost disabled.
  It checks the default background path, not enabled-boost or live gaming cost;
  it is not a matched performance comparison with previous builds.

### Quick performance follow-up

- Skip device-list resolution and cloning during debounce, pending work, settled
  fullscreen sessions and desktop idle time without an owned boost to restore.
- Continue checking connection generations independently so this optimization
  cannot revive a target after disconnection. New per-device opt-ins remain eligible.
- Stop boost-specific Shell sampling during suspension/shutdown and after opting
  out of a preflight read that did not change the rate. Keep the five-second cadence
  and all per-exchange authorization checks.
- Regression tests, strict Clippy, formatting and the production release build
  pass. No new resource measurement was performed for this quick follow-up; the
  earlier disabled-feature sample above does not quantify these savings.

Successful boost-setting saves and verified automatic rate changes retain their
success messages; they are not converted into generic failures. Native UI and
message-formatting regressions cover these confirmations.
