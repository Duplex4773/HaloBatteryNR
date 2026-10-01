# Polling controls and anti-cheat boundaries

Halo Battery's optional polling-rate controls configure an allowlisted mouse
through ordinary user-mode Windows HID access. This document describes the
implementation boundaries; it is not an anti-cheat approval or compatibility
guarantee. Include it with portable release documentation and notices.

## Enforced behavior

- Polling controls are off by default. Enabling them permits explicit reads and
  user Apply actions; saved choices are not applied at startup, reconnect,
  resume, or on a timer. There is no automatic rate enforcement.
- The configuration worker checks Windows Shell notification state through
  `SHQueryUserNotificationState`. Apply is refused for
  `QUNS_RUNNING_D3D_FULL_SCREEN`, `QUNS_PRESENTATION_MODE`, `QUNS_BUSY`, or
  `QUNS_APP`. Failed queries and unknown states also block writes. Only
  `QUNS_NOT_PRESENT`, `QUNS_ACCEPTS_NOTIFICATIONS` and `QUNS_QUIET_TIME` permit
  them; battery quiet-mode detection retains its existing behavior.
  This check does not inspect a game process. It runs before the configuration
  operation and again before each HID exchange in an Apply transaction. The
  point-in-time signal does not detect every game or every transition. The state
  definitions follow [Microsoft's documentation](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/ne-shellapi-query_user_notification_state),
  checked 2026-10-01; a Windows Store app could itself be a game.
- The control path sends only the allowlisted polling get/set packets and the
  identity/capability feature queries needed to select the exact device.
  Stable identities, collection shape, connection epoch, cancellation, deadline,
  supported rate, and readback must pass the respective guards. Partial writes
  and uncertain verification remain failures with the available observed rate.
  Razer dedicated receivers, exact Logitech HID++ model pairs and the scoped
  MCHOSE model/firmware use this same guarded worker; adding battery catalog
  entries never adds configuration permission. MCHOSE stores the rate in a
  larger block: the app alters only its rate nibble and requires all other bytes
  to remain identical. It does not create or modify button/macro assignments,
  switch profiles or execute stored actions. Its firmware-update collection is
  explicitly excluded.
- Settings and suspension revoke configuration permission synchronously, even
  with a full command queue. A latest-settings mailbox and acknowledged epochs
  prevent old jobs or lifecycle events from reviving permission. Shutdown drains
  results immediately rather than blocking while waiting for a Quit queue slot.
- The Windows executable uses `asInvoker` with `uiAccess=false`. Polling control
  does not request administrator elevation, install a driver, or start a service.
  The statically linked HIDAPI Windows C backend uses the Microsoft HID stack;
  HID feature IOCTLs target a device handle, not game or system memory.
- There is no game-process access, memory read/write, injection, input hook,
  synthetic input, macro feature, custom kernel driver, anti-cheat service
  manipulation, or evasion in this configuration path. No whitelist claim is made.

## What this does and does not establish

BattlEye distinguishes cheating or intentional protection bypass from ordinary
third-party tools, while noting that developer policy can restrict specific
software and some programs can trigger a kick. It also describes blocking
hardware utilities whose kernel drivers have exploitable security issues.
These statements do not approve Halo Battery or guarantee acceptance by any
game or anti-cheat vendor. [BattlEye's official FAQ](https://www.battleye.com/support/faq/)
(third-party software and hardware utility entries, checked 2026-09-30).

This implementation avoids those kernel-driver and game-access mechanisms.
Vendor classifications, game policy, future software changes, and interactions
with other installed utilities remain outside this source audit. Windows Shell
state is an additional restriction, not a reliable list of running games. No
protected game or anti-cheat session was used to validate compatibility.

## Source audit

The audit searched the production Rust crates, dependency declarations, app
manifest/build resources, and HIDAPI 2.6.7 Windows C backend for process memory
access, remote allocation/thread creation, injection, Windows input hooks,
input synthesis, service creation/start, and driver installation calls. It also
reviewed the configuration worker, controller session guard, Windows HID
transport, Shell state check, and settings default. No such dangerous calls
were found in the reviewed production sources. Broad Windows feature imports
are API availability, not proof that a particular API is called.

Two relevant existing mechanisms are explicit:

- `mydockfinder_running` uses Toolhelp process snapshots and executable names
  solely to select tray appearance when MyDockFinder is present. It does not open
  another process, inspect game memory, or drive polling controls.
- The HIDAPI backend uses `CreateFileW`, `HidD_SetFeature`, and HID report
  `DeviceIoControl` requests for device communication. SetupDi and configuration
  manager calls enumerate devices and read properties; their namespace name
  does not imply driver installation. Existing battery providers can also read
  input reports and controller state; they do not synthesize input.

The PowerShell/C# native/UI validation helpers operate the app's test windows
with window messages and capture images. They are isolated validation tools,
not compiled into the release executable. Source inspection is bounded evidence:
it is not a complete audit of every dependency, final executable import table,
firmware, or future release. No hardware polling writes were performed for this
audit. See [Razer protocol evidence](polling-razer-evidence.md),
[Logitech evidence](polling-logitech-evidence.md) and
[MCHOSE evidence](polling-mchose-evidence.md) for per-device
provenance and the distinction between reported rate and measured USB frequency.
