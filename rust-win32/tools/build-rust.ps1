param()
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$reference = (Resolve-Path (Join-Path $repo '..')).Path
$previousFlags = $env:CARGO_ENCODED_RUSTFLAGS
$flags = @('-C', 'target-feature=+crt-static',
  "--remap-path-prefix=$reference=/workspace",
  "--remap-path-prefix=$repo=/workspace/rust-win32")
# Remove local build-account paths from panic messages and other embedded strings.
# Resolve these at build time; never record a machine's account or profile in source.
if ($env:USERPROFILE) {
  $flags += "--remap-path-prefix=$env:USERPROFILE=/build/home"
}
if ($env:CARGO_HOME) {
  $flags += "--remap-path-prefix=$env:CARGO_HOME=/build/cargo"
}
$env:CARGO_ENCODED_RUSTFLAGS = $flags -join [char]31
Push-Location $repo
try {
  cargo build --release --locked
  if ($LASTEXITCODE -ne 0) { throw 'Release build failed.' }
} finally {
  Pop-Location
  $env:CARGO_ENCODED_RUSTFLAGS = $previousFlags
}
