# Wired keyboard polling: evidence and boundaries

Batteryless keyboards use a separate dashboard inventory, not a fabricated
battery reading. They appear on Devices with rename and applicable polling
controls, without a per-device tray icon, battery status entry, battery alert,
history row or Insights estimate. Existing application fallback-icon behavior is
retained. Exact VID/PID recognition and container identity deduplicate composite
HID collections. Passive enumeration runs at most every 30 seconds while Devices
is visible, including when polling controls are off, and stops when Devices is
not visible. Enumeration sends no configuration query or write.

Polling controls default off. Explicit Read, Apply and Restore use the existing
guarded user-mode HID worker. Saved choices do not apply on startup, reconnect,
resume, dashboard opening or a timer. A configured-rate readback is not a
measurement of effective USB frequency or an anti-cheat compatibility claim.

## Razer command allowlist

| VID | PID | Model | Offered rates (Hz) |
| --- | --- | --- | --- |
| 1532 | 026B | Huntsman V2 Tenkeyless | 125, 250, 500, 1000, 2000, 4000, 8000 |
| 1532 | 026C | Huntsman V2 | 125, 250, 500, 1000, 2000, 4000, 8000 |
| 1532 | 0287 | BlackWidow V4 | 125, 250, 500, 1000, 2000, 4000, 8000 |
| 1532 | 028D | BlackWidow V4 Pro | 125, 250, 500, 1000, 2000, 4000, 8000 |
| 1532 | 02A5 | BlackWidow V4 75% | 125, 250, 500, 1000, 2000, 4000, 8000 |

All five require interface 3 and a 91-byte Windows feature report collection.
Unknown, missing or ambiguous collection descriptors fail closed. No keyboard
input collection, guessed interface, model-name match or cross-device fallback
authorizes a command. The independently implemented report uses TID `1F`, class
`00`, GET `C0`, SET `40`, arguments `[0, 8000/hz]` and 1-ms settling. A real change
executes get-before, **exactly one acknowledged SET**, and fresh GET readback;
an already matching rate sends no SET. Mouse control sequences remain unchanged.

Primary Windows reference evidence is pinned to openrazer-win
`6a626b2d11069ec8be6a7eda6e0dd8ee3ece0998`:

- [Device names, PIDs and rates](https://github.com/Ar4ikov/openrazer-win/blob/6a626b2d11069ec8be6a7eda6e0dd8ee3ece0998/openrazer_win/devices/data/devices.json).
- [Per-model polling recipes, interface, transaction and delay](https://github.com/Ar4ikov/openrazer-win/blob/6a626b2d11069ec8be6a7eda6e0dd8ee3ece0998/openrazer_win/devices/data/recipes.json).
- [Polling report builders](https://github.com/Ar4ikov/openrazer-win/blob/6a626b2d11069ec8be6a7eda6e0dd8ee3ece0998/openrazer_win/protocol/chroma.py) and
  [Windows feature transport](https://github.com/Ar4ikov/openrazer-win/blob/6a626b2d11069ec8be6a7eda6e0dd8ee3ece0998/openrazer_win/core/transport.py).

The generated getter's subsequent legacy fallthrough is excluded: the Rust getter
sends and decodes the extended protocol once. No mode change, keymap, macro,
lighting, firmware update, input hook, synthetic input or maintained session is
part of this keyboard path. Local hardware behavior remains **unverified** for
all five keyboards. Synthetic protocol and native UI checks do not establish
physical rate, persistence, firmware compatibility or anti-cheat vendor approval.

## Corsair recognition-only allowlist

| VID | PIDs | Recognized family |
| --- | --- | --- |
| 1B1C | 1BB3, 1BD4 | K70 RGB Pro (mechanical / optical) |
| 1B1C | 1B73, 1BB9 | K70 RGB TKL / Champion optical |
| 1B1C | 1B7C, 1B7D, 1BC5 | K100 RGB |
| 1B1C | 1BAF, 1BC3, 1BCF | K65 RGB Mini |
| 1B1C | 1BD7 | K65 Pro Mini |
| 1B1C | 1BC0 | K70 Max |
| 1B1C | 2B14 | K70 Pro TKL |

These seven families and thirteen PIDs are recognized only. Other regional or
newer variants are not inferred. Their controls display exactly:

> Polling changes unavailable: this model requires a maintained software session, which is disabled by design.

No Corsair configuration handle is opened and no polling GET/SET, mode switch,
heartbeat, lighting, keymap or macro command is implemented. Current configured
rate is unavailable; neither cached profile data nor USB descriptor intervals
are substituted for a rate readback.

[Corsair's official 8K support article](https://help.corsair.com/hc/en-us/articles/14640422400525-Using-8K-Polling-on-CORSAIR-Keyboards)
says iCUE must remain open for 8K polling and that closing it or enabling the
tournament switch returns the keyboard to 1K. This conflicts with the requested
one-time-only configuration policy.

Primary source audit revisions:

- OpenLinkHub `d7d3eeafdffc6990d8c89fac451fe01ca891575a`:
  [device registration map](https://github.com/jurkovic-nikola/OpenLinkHub/blob/d7d3eeafdffc6990d8c89fac451fe01ca891575a/src/devices/devices.go#L753)
  provides model/PID recognition. Its
  [K70 Pro implementation](https://github.com/jurkovic-nikola/OpenLinkHub/blob/d7d3eeafdffc6990d8c89fac451fe01ca891575a/src/devices/k70pro/k70pro.go#L112)
  enters software mode, initializes lighting/key assignments and sends polling
  SET through Linux HID interface 1. The rate setter stores a local profile and
  checks transport success, not an independent configured-rate GET afterward.
- ckb-next `833ab50951e230674bda02e8448ef6ef365dfd81`:
  [USB identities](https://github.com/ckb-next/ckb-next/blob/833ab50951e230674bda02e8448ef6ef365dfd81/src/daemon/usb.h#L73),
  [Bragi properties](https://github.com/ckb-next/ckb-next/blob/833ab50951e230674bda02e8448ef6ef365dfd81/src/daemon/bragi_common.c#L4)
  and [mode lifecycle](https://github.com/ckb-next/ckb-next/blob/833ab50951e230674bda02e8448ef6ef365dfd81/src/daemon/device_bragi.c#L14)
  establish the protocol's rate getter but switch hardware to software mode to
  read properties and start a 50-second heartbeat while active. The maximum-rate
  property is [explicitly labeled untested](https://github.com/ckb-next/ckb-next/blob/833ab50951e230674bda02e8448ef6ef365dfd81/src/daemon/bragi_proto.h#L35).
- ckb-next's [interrupt endpoints](https://github.com/ckb-next/ckb-next/blob/833ab50951e230674bda02e8448ef6ef365dfd81/src/daemon/usb_bragi.c#L20)
  and [Linux USB claiming](https://github.com/ckb-next/ckb-next/blob/833ab50951e230674bda02e8448ef6ef365dfd81/src/daemon/usb_linux.c#L302)
  are Linux transport evidence. They do not establish a Windows HID usage-page,
  usage, collection or report route suitable for this application.

Bare software-mode entry plus heartbeat preserving onboard lighting and remaps
is not established by these sources. No live hardware writes or software-mode
entry were performed for this audit. No guessed Corsair Windows packet route is
added.

## Independent implementation and validation

Thanks to OpenRazer contributors, Ar4ikov, OpenLinkHub's Nikola Jurkovic and
ckb-next contributors for protocol research. References were inspected as narrow
wire and identity evidence; their driver/daemon implementations and generated
databases were not vendored. Existing upstream MIT notices and original protocol
credits remain supplied with the portable application.

The native keyboard UI validator launches only `--simulate --simulate-keyboards`
with an isolated ignored data directory. It uses invented mouse, Huntsman V2 and
K70 RGB Pro records, checks rename and absent battery controls, Razer explicit
Read/Apply 250/Restore, Corsair disabled controls and reason, battery-only history,
Insights and status, then measures forty dashboard close/reopen cycles and settled
memory. It refuses to run while an existing application instance is present and
does not open physical HID devices. Run after building and closing the application:

```powershell
./tools/validate-keyboards-ui.ps1
./tools/validate-tray-menu.ps1
```

The separate tray-menu validator checks the actual popup against the system app
appearance and cancels eight popups while checking GDI resource retention.
Dashboard and popup pixel tests cover both palettes and native high contrast.

Results and screenshots belong in `validation-local/`, which is ignored. These
checks exercise simulation behavior only; they must not be recorded as keyboard
hardware verification.
