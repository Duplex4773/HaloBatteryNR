# Optional hardware polling-rate controls

Enable **Settings → Enable polling-rate controls**, then open Devices and select
an online supported mouse or wired Razer keyboard. **Refresh rate** reads its configuration. Choose a rate
and press **Apply rate** for a read–write–read transaction. Success requires the
required acknowledgments and matching fresh device readback. This is a configured
rate, not an independent measurement of effective USB reporting frequency.

For supported mice, right-click their tray icon and open **Polling rate**, then
select a rate directly. No preliminary Refresh is required. With controls disabled,
the submenu links to Settings to enable the existing opt-in. Opening the menu uses
cached device metadata only; it adds no HID queries, timer or background work.
The selected rate goes through the existing worker's exact-device/capability checks,
GET-before → SET → GET-after transaction and gaming-state restrictions.

Before a hardware read, rates are model-based choices, not confirmed capabilities
of the connected receiver/firmware. Unsupported choices are rejected before a
rate-changing command. After a successful readback the submenu uses the reported
rate list and checks the last confirmed rate. **Refresh hardware rate** remains
available to read without changing anything; **Restore previous** uses the verified
before-value from the last change in that tray session. The dashboard remains closed,
and the tray reports success or failure. Neither opening the menu nor selecting a
saved device preference applies a rate automatically. Keyboard controls remain on
Devices because batteryless keyboards do not have tray icons.

Dashboard and tray writes clear each other's cached confirmation. A pending write
continues to block another configuration request even if its dashboard page closes.
Connection changes invalidate confirmations and previous-rate recovery values.

The setting defaults off. The last requested selection is stored separately from
the observation; startup, reconnect, scheduled battery refresh and closing the
dashboard never apply it. Reconnection invalidates the observed target and the
previous-rate recovery value. **Restore previous** explicitly applies the last
confirmed before-value from this connection session. Partial writes retain an
error even if readback matches; refresh before deciding whether to restore.

| Device / connection | Offered rates (Hz) | Limits |
| --- | --- | --- |
| DeathAdder V4 Pro `1532:00BE/00BF` | 125, 500, 1000, 2000, 4000, 8000 | Dedicated extended getter; two acknowledged setter exchanges |
| Viper V3 Pro dedicated receiver `1532:00C1` | 125, 500, 1000, 2000, 4000, 8000 | Extended protocol; 60-ms settling |
| Viper Mini Signature Edition dedicated receiver `1532:009F` | 125, 500, 1000, 2000, 4000, 8000 | Extended protocol; 8K requires suitable firmware |
| DeathAdder V3 Pro `1532:00B6/00B7` | 125, 500, 1000 | Conservative legacy protocol; higher rates unsupported here |
| PRO X Superlight 2 / DEX / PRO X2 Superstrike, receiver `046D:C54D` | Advertised subset of 125–8000 | Exact HID++ unit/model pair; software control mode required |
| PRO X Superlight 2 / DEX, legacy receiver `046D:C53A` | Advertised subset up to 1000 | Conservative ceiling; no verified high-rate receiver route |
| PRO X Superlight 2 / DEX / Superstrike, direct USB | Advertised subset up to 1000 | Conservative wired limits from cited hardware evidence |
| MCHOSE A7 V2 Ultra+, receiver `3837:100B`, paired model `4021` | 125, 500, 1000, 2000, 4000, 8000 | Only verified protocol schema for firmware 5.46.2.4; configuration preservation checks |
| Huntsman V2 Tenkeyless / V2, `1532:026B/026C` | 125, 250, 500, 1000, 2000, 4000, 8000 | Interface 3, 91-byte feature collection; one acknowledged SET; hardware unverified locally |
| BlackWidow V4 / Pro / 75%, `1532:0287/028D/02A5` | 125, 250, 500, 1000, 2000, 4000, 8000 | Same keyboard transaction; hardware unverified locally |
| Known Corsair high-rate wired keyboards, including K70 RGB Pro | None | Recognition only; maintained software session disabled by design |

Wired keyboards without a battery appear only on Devices. Rename remains
available; battery hide, low-alert threshold and tray-icon controls do not apply.
They create no per-device tray icon, battery history, alerts, Insights estimates
or status battery entry. The app's existing fallback tray icon is retained.
Passive inventory enumeration runs at most every 30 seconds while Devices is
visible, independently of the polling-control setting. It sends no configuration
queries and stops when Devices is not visible. Explicit Razer Read/Apply/Restore
uses the same opt-in and worker guards as mouse controls.

Corsair recognition never opens a command session or sends a rate query, mode
change, SET or heartbeat. Its controls display exactly: **Polling changes
unavailable: this model requires a maintained software session, which is disabled
by design.** See [keyboard identities and evidence](polling-keyboard-evidence.md).

DeathAdder V4 Pro wireless configuration changes have **user-reported working**
evidence from 1 October 2026: **1000 → 8000 → 125 → 2000 Hz**. This report does
not establish wired behavior, 500/4000 Hz, effective USB frequency or anti-cheat
compatibility. See [validation details](validation-next.md).

Logitech onboard profile mode is preserved. Changing that mode or rewriting
profiles could affect other settings, so the app refuses the rate change and
explains that software control mode is required. No DPI, button assignment,
macro, firmware or flash-sector commands are implemented.

MCHOSE polling occupies one nibble in its configuration block. Two identical,
validated reads and a second identity/profile check are required before a write.
The app changes only the wireless rate nibble and preserves every other byte,
including existing DPI, button records and the inactive link. Settled readback
must retain the same profile and all unrelated bytes. It implements no profile
switch, remapping, macro, lighting or firmware-update command. Other models,
firmware, wired/Bluetooth paths and firmware-update collections are refused.

Generic Razer HyperPolling/Dock accessories and shared G-Wolves receivers are
excluded because their USB identity does not establish the paired model's rate
ceiling and protocol. Other battery providers remain unavailable for polling
configuration until exact target, command and readback evidence is reviewed.
No device receives a guessed command or a fallback intended for another vendor.

Known PID, collection and device identity allowlists are independent of the
battery catalog. An upstream product update does not automatically authorize new
configuration commands. Ambiguous collections, swapped paired slots, unknown
units, inaccessible devices and stale connection epochs fail explicitly.

The same two HID workers handle battery and control operations; a vendor gate
serializes each full transaction with every receiver operation for that vendor.
Queues are bounded and rejected control submissions display an error. Disabling
the option, suspension and shutdown revoke queued work; cancellation and the
enumeration epoch are checked around every HID operation. Confirmed rate changes
clear affected remaining-use learning, including proven serial aliases, while
the 30-day battery history remains intact.

Rate changes are blocked when Windows' public notification state reports gaming,
fullscreen, presentation or Windows Store app activity, and when its query fails
or returns an unknown state. Only known non-gaming states permit Apply, checked
before execution and each HID exchange. Apply before starting a game. That signal
does not detect every game and is not anti-cheat certification. See
[the implementation restrictions](anti-cheat.md),
[Razer evidence](polling-razer-evidence.md),
[Logitech evidence](polling-logitech-evidence.md) and
[MCHOSE evidence](polling-mchose-evidence.md) for the exact support boundaries.
Keyboard-specific protocol and recognition limits are in
[keyboard evidence](polling-keyboard-evidence.md).

## Diagnostic access

The executable offers `--polling-probe --provider razer|logitech|mchose --output <file>`
for a read-only diagnostic session. A change additionally requires an exact
`--device-key <stable-key>` and `--polling-hz <rate>`. Diagnostic access acquires
the application's singleton mutex and refuses a concurrent monitoring instance.
Outputs belong in a private data directory; never commit live device keys,
serials, names or captures. Synthetic tests use invented identities.

Add `--device-kind keyboard` with `--provider razer|corsair` for passive keyboard
metadata discovery independent of battery providers. Supplying an exact keyboard
`--device-key` permits the guarded explicit Razer configured-rate GET; adding
`--polling-hz` requests its one-time Apply. Corsair returns its unavailable reason
before any configuration packet. Keyboard metadata discovery alone sends no
configuration GET. The `--simulate-keyboards` fixture flag requires `--simulate`
and adds invented Huntsman V2 and K70 RGB Pro records to the existing mouse.

The original project's verified/unverified labels remain unchanged. Reference
captures support protocol implementation, and synthetic tests exercise behavior;
neither establishes local hardware verification or anti-cheat vendor approval.
