# Changelog

## Unreleased — 4 October 2026 dashboard updates

- Group Settings into alerts, appearance, battery checks, polling rate and
  device brands, with less-used settings under More options. Native scrolling
  keeps navigation visible and Settings Save actions and feedback in a fixed footer.
- Use friendly device, icon and colour labels; show battery check units and
  retain form edits when validation reports an invalid value.
- Distinguish Last confirmed rate from Saved choice, use local check and session
  dates, and label unavailable retained battery levels as last known. Preserve
  supported-device limits, opt-in controls and the single-attempt change policy.
- Simplify Insights estimates and use-between-charges summaries, with technical evidence
  retained in locally saved support reports and plain-language result feedback.
- Preserve device edits during inventory updates and restore keyboard focus after
  rebuilding a page. Batch native child positions so off-screen controls remain
  reachable without accumulating DPI rounding errors.
- Reuse dashboard fonts and release them and loaded Insights on close. The layout adds no dependency,
  UI framework or background graphics; no new performance measurement is claimed.

## Unreleased — 3 October 2026 audit

- Bound HID access recovery and worker-queue waits; native connection events
  now schedule HID providers as well as controller/Bluetooth integrations.
- Bound pending history through write failures and retain unprocessed device
  observations for recovery. Recover deferred readings/estimates on exit and
  preserve the latest checkpoint. Keep sampled chart endpoints and unknown gaps.
- Release obsolete provider/Bluetooth cache entries while preserving sleeping
  devices; forget explicitly unpaired Logitech receiver slots.
- Keep History data visible during resizing, reload once the sizing loop ends,
  and stream chart vertices without a second history buffer.
- Restore the original Windows startup registration after isolated Settings
  validation, including failure cleanup.
- Record findings, measurements and remaining limitations in the
  [performance audit](docs/performance-audit-20261003.md).

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
