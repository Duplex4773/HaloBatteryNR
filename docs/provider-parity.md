# Provider port validation

The Rust port has **partial behavioral parity**, not complete equivalence to the
Python reference. The original providers, tests, protocol credits and MIT license
remain in the repository. Simulated protocol tests do not establish hardware
verification. The project's physical Razer DeathAdder check is separate evidence;
other families have not been tested on physical devices by this work.

## Reproducible evidence

Run `python tools/provider_fixtures.py` in the reference Python environment and
`cargo test -p hb-providers`. The generator executes the upstream fake-device test
suite while capturing pure-parser calls, then adds deterministic packet cases.
The notification text test is isolated from actual fullscreen desktop state.

The generated corpus contains **14,718 parser cases**, **2,264 PA reply-shape
cases**, and **247 catalog rows**. Cases cover all controller status bytes,
Logitech voltage interpolation, valid and truncated packets, mutated packets,
percentage, charging and available display labels. Four Rust tests validate those
fixtures and packet sizes/checksums. This does not establish equivalent poll,
cache, identity, error or diagnostic behavior for a whole upstream test.

There are **25 scripted provider I/O scenarios** at this checkpoint. They assert
exact requests and read operations with a fake transport and clock. They cover
G7 and MCHOSE discovery/error/cache behavior, Razer transaction fallback and
cached collection timing, BlackShark/Barracuda wake/query/cleanup, Logitech token
matching and identity cache, Audeze control input reads and newest-frame selection,
ASUS errors, Pulsar checksum/drain, Lofree start/stop and a shared three-second
deadline, Corsair heartbeat, Nintendo subcommands, PlayStation report modes,
8BitDo passive input, positive transactions for remaining HID families, safe
candidate selection, cancellation and strict command allowlists.

`docs/provider-test-inventory.json` inventories all **459** upstream test IDs.
`docs/coverage_mapping_providers.json` records provider-specific mapped or partial
assertions; core, Windows, storage and UI mappings are maintained separately and
merged by the coverage audit. A parser-linked partial row is not full behavioral
coverage. No claim of equivalent coverage for all 459 tests is supported yet.

## Discovery and protocol behavior

The generated catalog is an illustrative model table. MCHOSE 5253 and 3837 are
explicitly supported vendor families; FF01 collections have priority. A8A5 stays
restricted to G7 mouse PID 2255 and FF01. This G7 mouse is distinct from GameSir
G7 Pro, handled by the separate Windows WGI controller provider.

Razer commands are restricted to known model IDs. Unknown products are never
probed based only on a wireless-looking name. The preferred transaction ID and
fallback IDs are tried with matching response checks; working collection and ID
are cached. PA headset protocols use distinct wake/drain/cleanup sequences.
Audeze uses control input reports. Lofree START, command and END share a deadline.
Logitech reads identity and battery features with software-token checks and
isolates channel failures. Family-specific packets, report sizes, collection
selection and retry bounds are implemented rather than generic feature polling.

## Remaining behavioral work

The following gaps are open at this checkpoint and are being addressed after the
requested source checkpoint. They must not be inferred complete from parser tests:

- PA no-wake/reopen/error/cache edge cases and exact noisy-reply deadline timing.
- Audeze unknown/startup/stuck state and pending retry semantics.
- Multi-receiver identity: family-wide keys in several mouse protocols still
  merge ambiguous devices; trusted serial/container/path grouping is required.
- Safe PlayStation physical identity merging and additional Logitech empty-slot,
  replacement, receiver grouping and sleeping-cache scenarios.
- Remaining family-specific malformed, late, transport-error, fallback and cache
  cases. Additional SteelSeries, Corsair, LAMZU, G-Wolves, AM Infinity and HyperX
  scripted tests are being developed in a separate test file.
- Complete per-upstream-test auditing of diagnostics and stateful assertions.

Strict command allowlists intentionally differ from upstream name-based or broad
unknown-product probing where the project's safety plan requires them. XInput,
WGI and native Bluetooth validation belongs to the Windows crate and separate
coverage mappings. These are simulation and source-audit results except for the
explicitly recorded physical Razer check.
