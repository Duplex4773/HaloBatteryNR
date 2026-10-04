# Native dashboard screenshots

Devices, Settings and Insights were rendered on 4 October 2026 by the native
dashboard regression test. They show the current native client area with
**synthetic data**, without desktop content or a title bar. History retains its
2 October Windows release capture with synthetic history. These images demonstrate
the interface; they establish no hardware verification.

To reproduce the native test renders from the repository root:

```powershell
$previousCapture = $env:HALO_CAPTURE_DASHBOARD_TEST
try {
  $env:HALO_CAPTURE_DASHBOARD_TEST = '1'
  cargo test -p halo-battery-next --locked native_dashboard_reopens_after_nested_close_and_external_destruction -- --nocapture --test-threads=1
} finally {
  $env:HALO_CAPTURE_DASHBOARD_TEST = $previousCapture
}
```

The test renders its own synthetic windows to BMPs under ignored
`validation-local/dashboard`. Convert those BMPs to PNGs for documentation.
Its WM_PRINT capture does not include the Direct2D History plot; do not substitute
the resulting blank History image for a release capture.

For release-window captures, including the History plot, use the separate tool
below. It was not run during the 4 October pass because live computer inspection
had been stopped.

From the repository root, close the app and run:

```powershell
.\tools\build-rust.ps1
.\tools\capture-docs.ps1 -Themes Dark,Light
```

The capture tool isolates all data under ignored `validation-local`, sends window
messages only to its simulation process, and captures its window rather than the
desktop. Explicit theme capture temporarily changes the Windows app-theme
preference, notifies only the simulation, and restores the preference on exit.
Use the default `-Themes Current` to leave that preference untouched.
