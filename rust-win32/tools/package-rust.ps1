param([string]$Version = '0.1.0')
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$reference = (Resolve-Path (Join-Path $repo '..')).Path
$exe = Join-Path $repo 'target/x86_64-pc-windows-msvc/release/HaloBatteryNext.exe'
if (!(Test-Path -LiteralPath $exe)) { throw 'Run cargo build --release --locked first.' }
if ((Get-Item -LiteralPath $exe).Length -gt 10MB) { throw 'Executable exceeds the 10 MiB engineering target.' }
$output = Join-Path $repo "dist/HaloBatteryNext-$Version-windows-x64"
[IO.Directory]::CreateDirectory($output) | Out-Null
Copy-Item -LiteralPath $exe -Destination $output
Copy-Item -LiteralPath (Join-Path $reference 'LICENSE') -Destination (Join-Path $output 'LICENSE-HaloBattery.txt')
Copy-Item -LiteralPath (Join-Path $repo 'README-next.md') -Destination (Join-Path $output 'README.md')
Copy-Item -LiteralPath (Join-Path $reference 'docs/protocols.md') -Destination (Join-Path $output 'PROTOCOL-CREDITS.md')
$documentation = Join-Path $output 'docs'
[IO.Directory]::CreateDirectory($documentation) | Out-Null
Copy-Item -LiteralPath (Join-Path $reference 'docs/protocols.md') -Destination $documentation
foreach ($name in @('provider-parity.md', 'coverage-summary.md', 'validation-next.md', 'windows-integration.md')) {
  Copy-Item -LiteralPath (Join-Path $repo "docs/$name") -Destination $documentation
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
      foreach ($file in Get-ChildItem -LiteralPath (Join-Path $folder 'hidapi') -Filter 'LICENSE*' -File) {
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
