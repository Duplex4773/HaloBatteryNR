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
| 00BF, DeathAdder V4 Pro receiver | extended | 125, 500, 1000, 2000, 4000, 8000 | hardware verified by user testing: all six supported wireless rates, 2026-10-02 |
| 009F, Viper Mini Signature Edition dedicated receiver | extended, 60-ms settle | 125, 500, 1000, 2000, 4000, 8000 | direct upstream protocol and rate list; hardware unverified locally; 8K requires suitable firmware |
| 00C1, Viper V3 Pro dedicated receiver | extended, 60-ms settle | 125, 500, 1000, 2000, 4000, 8000 | direct upstream protocol and rate list, corroborating OpenMouse hardware report; hardware unverified locally |
| 00B6 / 00B7, DeathAdder V3 Pro wired / stock receiver | legacy | 125, 500, 1000 | third-party physical test evidence; hardware unverified by Halo Battery |

Every mouse entry requires USB interface 0 and a 91-byte Windows feature collection.
Missing descriptor information is rejected, as are other collections and PIDs.
That conservative Windows gate can exclude hardware accessible through WebHID.
No Bluetooth, generic dongle, inferred model, or legacy fallback is enabled.

## Expansion audit (1 October 2026)

The catalog's new high-rate routes are restricted to the dedicated **009F** and
**00C1** receivers. This audit read the actual upstream driver and daemon rather
than relying on a generated capability list. Sources are pinned for review:

- [OpenRazer driver at 6820f9da](https://github.com/openrazer/openrazer/blob/6820f9da169d354bc7e6e93a0aa8683a6bb75792/driver/razermouse_driver.c),
  `razer_get_report`, `razer_attr_read_poll_rate`, and `razer_attr_write_poll_rate`:
  interface 0, 59,900-us wait, GET C0 with TID 1F and decode argument 1;
  SET 40 with argument 0 equal to 0 then 1, both TID 1F.
- [OpenRazer PID definitions](https://github.com/openrazer/openrazer/blob/6820f9da169d354bc7e6e93a0aa8683a6bb75792/driver/razermouse_driver.h)
  and [direct daemon rate lists](https://github.com/openrazer/openrazer/blob/6820f9da169d354bc7e6e93a0aa8683a6bb75792/daemon/openrazer_daemon/hardware/mouse.py):
  `RazerViperMiniSEWireless` is 009F and `RazerViperV3ProWireless` is 00C1;
  each lists exactly 125/500/1000/2000/4000/8000 Hz. Their wired counterparts
  009E and 00C0 list only 125/500/1000 Hz and are deliberately excluded.
- [Pinned Windows recipes](https://github.com/Ar4ikov/openrazer-win/blob/6a626b2d11069ec8be6a7eda6e0dd8ee3ece0998/openrazer_win/devices/data/recipes.json)
  corroborate the commands, two-step SET, interface and wait. The generated
  extended getter fallthrough remains excluded as described below.
- [Razer's Mini SE firmware announcement](https://www.razer.com/newsroom/product-news/razer-8000-hz-wireless-polling-rate)
  documents an update enabling 8000-Hz wireless operation. Older firmware may
  refuse or retain a lower requested value; no update or fallback is attempted.
  [Razer's Viper V3 Pro guide](https://dl.razerzone.com/master-guides/RazerSynapse3/VIPERV3PRO-00000192-en.pdf)
  independently lists the same six selectable rates.
- [OpenMouse testing at beef2df9](https://github.com/OpenMouse-Project/mouse-protocol/blob/beef2df996836dbc6a488b8c2e38a67603ec6102/docs/razer-testing.md)
  records Viper V3 Pro wired/receiver controls, and separately reports that Viper
  V3 HyperSpeed stock receiver 00B8 rejects extended polling. Its experimental
  model discovery and fallback policies are not used here.

Rust rounds the documented 59.9-ms settling time up to 60 ms on the two new
receivers. DeathAdder V4 Pro and the existing conservative V3 Pro route retain
31 ms. Every new receiver still uses get-before, two acknowledged writes, and
get-after; any malformed/status/unknown-rate response stops the change without
trying a different command. Both receiver additions have synthetic validation
only in Halo Battery. Readback confirms configuration, not measured frequency.

The catalog also contains pairable HyperPolling dongle **00B3** and Mouse Dock
Pro **00A4**, as well as Viper V2 Pro, DeathAdder V3 Pro/HyperSpeed, Cobra Pro,
Basilisk V3 Pro and 35K variants with high rates advertised through an optional
accessory. They are not upgraded by model name. 00B3 has a documented extended
route (its second SET uses TID FF), but the app does not verify the paired mouse
and its firmware/rate ceiling. 00A4 does not have the same verified polling route.
Stock and wired PIDs do not acquire the accessory's capability. No generic
receiver write, pairing change, firmware update or speculative probe is added.
The non-battery Viper 8K (0091) and DeathAdder V3 (00B2) are outside the current
app catalog. This mouse audit enables no additional mouse PIDs or 250-Hz encoding.

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
encoding is accepted for mice; no default substitution or clamping is accepted.

## Wired keyboard extension

Exact keyboard PIDs `026B` (Huntsman V2 Tenkeyless), `026C` (Huntsman V2),
`0287` (BlackWidow V4), `028D` (BlackWidow V4 Pro) and `02A5` (BlackWidow V4
75%) use USB interface **3** and a **91-byte Windows feature collection**.
They offer 125/250/500/1000/2000/4000/8000 Hz. This separate keyboard allowlist
does not change mouse rate sets, interfaces, waits or write sequences.

The pinned Windows [device identities](https://github.com/Ar4ikov/openrazer-win/blob/6a626b2d11069ec8be6a7eda6e0dd8ee3ece0998/openrazer_win/devices/data/devices.json),
[recipes](https://github.com/Ar4ikov/openrazer-win/blob/6a626b2d11069ec8be6a7eda6e0dd8ee3ece0998/openrazer_win/devices/data/recipes.json)
and [polling builders](https://github.com/Ar4ikov/openrazer-win/blob/6a626b2d11069ec8be6a7eda6e0dd8ee3ece0998/openrazer_win/protocol/chroma.py)
provide the narrow wire evidence: TID `1F`, class `00`, GET `C0`, SET `40`,
arguments `[0, 8000/hz]`, and 1-ms settling. A changed rate sends **exactly one
SET**, then GET readback. The keyboard path must not inherit a mouse receiver's
two-step SET. The getter sends once and decodes extended argument 1, excluding
the generated recipe's legacy fallthrough. Already matching rates send no SET.

The implementation uses independently written protocol builders and synthetic
fixtures, retaining the existing upstream and protocol credits. These keyboards
have no local hardware validation; synthetic readback proves application behavior,
not effective frequency, persistence or anti-cheat approval. Full inventory and
Corsair exclusion evidence is in [keyboard evidence](polling-keyboard-evidence.md).

[Per-PID generated recipes](https://github.com/Ar4ikov/openrazer-win/blob/6a626b2/openrazer_win/devices/data/recipes.json)
identify BE/BF TID 1F, read argument 1, and two write packets with argument 0
equal to 0 then 1, rate in argument 1. Their generated getter subsequently sends
again and decodes argument 0 as legacy: that fallthrough is intentionally not
reproduced. Each Rust getter sends once and decodes its protocol once.

[Windows transport](https://github.com/Ar4ikov/openrazer-win/blob/6a626b2/openrazer_win/core/transport.py)
selects a 91-byte feature collection, prefers the upstream interface, and matches
class, command, and remaining-packet fields. The local transport table gives
BE/BF a 31-ms wait. Rust retains that wait for the existing DeathAdder routes.
The dedicated 009F/00C1 receiver routes instead use 60 ms as documented above.
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

On 2 October 2026, the user confirmed that all supported wireless polling rates
work on the DeathAdder V4 Pro: **125, 500, 1000, 2000, 4000 and 8000 Hz**.
Support is **hardware verified by user testing** for these configuration changes.
This supersedes the partial 1 October report. No identifying captures are retained.
Wired operation, persistence across reconnect/power cycle and independently measured
USB frequency remain unverified. This is not manufacturer or anti-cheat certification.
Readback confirms reported configuration, not physical USB report frequency.
