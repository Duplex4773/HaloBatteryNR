# Reproducing resource comparisons

Run one tray application at a time on Windows. Use Python 3.10 or newer with the
external reference's `requirements.txt` dependencies and Tk installed. The sampler uses only
the Python standard library. Commands below run from the Rust repository root and use `python`
from the chosen environment; `pythonw` can replace it for the launcher.

Validate without launching any tray application or accessing HID hardware:

```powershell
python tools/compare-upstream.py --upstream ../reference-upstream-1.13.0 --validate --status
python tools/compare-upstream.py --upstream ../reference-upstream-1.13.0 --validate --scenario razer --status
python tools/compare-upstream.py --upstream ../reference-upstream-1.13.0 --validate --scenario hardware --status
python tools/sample-process-tree.py --self-test
```

Launch the synthetic upstream scenario in a fresh isolated directory:

```powershell
python tools/compare-upstream.py --upstream ../reference-upstream-1.13.0 --scenario synthetic --status --duration 210 --appdata-root validation-local/python-synthetic
```

Run the launcher in a separate terminal or use `Start-Process -WindowStyle Hidden`
with `-PassThru` to obtain its PID. The launcher manifest is
`validation-local/python-synthetic/launcher.json`. Sample that PID externally:

```powershell
python tools/sample-process-tree.py --pid TARGET_PID --duration 180 --output validation-local/python-synthetic-resources.json
```

Both tools default to 180 seconds; sampling defaults to 200 ms. The launcher
exits gracefully through upstream `App.quit()`. Allow extra launcher duration
when a full 180-second measurement follows a warm-up; use identical warm-up and
measurement windows for both implementations. Sample Rust with the same tool.
Use its ordinary `--simulate --data-dir validation-local/rust-synthetic` route
and set its isolated configuration to the same enabled features before starting.

The launcher reads `halo_battery.pyw` from the explicit external reference. It sets
`APPDATA` to a new child of `validation-local`, imports upstream source,
and calls `App.run()` directly. This bypasses upstream `main()` registry and
legacy startup migration. It does not edit upstream source or user configuration.
All output directories must remain under the ignored `validation-local` folder.

## Workload and limits

The synthetic scenario replaces provider construction and the HID change
signature with one fixed mouse reading: 73%, charging, online. It retains the
real upstream rendering, animation, menu, history and ordinary worker loops.
Animation is enabled by default; upstream uses 30 frames per three seconds.
Provider polling is 60 seconds. Bluetooth, notifications, full-charge alerts,
update checks and full-screen interval changes are disabled. Status output is
off by default; `--status` enables it to match a Rust run with status output on.
`--no-animation` and `--no-fluent-menu` permit corresponding feature comparisons.

This is a controlled synthetic harness, not an all-provider or hardware result.
Rust simulation may still create platform watcher objects while skipping provider
jobs. Record those implementation differences and match visible features,
poll interval, status output, theme, build type and runtime versions. Keep the
mouse away from tray icons and leave menus/flyouts closed during idle runs.

`--scenario razer` instantiates only the existing upstream Razer battery provider.
It retains native device-change discovery. Battery queries use the upstream
protocol; the harness adds no polling-rate commands or device setting changes.
Hardware results depend on the connected device, connection state and charge
state, so record them separately from synthetic measurements.

`--scenario hardware` leaves upstream's provider construction and native change
signature intact, including Bluetooth and controller discovery. Notifications,
full-charge alerts and full-screen suppression retain upstream defaults, matching
the native validator. Update checks remain disabled. This comparison includes
ordinary helper processes and uses real battery queries; no polling rates change.
Use the native validator's `-Hardware -SamplerPython PYTHON_PATH` route for Rust.
For the controlled animated comparison use `-Animation -SamplerPython PYTHON_PATH`.

The sampler sums private committed bytes and kernel plus user CPU for the root
and observed descendants. Identities include PID and process creation time, so
CPU records survive PID reuse. Existing processes use measured CPU deltas;
children created during measurement include their observed lifetime CPU. Children
that start and exit between samples, or CPU consumed after their last observed
sample, can be missed. Memory averages and peaks use successfully queried live
processes at each sample and are not working-set measurements. The sampler's own
CPU is excluded; use it consistently for both applications. Its JSON contains
full samples, identities and limitations, with no profile paths or device names.
