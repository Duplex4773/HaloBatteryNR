# Local portable release

The repository builds a Windows 11 x64 portable application. Release workflows
remain disabled, and update checking is disabled pending an independently
configured release repository. These instructions prepare and review local
artifacts; publishing a GitHub release requires separate authorization.

## Prepare

From the repository root, with Rust/MSVC and the Windows SDK installed:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python tools/merge-coverage.py --check --require-complete
.\tools\build-rust.ps1
.\tools\package-rust.ps1 -Version 0.1.0
```

Keep the version in the Cargo workspace, application documentation and package
argument consistent. Record changes in the [source changelog](https://github.com/Duplex4773/HaloBatteryNR/blob/main/CHANGELOG.md).
The Python coverage check uses the standard library and stored evidence; it does
not require an external Python application checkout. The executable needs no
Python, .NET or webview runtime.

## Inspect the artifact

The package is created in `dist/HaloBatteryNext-<version>-windows-x64/`, with an
adjacent ZIP. Review the portable executable, README, source MIT notice, protocol
credits, third-party notices and license files, documentation and screenshots,
and SHA-256 checksum. The source [application guide](application.md) is copied as
package `README.md`, with documentation links adjusted for the package root.

Use the packaged executable for smoke checks: dashboard reopening, quiet duplicate
background launch, tray actions, light/dark appearance, clean shutdown and history
persistence. For scripted UI checks, use the validation tools with simulated data
and a separate temporary data directory. Inspect the [validation record](validation-next.md)
for the exact measured build and limitations. Re-run relevant resource measurements
when runtime or build configuration changes; do not relabel historical checkpoint
figures as current measurements.

Check that package links and screenshot files resolve after extraction. Screenshots
must identify simulated data and must not contain private identifiers. Confirm the
checksum against the packaged executable and check the archive can be extracted
and launched. The package script checks executable size and embedded local profile
paths; the engineering size target is 10 MiB.

## Evidence and attribution

Retain `LICENSE`, the original [protocol credits](protocols.md), and dependency
license notices. Inherited parent battery reports and Rust hardware verification
remain distinct in [device support](device-support.md). The wireless DeathAdder V4
Pro's six configured-rate checks do not certify other models, wired operation,
effective USB frequency or anti-cheat approval.

Fetch newer official Python source into an external reference checkout and review
protocol/provider/test changes before porting them. Never merge the Python
application into this standalone release tree. See the [reference update workflow](application.md#updating-the-upstream-reference)
and [contribution guide](https://github.com/Duplex4773/HaloBatteryNR/blob/main/CONTRIBUTING.md).
