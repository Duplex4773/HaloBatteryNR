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
| DeathAdder V3 Pro `1532:00B6/00B7` | 125, 500, 1000 | Conservative legacy protocol; higher rates unsupported here |
| PRO X Superlight 2 / DEX, supported receiver | Advertised subset of 125–8000 | Exact HID++ unit and model; software control mode required |
| PRO X Superlight 2 / DEX, direct USB | Advertised subset up to 1000 | Effective wired limit from cited hardware evidence |

Logitech onboard profile mode is preserved. Changing that mode or rewriting
profiles could affect other settings, so the app refuses the rate change and
explains that software control mode is required. No DPI, button assignment,
macro, firmware or flash-sector commands are implemented.

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
[Razer evidence](polling-razer-evidence.md) and
[Logitech evidence](polling-logitech-evidence.md) for the exact support boundaries.

## Diagnostic access

The executable offers `--polling-probe --provider razer|logitech --output <file>`
for a read-only diagnostic session. A change additionally requires an exact
`--device-key <stable-key>` and `--polling-hz <rate>`. Diagnostic access acquires
the application's singleton mutex and refuses a concurrent monitoring instance.
Outputs belong in a private data directory; never commit live device keys,
serials, names or captures. Synthetic tests use invented identities.

The original project's verified/unverified labels remain unchanged. Reference
captures support protocol implementation, and synthetic tests exercise behavior;
neither establishes local hardware verification or anti-cheat vendor approval.
