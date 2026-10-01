# Razer hardware polling controls: evidence and limits

This is an optional device setting, distinct from Halo Battery's battery refresh
interval. Discovery does not write polling settings. The app's HID owner must
validate the current target identity and connection generation before opening
the collection; protocol code receives only that validated session.

The only intended execution path is an explicit user Apply action through
ordinary user-mode Windows HID configuration. There is no game-process access,
injection, input hook, synthetic input, macro, custom driver, or continuous
enforcement. A readback does not establish compatibility with any anti-cheat
product; no such compatibility claim is made.

## Allowlist

| PID (VID 1532) | Protocol | Offered Hz | Evidence status |
| --- | --- | --- | --- |
| 00BE, DeathAdder V4 Pro wired | extended | 125, 500, 1000, 2000, 4000, 8000 | reference supported; wired controls unverified locally |
| 00BF, DeathAdder V4 Pro receiver | extended | 125, 500, 1000, 2000, 4000, 8000 | user-reported wireless changes: 1000 → 8000 → 125 → 2000 Hz, 2026-10-01; 500/4000 unreported |
| 00B6 / 00B7, DeathAdder V3 Pro wired / stock receiver | legacy | 125, 500, 1000 | third-party physical test evidence; hardware unverified by Halo Battery |

Every entry requires USB interface 0 and a 91-byte Windows feature collection.
Missing descriptor information is rejected, as are other collections and PIDs.
That conservative Windows gate can exclude hardware accessible through WebHID.
No Bluetooth, generic dongle, inferred model, or legacy fallback is enabled.

[OpenMouse's hardware test report](https://github.com/OpenMouse-Project/mouse-protocol/blob/main/docs/razer-testing.md)
documents DeathAdder V3 Pro firmware 2.1 using MI_00, actual polling measurement,
125/500/1000 writes, reconnect checks, and a power-cycle check. It explicitly
corrects that model to legacy commands. This is credible evidence for those two
PIDs; a generated capability list alone is insufficient to expand support.

## Wire facts and independent implementation

Credit: OpenRazer contributors, including Terri Cain and Tim Theede, and
Ar4ikov's Windows transport research. The read-only local reference is
`references/openrazer-win`, commit `6a626b2`. Its GPL-2.0-or-later implementation
and generated database/recipes were inspected as protocol evidence. No source
functions, recipe interpreter, generated database, or fixture corpus were
vendored. The Rust state machine and synthetic test reports were independently
written from the following narrow wire facts.

[Report layout](https://github.com/Ar4ikov/openrazer-win/blob/6a626b2/openrazer_win/protocol/report.py):
90 bytes plus Windows report ID 0; TID byte 1; size byte 5; class byte 6;
command byte 7; arguments from byte 8; XOR of bytes 2–87 at byte 88.
[Polling builders](https://github.com/Ar4ikov/openrazer-win/blob/6a626b2/openrazer_win/protocol/chroma.py)
use class 00, extended read C0, write 40, and legacy read 85/write 05.
Extended codes are divisors of 8000, legacy codes divisors of 1000. No 250-Hz
encoding, default substitution, or clamping is accepted.

[Per-PID generated recipes](https://github.com/Ar4ikov/openrazer-win/blob/6a626b2/openrazer_win/devices/data/recipes.json)
identify BE/BF TID 1F, read argument 1, and two write packets with argument 0
equal to 0 then 1, rate in argument 1. Their generated getter subsequently sends
again and decodes argument 0 as legacy: that fallthrough is intentionally not
reproduced. Each Rust getter sends once and decodes its protocol once.

[Windows transport](https://github.com/Ar4ikov/openrazer-win/blob/6a626b2/openrazer_win/core/transport.py)
selects a 91-byte feature collection, prefers the upstream interface, and matches
class, command, and remaining-packet fields. The local transport table gives
BE/BF a 31-ms wait. Rust uses that conservative wait for all enabled models.
It does not impose unverified reply TID or CRC equality: the Windows reference
does not validate those fields. Structural length, report ID, command, remaining
packets, payload bounds, success status, and known rate codes are validated.

## Behavior and verification

Set executes get-before, the protocol's write sequence, then get-after. An
already matching rate produces no SET. BUSY never counts as success. A partial
write or failed acknowledgement remains a structured failure even if a later
read matches. Once a SET may have reached hardware, an unsuccessful verification
reports no observed rate; a healthy verification reports the retained rate.
The initial confirmed rate is retained separately as `previous_hz`, allowing
the owner to offer an explicit restore after a real change.
Cancellation/deadline gates each transport operation and settling sleep. Native
synchronous feature IO itself cannot be interrupted by this protocol layer.

Tests use fake HID sessions and clocks only: exact report fields/XOR, extended
argument-1 decoding and single send, rate rejection, write order, partial BUSY,
readback mismatch, collection allowlist, malformed packets, deadline, and
cancellation. The synthetic tests do not open physical hardware. Earlier automated
read-only hardware results are recorded in `validation-next.md`; those probes
did not attempt a polling SET while the receiver reported a device timeout.

On 1 October 2026, the user reported working wireless polling changes on a
DeathAdder V4 Pro in this sequence: **1000 → 8000 → 125 → 2000 Hz**. This is
user-reported hardware validation of configuration changes, without identifying
device captures. It does not cover 500/4000 Hz, wired operation, reconnection or
power-cycle persistence, independently measured USB frequency, or anti-cheat
compatibility. The original upstream support labels remain unchanged.
Readback confirms reported configuration, not physical USB report frequency.
