param([string]$Version = '0.1.0')
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$exe = Join-Path $repo 'target/x86_64-pc-windows-msvc/release/HaloBatteryNext.exe'
if (!(Test-Path -LiteralPath $exe)) { throw 'Run tools/build-rust.ps1 first.' }
if ((Get-Item -LiteralPath $exe).Length -gt 10MB) { throw 'Executable exceeds the 10 MiB engineering target.' }
if ($env:USERPROFILE) {
  $binaryText = [Text.Encoding]::Latin1.GetString([IO.File]::ReadAllBytes($exe))
  if ($binaryText.Contains($env:USERPROFILE)) {
    throw 'Executable contains a local build profile path. Rebuild with tools/build-rust.ps1.'
  }
}
$output = Join-Path $repo "dist/HaloBatteryNext-$Version-windows-x64"
[IO.Directory]::CreateDirectory($output) | Out-Null
Copy-Item -LiteralPath $exe -Destination $output
Copy-Item -LiteralPath (Join-Path $repo 'LICENSE') -Destination (Join-Path $output 'LICENSE-HaloBattery.txt')
# Source guide lives beside its docs; package README lives one level above them.
$guide = Get-Content -LiteralPath (Join-Path $repo 'docs/application.md') -Raw
$guide = [regex]::Replace($guide, '(?<=\]\()(?!https?://|#)([^)]+\.(?:md|png)(?:#[^)]*)?)(?=\))', 'docs/$1')
$guide | Set-Content -LiteralPath (Join-Path $output 'README.md') -Encoding utf8
Copy-Item -LiteralPath (Join-Path $repo 'CONTRIBUTING.md') -Destination $output
Copy-Item -LiteralPath (Join-Path $repo 'CHANGELOG.md') -Destination $output
$credits = Get-Content -LiteralPath (Join-Path $repo 'docs/protocols.md') -Raw
$credits = [regex]::Replace($credits, '(?<=\]\()(?!https?://|#)([^)]+\.md(?:#[^)]*)?)(?=\))', 'docs/$1')
$credits | Set-Content -LiteralPath (Join-Path $output 'PROTOCOL-CREDITS.md') -Encoding utf8
$documentation = Join-Path $output 'docs'
[IO.Directory]::CreateDirectory($documentation) | Out-Null
Copy-Item -LiteralPath (Join-Path $repo 'docs/protocols.md') -Destination $documentation
foreach ($name in @('application.md', 'releasing.md', 'device-support.md', 'provider-parity.md', 'coverage-summary.md', 'validation-next.md', 'resource-audit.md', 'performance-audit-20261002.md', 'performance-audit-20261003.md', 'ui-audit-20261004.md', 'windows-integration.md', 'polling-controls.md', 'polling-razer-evidence.md', 'polling-keyboard-evidence.md', 'polling-logitech-evidence.md', 'polling-mchose-evidence.md', 'anti-cheat.md', 'history.md', 'insights.md')) {
  Copy-Item -LiteralPath (Join-Path $repo "docs/$name") -Destination $documentation
}
foreach ($name in @('upstream-1.14.0-review.md', 'upstream-1.14.0-test-delta.md', 'ui-audit-20261005.md', 'source-audit-20261006.md')) {
  Copy-Item -LiteralPath (Join-Path $repo "docs/$name") -Destination $documentation
}
$screenshots = Join-Path $repo 'docs/screenshots'
if (Test-Path -LiteralPath $screenshots) {
  Copy-Item -LiteralPath $screenshots -Destination $documentation -Recurse -Force
}
$notices = Join-Path $output 'licenses'
[IO.Directory]::CreateDirectory($notices) | Out-Null
Push-Location $repo
try {
  $metadata = cargo metadata --locked --format-version 1 | ConvertFrom-Json
  if ($LASTEXITCODE -ne 0) { throw 'Cargo metadata failed.' }
  $summary = @('Halo Battery Next third-party notices', '', 'HIDAPI, SQLite and the MSVC CRT are linked into the portable executable.', 'SQLite is in the public domain: https://sqlite.org/copyright.html', 'Protocol credits and the upstream MIT license are supplied alongside this file.', '')
  foreach ($package in $metadata.packages | Where-Object source) {
    $summary += "$($package.name) $($package.version): $($package.license)"
    $folder = Split-Path $package.manifest_path
    $licenseFiles = Get-ChildItem -LiteralPath $folder -File | Where-Object { $_.Name -match '^(LICENSE|LICENCE|COPYING|NOTICE)' }
    foreach ($file in $licenseFiles) {
      Copy-Item -LiteralPath $file.FullName -Destination (Join-Path $notices "$($package.name)-$($package.version)-$($file.Name)")
    }
    # HIDAPI's original C-backend license is nested under the crate.
    if ($package.name -eq 'hidapi') {
      foreach ($file in Get-ChildItem -LiteralPath (Join-Path $folder 'etc/hidapi') -Filter 'LICENSE*' -File) {
        Copy-Item -LiteralPath $file.FullName -Destination (Join-Path $notices "hidapi-C-$($file.Name)")
      }
    }
  }
  $summary | Set-Content -LiteralPath (Join-Path $output 'THIRD-PARTY-NOTICES.txt') -Encoding utf8
  $hash = (Get-FileHash -LiteralPath (Join-Path $output 'HaloBatteryNext.exe') -Algorithm SHA256).Hash
  "$hash  HaloBatteryNext.exe" | Set-Content -LiteralPath (Join-Path $output 'SHA256SUMS.txt')
  Compress-Archive -Path (Join-Path $output '*') -DestinationPath "$output.zip" -Force
  Get-Item -LiteralPath "$output.zip" | Select-Object FullName,Length
} finally { Pop-Location }
