# Optional hardware polling-rate controls

Enable **Settings → Enable polling-rate controls**, then open Devices and select
an online supported mouse. **Refresh rate** reads its configuration. Choose a rate
and press **Apply rate** for a read–write–read transaction. Success requires the
required acknowledgments and matching fresh device readback. This is a configured
rate, not an independent measurement of effective USB reporting frequency.

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

## Diagnostic access

The executable offers `--polling-probe --provider razer|logitech|mchose --output <file>`
for a read-only diagnostic session. A change additionally requires an exact
`--device-key <stable-key>` and `--polling-hz <rate>`. Diagnostic access acquires
the application's singleton mutex and refuses a concurrent monitoring instance.
Outputs belong in a private data directory; never commit live device keys,
serials, names or captures. Synthetic tests use invented identities.

The original project's verified/unverified labels remain unchanged. Reference
captures support protocol implementation, and synthetic tests exercise behavior;
neither establishes local hardware verification or anti-cheat vendor approval.
