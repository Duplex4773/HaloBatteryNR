# Contributing

Halo Battery Next is a standalone Rust/Win32 application for Windows 11 x64.
The Cargo workspace is at the repository root. The original HaloBattery Python
application is an external protocol and behavior reference; it is not needed to
build or run this application.

## Build and validate

Install Rust with the MSVC x64 target and Visual Studio C++ build tools with the
Windows SDK. Run these commands from the repository root:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python tools/merge-coverage.py --check --require-complete
.\tools\build-rust.ps1
.\tools\package-rust.ps1
```

Only the coverage command requires Python, using its standard library. Rust tests
consume stored fixtures and do not import the parent application. The build script
sets portable linking and removes local build paths from embedded messages. Use
[the release guide](docs/releasing.md) for package contents and local validation.

## Change the implementation

Keep device state, identity and alerts in `crates/core`, protocols and injected HID
contracts in `crates/providers`, Windows integrations in `crates/windows`, storage
in `crates/storage`, and engine/UI orchestration in `crates/app`.

For protocol changes, add meaningful fake-session regressions for packet bytes,
reply matching, capability/identity checks, timeouts, cancellation and recovery.
Parser agreement alone does not prove transaction equivalence. Keep work queues,
retries and caches bounded. Communication failures must retain explicitly stale
readings rather than pretend a device disconnected.

Polling-rate changes require reviewed exact device/connection identities, command
and acknowledgement evidence, and fresh readback. Battery catalog updates do not
authorize configuration writes. Preserve the opt-in, gaming-state restrictions,
connection epochs and existing settings outside the requested rate.

Use invented identities in fixtures and UI validation. Never commit real device
keys, serials, Bluetooth addresses, names, account paths or raw private captures.
Use a separate temporary data directory for simulated validation; preserve the
user's application settings and history.

## Update the official source reference

Fetch [HeyOkay/HaloBattery](https://github.com/HeyOkay/HaloBattery) into a separate
reference checkout. Preserve the archived 1.13.0 source used for existing evidence.
Review changes to protocols, provider behavior, discovery and tests before porting
them. Do not merge the parent Python application into this repository.

With that external checkout's Python dependencies installed:

```powershell
python tools/port_catalog.py --upstream C:\reference\HaloBattery
python tools/provider_fixtures.py --upstream C:\reference\HaloBattery
cargo fmt --all
python tools/merge-coverage.py --upstream C:\reference\HaloBattery
python tools/merge-coverage.py --check --require-complete
cargo test --workspace --locked
```

The 1.14.0 catalog and parser delta can be regenerated with `tools/port_catalog.py`
and `tools/upstream114_fixtures.py` using that exact external version. Keep the
459-ID mapping check against the archived **1.13.0** reference. A changed test-ID
set produces a separate candidate inventory rather than replacing the frozen
mapping. See the [1.14.0 review](docs/upstream-1.14.0-review.md) and
[test delta](docs/upstream-1.14.0-test-delta.md).

Review generated diffs, update coverage mappings and implement transaction changes
before claiming support. The default coverage checker validates the stored
inventory and actual Rust test links. Optional `--upstream` additionally checks the
external reference's test inventory through AST inspection.

## Report evidence accurately

Keep **User-verified in parent app** battery reports scoped to their exact models
and connections. The Rust rewrite inherits parent protocols, provider behavior,
tests and hardware reports, with separate native implementation and verification.
Synthetic tests and inherited reports do not certify every model or transport.
The wireless DeathAdder V4 Pro has Rust-port hardware evidence and user-verified
configured-rate changes at 125, 500, 1000, 2000, 4000 and 8000 Hz (2 October 2026).
Other local hardware claims require recorded evidence; this is not manufacturer
or anti-cheat certification.

Retain the upstream MIT notices and [protocol credits](docs/protocols.md). Preserve
historical resource measurements, test checkpoints and their limitations. Label
simulated screenshots explicitly. Review [provider parity](docs/provider-parity.md)
and [device support](docs/device-support.md) when changing support claims.
