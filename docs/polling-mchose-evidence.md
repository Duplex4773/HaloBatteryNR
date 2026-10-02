# MCHOSE A7 V2 Ultra+ polling evidence

This implementation is deliberately limited to the 3837:100B receiver, vendor collection usage FF01:0001 with a 65-byte maximum feature report, connected to the captured 3837:4021 A7 V2 Ultra+ model on firmware 5.46.2.4 and captured mode/status byte 09. Other models, firmware, Bluetooth receivers, and direct USB collections remain unavailable. Reference captures establish this dialect; local hardware has not been verified and no physical SET was performed during development.

## Pinned primary evidence

The reference is OpenMouse mouse-protocol commit `beef2df996836dbc6a488b8c2e38a67603ec6102`:

- [Protocol description](https://github.com/OpenMouse-Project/mouse-protocol/blob/beef2df996836dbc6a488b8c2e38a67603ec6102/docs/mchose-protocol.md): reports 11/12, command inversion, paired identity queries, configuration read 67 and write 57, and the wired/wireless rate fields.
- [Protocol definitions](https://github.com/OpenMouse-Project/mouse-protocol/blob/beef2df996836dbc6a488b8c2e38a67603ec6102/src/mchose/index.ts): configuration schema and rate indices 125, 500, 1000, 2000, 4000, 8000 Hz. Configuration offset 2 high nibble is wireless rate; its low nibble is the selected DPI stage. Offset 1 is the wired counterpart.
- [Captured protocol fixtures](https://github.com/OpenMouse-Project/mouse-protocol/blob/beef2df996836dbc6a488b8c2e38a67603ec6102/src/mchose/index.test.ts): command 03 decoded bytes `01 37 38 0b 10 01 00` identify the bonded/connected receiver and host PID 100B. Command 06 decoded bytes `37 38 21 40 05 2e 02 04 09 29 00 2c` identify paired model 4021 and firmware. The trailing byte is outside the identity schema used here.
- [Driver sequencing](https://github.com/OpenMouse-Project/mouse-protocol/blob/beef2df996836dbc6a488b8c2e38a67603ec6102/src/drivers/mchose/hid.ts) and [driver regressions](https://github.com/OpenMouse-Project/mouse-protocol/blob/beef2df996836dbc6a488b8c2e38a67603ec6102/src/drivers/mchose/hid.test.ts): host feature transactions and post-write configuration verification.

These links describe independently implemented protocol facts, not copied source. The captured 09 flag does not agree unambiguously with a generic bit-order interpretation of mode/status; therefore this route requires that exact captured value plus the independent connected-receiver query. It does not infer arbitrary mode bits or additional firmware compatibility.

## Transaction and preservation requirements

Numbered SHORT replies may contain 21 bytes including report ID, or a 65-byte padded buffer. LONG replies must contain exactly 65 bytes. Every accepted reply requires the correct report ID and inverted command echo. Foreign command echoes reset consistency and are ignored within a twelve-query bound. Queries pause 90 ms, and only two consecutive identical payloads establish a result. The command echo has no transaction token: consistency filtering cannot prove freshness against two identical delayed same-command frames; this is a documented protocol limitation.

An Apply first obtains stable receiver identity, paired identity and all 63 configuration bytes, then repeats those checks immediately before SET. Profile, identity or any byte change denies the write. The clone changes only offset 2's high nibble; stage selection, wired rate, DPI, buttons, macros and unknown tail bytes are preserved. Schema validation bounds profiles, rate/stage indices, stage count, DPI and debounce. An unsupported requested rate sends no reports.

SET is a single LONG 57 feature request. No undocumented SET acknowledgement is invented. After a 400 ms settling interval, two fresh queries per identity/configuration check must establish the expected configuration. Every unrelated byte must equal the saved blob. A failure after attempting SET remains uncertain (`may_have_changed`), including a transport error even if readback matches. Changed bindings/profiles/configurations clear previous/current/supported values so stale recovery cannot be offered. No discovery, firmware, profile-switch, macro or button commands are issued.

The caller must retain selected receiver serial/container binding, enumeration epoch guards, serialization and explicit write permission around every transaction. All operations use the existing guarded user-mode HID session. No hooks, process access, injection, drivers, service changes or simulated input are required. Passing the source/import API guard does not certify any anti-cheat decision.

## Validation

`cargo test -p hb-providers --test mchose_controls` exercises synthetic transactions only: short reply lengths, full-byte preservation, unsupported rates and firmware, malformed/foreign replies, unstable reads, binding/profile/configuration changes, uncertain SET, verification mismatch, cancellation and deadlines before and after SET. These tests establish host behavior; they do not establish local hardware compatibility.
