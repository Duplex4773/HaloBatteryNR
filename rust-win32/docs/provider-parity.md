# Provider port validation

All 25 upstream providers remain represented: 23 HID families in
`hb-providers`, plus Bluetooth and a controller provider using XInput/WGI in the Windows layer. The original Python
providers, protocol credits, tests and MIT license remain available for comparison.

Inherited hardware reports are listed as **User-verified in parent app** in the
[device support table](device-support.md), separately from Rust-port tests and
polling configuration evidence.

## Reproducible evidence

Run `python tools/provider_fixtures.py` in the upstream Python environment,
`cargo test -p hb-providers`, and
`cargo clippy -p hb-providers --all-targets -- -D warnings`.
The fixture generator executes the upstream fake-device suite while recording
pure-parser calls, then adds deterministic cases. One notification test is isolated
from the actual desktop's fullscreen state.

Four Rust tests validate **14,718 parser cases**, **2,264 PA reply-shape cases**,
**247 catalog rows**, and request sizes/checksums. They cover controller status
bytes, Logitech voltage interpolation, valid/truncated/mutated packets, percentages,
charging and available display labels. **141 additional Rust regressions** exercise
protocol transactions, state, identity, diagnostics and deadlines using fake HID
sessions and clocks. Transcripts assert packet bytes, report sizes, matching replies,
fallback channels and retry behavior; parser fixtures alone do not establish poll
or cache equivalence.

`docs/provider-test-inventory.json` inventories all **459** upstream test IDs.
The per-area `coverage_mapping_*.json` files record actual Rust test links,
intentional differences and retired behavior. `tools/merge-coverage.py` checks the
reference inventory and that links point to real Rust tests. The generated coverage
summary is authoritative for the aggregate classification; it covers core, Windows,
UI and storage tests as well as these providers. A retired Python UI/updater test is
not counted as a protocol test passing on physical hardware.

## Protocol and identity behavior

Razer uses known product allowlists, bounded feature-response matching, transaction
fallback and per-device working-path caches. BlackShark PA and Barracuda have
separate wake, drain, query, no-wake/offline, reopen and cleanup behavior. Accepted
offline paths are cached; rejected wake paths do not prevent reaching another
collection. A charging-query failure preserves an already valid Razer percentage.

Audeze reads control input reports, selects the newest battery frame, caches the
short/full sequence decision, detects repeated empty echoes and recovers under the
same device identity. Startup zero is unknown for 90 seconds, with a three-second
scheduled retry; disconnect resets that grace period. Missing vendor collections
produce an unknown reading without writes, and a dongle's no-headset identity
suppresses the transaction. Cable charging remains explicitly inferred.

Mouse families select their documented collections and report capabilities.
G-Wolves cable models take priority over the generic receiver. ASUS and trusted
MCHOSE cable/radio sources prefer charging readings. Pulsar validates checksums,
chooses a fitting output channel and records capability/voltage diagnostics.
WLmouse still checks readable status after a failed feature send. Lofree START,
command and END share one three-second deadline, including malformed-report and
silent-start handling. Corsair handshakes, SteelSeries echo matching, HyperX
fallbacks and AM Infinity request lengths have strict transaction regressions.

Logitech matches software tokens, ignores foreign/late replies, retries incomplete
identity reads and invalidates changed receiver slots. Known hardware unit IDs stay
stable when other receivers arrive or leave; fallback identities always contain the
physical receiver and slot. Sleeping, HID++ 1.0 and inactive-headset states have
visible diagnostics. HID++ 1.0 battery support remains unimplemented, matching the
upstream provider's explicitly unsupported state.

PlayStation stays passive in basic Bluetooth mode unless full reports are enabled;
USB feature reads and report-family offsets are tested separately. Unknown startup
retrying is bounded to 120 seconds. 8BitDo is read-only and treats zero padding as
unknown. Nintendo uses bounded listening/subcommand timing and explicit coarse
labels; a sleeping snapshot clears charging and says it is the last known value.

Keys and candidate/cache groups use trusted serials, containers or physical paths.
Placeholder values (`Unknown`, `NONE`, `N/A`, `NULL`, zero serials/containers) do
not establish shared hardware. Identity values are canonicalized; only native HID
paths with a collection segment have that segment removed. Opaque paths retain
trailing ampersand components. Equal model names or product IDs never establish
that two devices are one. Cable/radio suppression requires shared hardware evidence.
TTL, rejected-path cooldown and startup grace calculations use monotonic elapsed
time; published reading timestamps retain wall-clock time.

## Intentional differences and verification limits

The generated model catalog is illustrative rather than the complete discovery
policy. MCHOSE 5253/3837 are explicitly supported vendor families. A8A5 stays
restricted to G7 mouse PID 2255/FF01; that mouse is distinct from the GameSir G7 Pro
controller handled by WGI. Unknown Razer products are not probed solely because a
product name looks wireless. Coarse measurements are labeled explicitly, and
ambiguous equal-model hardware stays separate rather than inheriting upstream
name/PID-only deduplication. Native collection capability caches periodically
revalidate rather than retaining an indefinite Python cache.

The recorded physical Razer DeathAdder check is separate evidence. Other provider
families and GameSir support were validated through source audits, fixtures and
simulated/platform tests, not physical-device runs. Passing this suite establishes
those assertions and intentional policies; it does not certify every firmware,
USB interface or Bluetooth stack combination.
