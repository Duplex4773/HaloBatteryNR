# Changelog

## 0.1.0 — local release preparation, 2 October 2026

- Standalone Rust/Win32 rewrite based on HaloBattery 1.13.0 (`a566a046`),
  inheriting device protocols, provider behavior, tests and hardware reports.
  The Cargo workspace, crates, documentation and tools now live at repository
  root; the Python application remains an external archived source reference.
- Native dashboard with Devices, History, Settings and Insights; Windows
  light/dark and high-contrast appearance; persistent tray identity across
  supported sleep/wake transitions; configurable orange battery warning band.
- Bounded workers and queues, atomic configuration and status export, and
  batched SQLite history with 30-day retention and awake-use estimates.
- Optional guarded polling controls, tray mouse rate selection, default-off
  startup restoration, and separately scoped batteryless keyboard inventory.
- Supported mouse rates are read once during startup discovery when controls are
  enabled, so tray tooltips populate without dashboard/flyout interaction. Startup
  transactions wait for local worker capacity before being consumed.
- Wireless DeathAdder V4 Pro configured-rate changes user verified at 125,
  500, 1000, 2000, 4000 and 8000 Hz on 2 October 2026. Parent battery reports
  remain separately labelled; no other-model or certification claim is implied.
- Stored parser fixtures, transaction regressions and an auditable 459-ID parent
  test inventory distinguish equivalent behavior, intentional differences and
  retired Python implementation details. See [provider parity](docs/provider-parity.md).
- Local portable build/package scripts include protocol credits, MIT notices,
  third-party licenses and checksums. Release workflows and update checking
  remain disabled; this entry does not announce a published GitHub release.

Historical measured checkpoints and their conditions remain in the
[resource audit](docs/resource-audit.md) and
[2 October performance audit](docs/performance-audit-20261002.md).
