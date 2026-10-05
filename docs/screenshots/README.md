# Native dashboard screenshots

The 5 October 2026 consumer UI update is shown using **invented device, battery
and polling data**. Native test windows render both light and dark client areas,
without desktop content or title bars. These images establish no hardware verification.

The Insights images were refreshed on 6 October with separate estimate/total-use
durations and the last saved reading's timestamp. Both themes were inspected.

Devices emphasizes battery status and preferences; History shows a two-hour
synthetic discharge; Settings includes audio alerts and its expanded options;
Insights separates battery-life estimates from recent sessions.

To reproduce from the repository root:

```powershell
$previousCapture = $env:HALO_CAPTURE_DASHBOARD_TEST
try {
  $env:HALO_CAPTURE_DASHBOARD_TEST = '1'
  cargo test -p halo-battery-next --locked native_dashboard_reopens_after_nested_close_and_external_destruction -- --nocapture --test-threads=1
} finally {
  $env:HALO_CAPTURE_DASHBOARD_TEST = $previousCapture
}
```

The test writes BMPs under ignored `validation-local/dashboard`. Convert them to
PNG for documentation. Native control captures use WM_PRINT. History additionally
copies the software-rendered Direct2D plot from the test window into the capture;
it uses the same chart renderer and invented readings. Inspect the result before
publishing, since window occlusion can affect that chart-region capture.

No user settings, physical devices or Windows appearance preferences are changed.
The older `tools/capture-docs.ps1` remains available for isolated release-window
captures; it requires the ordinary app to be closed. Its `-Themes Current` option
leaves Windows appearance preferences untouched.
