# Native window captures use isolated synthetic data, never user settings or hardware.
param([ValidateSet('Current','Dark','Light')][string[]]$Themes = @('Current'))
$ErrorActionPreference = 'Stop'
if (Get-Process HaloBatteryNext -ErrorAction SilentlyContinue) { throw 'Close Halo Battery Next before capturing the isolated simulation.' }
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$folder = Join-Path $repo ('validation-local/docs-capture-' + [Guid]::NewGuid().ToString('N'))
$output = Join-Path $repo 'docs/screenshots'
[IO.Directory]::CreateDirectory($folder) | Out-Null
[IO.Directory]::CreateDirectory($output) | Out-Null
# Share the native test's invented cycles and rate evidence instead of private history.
$fixture = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'validate-insights-ui.ps1') -Raw
$seed = [regex]::Match($fixture, '(?s)\$seed=@''\r?\n(.*?)\r?\n''@').Groups[1].Value
if (!$seed) { throw 'Synthetic fixture source missing.' }
$seedPath = Join-Path $folder 'seed.py'
[IO.File]::WriteAllText($seedPath, $seed)
python $seedPath $folder
if ($LASTEXITCODE -ne 0) { throw 'Synthetic fixture failed.' }
@{ animation=$false; notify=$false; polling_controls=$true; restore_polling_on_startup=$false; interval=3600; status_file=$false } | ConvertTo-Json | Set-Content (Join-Path $folder 'config.json') -Encoding utf8
$helper = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'validate-ui.ps1') -Raw
$definition = [regex]::Match($helper, '(?s)Add-Type -TypeDefinition @"\r?\n(.*?)\r?\n"@ -ReferencedAssemblies').Groups[1].Value
if (!$definition) { throw 'Native capture helper missing.' }
Add-Type -TypeDefinition $definition -ReferencedAssemblies System.Drawing.Common,System.Runtime,System.Drawing.Primitives,System.Runtime.InteropServices,System.Private.Windows.GdiPlus,System.Private.Windows.Core,System.Text.Encoding.Extensions
if ([HaloShot]::HighContrast() -and $Themes -ne 'Current') { throw 'Turn off high contrast before requesting specific themes.' }
[HaloShot]::SetThreadDpiAwarenessContext([IntPtr](-4)) | Out-Null
$themeKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'
$original = Get-ItemProperty -LiteralPath $themeKey -ErrorAction SilentlyContinue
$hadPreference = $original -and $original.PSObject.Properties.Name -contains 'AppsUseLightTheme'
$oldPreference = if ($hadPreference) { $original.AppsUseLightTheme } else { 1 }
$changedPreference = $false
$process = $null
function Message([IntPtr]$window, [uint32]$message, [uint64]$value=0) {
  [HaloShot]::PostMessage($window,$message,[UIntPtr]$value,[IntPtr]::Zero) | Out-Null
}
function Wait-Window([string]$title) {
  $deadline = [DateTime]::UtcNow.AddSeconds(10)
  do { Start-Sleep -Milliseconds 100; $window = [HaloShot]::FindWindow($null,$title) } while ($window -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline)
  if ($window -eq [IntPtr]::Zero) { throw "Missing simulation window: $title" }
  return $window
}
try {
  $exe = Join-Path $repo 'target/x86_64-pc-windows-msvc/release/HaloBatteryNext.exe'
  $process = Start-Process -FilePath $exe -ArgumentList @('--background','--simulate','--simulate-keyboards','--data-dir',"`"$folder`"") -PassThru -WindowStyle Hidden
  [HaloShot]::TargetPid = $process.Id
  $monitor = Wait-Window 'Halo Battery Next monitor'
  Start-Sleep -Milliseconds 600
  Message $monitor 32776
  $dashboard = Wait-Window 'Halo Battery Next'
  foreach ($theme in $Themes) {
    if ($theme -ne 'Current') {
      $value = if ($theme -eq 'Light') { 1 } else { 0 }
      New-ItemProperty -LiteralPath $themeKey -Name AppsUseLightTheme -Value $value -PropertyType DWord -Force | Out-Null
      $changedPreference = $true
      # Notify only this simulation, without broadcasting to other applications.
      [HaloShot]::SendMessage($monitor,26,[UIntPtr]::Zero,[IntPtr]::Zero) | Out-Null
      $expected = if ($value -eq 0) { 0x202020 } else { 0xfafafa }
      $deadline = [DateTime]::UtcNow.AddSeconds(5)
      do { Start-Sleep -Milliseconds 200; $actual = [HaloShot]::Background($dashboard) } while ($actual -ne $expected -and [DateTime]::UtcNow -lt $deadline)
      if ($actual -ne $expected) { throw "Theme capture failed: $theme palette was not applied (registry=$((Get-ItemProperty -LiteralPath $themeKey).AppsUseLightTheme), background=$actual)." }
    }
    $suffix = $theme.ToLowerInvariant()
    if ($theme -eq 'Current') { $suffix = if ($oldPreference -eq 0) { 'dark' } else { 'light' } }
    $pages = if ($suffix -eq 'light') { @(@('devices',1),@('settings',3)) } else { @(@('devices',1),@('history',2),@('settings',3),@('insights',6)) }
    foreach ($page in $pages) {
      Message $dashboard 273 $page[1]
      Start-Sleep -Milliseconds 800
      [HaloShot]::Save($dashboard, (Join-Path $output "$($page[0])-$suffix.png"))
    }
  }
} finally {
  if ($changedPreference) {
    if ($hadPreference) { Set-ItemProperty -LiteralPath $themeKey -Name AppsUseLightTheme -Value $oldPreference }
    else { Remove-ItemProperty -LiteralPath $themeKey -Name AppsUseLightTheme }
  }
  if ($process -and !$process.HasExited) {
    $monitor = [HaloShot]::FindWindow($null,'Halo Battery Next monitor')
    if ($monitor -ne [IntPtr]::Zero) { Message $monitor 32778 }
    if (!$process.WaitForExit(10000)) { Stop-Process -Id $process.Id }
  }
}
Write-Output 'Native synthetic UI screenshots captured; original theme preference restored.'
