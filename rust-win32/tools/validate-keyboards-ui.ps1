# Native simulation only. Refuses an existing app; never enumerates physical HID.
param([string]$Executable = '')
$ErrorActionPreference = 'Stop'
if (Get-Process HaloBatteryNext -ErrorAction SilentlyContinue) {
  throw 'Close the existing Halo Battery Next instance before running this isolated keyboard test.'
}
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$folder = Join-Path $repo ('validation-local/keyboard-ui-' + [Guid]::NewGuid().ToString())
[IO.Directory]::CreateDirectory($folder) | Out-Null
python (Join-Path $PSScriptRoot 'seed-history-test.py') $folder
if ($LASTEXITCODE -ne 0) { throw 'Synthetic history fixture failed.' }
@{animation=$true;status_file=$true;polling_controls=$true} | ConvertTo-Json |
  Set-Content -LiteralPath (Join-Path $folder 'config.json')

# Reuse only the existing validator's native helper definitions, not its launch,
# settings edits or test body. The source file is read-only.
if (-not ('HaloShot' -as [type])) {
  $helperSource = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'validate-ui.ps1') -Raw
  $helper = [regex]::Match($helperSource, '(?s)Add-Type -TypeDefinition @"\r?\n(.*?)\r?\n"@ -ReferencedAssemblies')
  if (-not $helper.Success) { throw 'Existing native UI helper definition was not found.' }
  Add-Type -TypeDefinition $helper.Groups[1].Value -ReferencedAssemblies System.Drawing.Common,System.Runtime,System.Drawing.Primitives,System.Runtime.InteropServices,System.Private.Windows.GdiPlus,System.Private.Windows.Core,System.Text.Encoding.Extensions
}
if (-not ('HaloKeyboardCombo' -as [type])) {
  Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class HaloKeyboardCombo {
 [DllImport("user32.dll", CharSet=CharSet.Unicode, EntryPoint="SendMessageW")]
 static extern IntPtr ReadItem(IntPtr w,uint m,UIntPtr index,StringBuilder text);
 public static string Item(IntPtr w,int index) {
   var text=new StringBuilder(4096);
   if(ReadItem(w,328,(UIntPtr)index,text).ToInt64()<0) throw new InvalidOperationException("Combo item read failed");
   return text.ToString();
 }
}
'@
}
[HaloShot]::SetThreadDpiAwarenessContext([IntPtr](-4)) | Out-Null
if (-not $Executable) { $Executable = Join-Path $repo 'target/x86_64-pc-windows-msvc/release/HaloBatteryNext.exe' }
if (-not (Test-Path -LiteralPath $Executable)) { throw 'Build the release executable first.' }
$p = Start-Process $Executable -ArgumentList @('--background','--simulate','--simulate-keyboards','--data-dir',"`"$folder`"") -PassThru -WindowStyle Hidden
[HaloShot]::TargetPid = $p.Id

function Wait-Window([string]$Title, [bool]$Present=$true) {
  $deadline = [DateTime]::UtcNow.AddSeconds(10)
  do {
    Start-Sleep -Milliseconds 50
    $window = [HaloShot]::FindWindow($null,$Title)
  } while (($Present -ne ($window -ne [IntPtr]::Zero)) -and [DateTime]::UtcNow -lt $deadline)
  if ($Present -ne ($window -ne [IntPtr]::Zero)) { throw "Window state timeout: $Title present=$Present" }
  return $window
}
function Invoke-CommandId([int]$Id) {
  [HaloShot]::PostMessage($script:dashboard,273,[UIntPtr]$Id,[IntPtr]::Zero) | Out-Null
  Start-Sleep -Milliseconds 150
}
function Get-ComboItems([int]$Id) {
  $combo = [HaloShot]::GetDlgItem($script:dashboard,$Id)
  if ($combo -eq [IntPtr]::Zero) { throw "Missing combo $Id" }
  $count = [HaloShot]::SendMessage($combo,326,[UIntPtr]::Zero,[IntPtr]::Zero).ToInt32()
  for ($i=0; $i -lt $count; $i++) { [HaloKeyboardCombo]::Item($combo,$i) }
}
function Select-ComboText([int]$Id,[string]$Model) {
  $deadline = [DateTime]::UtcNow.AddSeconds(5)
  do {
    $items = @(Get-ComboItems $Id)
    $matches = @($items | Where-Object { $_ -like "*$Model*" })
    if ($matches.Count -eq 1) { break }
    Start-Sleep -Milliseconds 100
  } while ([DateTime]::UtcNow -lt $deadline)
  if ($matches.Count -ne 1) { throw "Expected one '$Model' combo item; saw: $($items -join '; ')" }
  $index = [Array]::IndexOf($items,$matches[0])
  [HaloShot]::SendMessage([HaloShot]::GetDlgItem($script:dashboard,$Id),334,[UIntPtr]$index,[IntPtr]::Zero) | Out-Null
  [HaloShot]::PostMessage($script:dashboard,273,[UIntPtr](65536+$Id),[IntPtr]::Zero) | Out-Null
  Start-Sleep -Milliseconds 200
}
function Wait-Rate([int]$Hz) {
  $deadline = [DateTime]::UtcNow.AddSeconds(5)
  do {
    Start-Sleep -Milliseconds 100
    $text = [HaloShot]::Text([HaloShot]::GetDlgItem($script:dashboard,44))
    $ready = [HaloShot]::IsWindowEnabled([HaloShot]::GetDlgItem($script:dashboard,41))
  } while ((!$ready -or $text -ne "Device-reported configured rate: $Hz Hz") -and [DateTime]::UtcNow -lt $deadline)
  if (!$ready -or $text -ne "Device-reported configured rate: $Hz Hz") { throw "Unverified simulated rate: wanted $Hz, saw $text" }
}
function Assert-KeyboardControls {
  if ([HaloShot]::Text([HaloShot]::GetDlgItem($script:dashboard,91)) -ne 'Wired keyboard · No battery') { throw 'Keyboard detail invented battery state.' }
  foreach ($id in 12,13,14) {
    if ([HaloShot]::GetDlgItem($script:dashboard,$id) -ne [IntPtr]::Zero) { throw "Battery control $id exposed for keyboard." }
  }
}
function Resource-Sample {
  $p.Refresh()
  return @{user=[HaloShot]::GetGuiResources($p.Handle,1);gdi=[HaloShot]::GetGuiResources($p.Handle,0);handles=$p.HandleCount;private=$p.PrivateMemorySize64}
}
function Assert-BatteryStatus {
  $deadline = [DateTime]::UtcNow.AddSeconds(5)
  do {
    Start-Sleep -Milliseconds 100
    $statusPath = Join-Path $folder 'status.json'
    if (Test-Path -LiteralPath $statusPath) { $status = Get-Content -LiteralPath $statusPath -Raw | ConvertFrom-Json }
  } while ($null -eq $status -and [DateTime]::UtcNow -lt $deadline)
  if ($null -eq $status -or @($status.devices).Count -ne 1 -or $status.devices[0].kind -ne 'mouse') {
    throw 'Status must contain exactly the simulated mouse battery row.'
  }
}
try {
  $monitor = Wait-Window 'Halo Battery Next monitor'
  Start-Sleep -Seconds 2
  $cold = Resource-Sample
  [HaloShot]::PostMessage($monitor,32776,[UIntPtr]::Zero,[IntPtr]::Zero) | Out-Null
  $script:dashboard = Wait-Window 'Halo Battery Next'
  Invoke-CommandId 3
  if ([HaloShot]::SendMessage([HaloShot]::GetDlgItem($dashboard,112),240,[UIntPtr]::Zero,[IntPtr]::Zero).ToInt32() -ne 1) { throw 'Isolated polling opt-in was not loaded.' }
  Invoke-CommandId 1
  Select-ComboText 10 'Huntsman V2'
  Assert-KeyboardControls
  Invoke-CommandId 41
  Wait-Rate 1000
  $keyboardBefore = [HaloShot]::Text([HaloShot]::GetDlgItem($dashboard,11))
  [HaloShot]::SendText([HaloShot]::GetDlgItem($dashboard,11),12,[UIntPtr]::Zero,'Renamed test keyboard') | Out-Null
  Invoke-CommandId 15
  Select-ComboText 10 'Renamed test keyboard'
  Assert-KeyboardControls
  $config = Get-Content -LiteralPath (Join-Path $folder 'config.json') -Raw | ConvertFrom-Json
  $renamed = @($config.devices.PSObject.Properties | Where-Object { $_.Value.name -eq 'Renamed test keyboard' })
  if ($renamed.Count -ne 1) { throw 'Keyboard rename was not persisted uniquely.' }
  if ($renamed[0].Value.hidden -or $null -ne $renamed[0].Value.low -or $null -ne $renamed[0].Value.icon) { throw 'Keyboard rename created battery preferences.' }
  Select-ComboText 43 '250 Hz'
  Invoke-CommandId 40
  Wait-Rate 250
  $config = Get-Content -LiteralPath (Join-Path $folder 'config.json') -Raw | ConvertFrom-Json
  if ($config.devices.($renamed[0].Name).requested_polling_rate -ne 250) { throw 'Keyboard Apply 250 intent did not persist.' }
  Invoke-CommandId 41
  Wait-Rate 250
  if (![HaloShot]::IsWindowEnabled([HaloShot]::GetDlgItem($dashboard,42))) { throw 'Keyboard Restore was unavailable after verified Apply.' }
  Invoke-CommandId 42
  Wait-Rate 1000
  [HaloShot]::Save($dashboard,(Join-Path $folder 'razer-keyboard.png'))
  Invoke-CommandId 16
  Select-ComboText 10 $keyboardBefore
  Assert-KeyboardControls

  Select-ComboText 10 'K70 RGB Pro'
  Assert-KeyboardControls
  $reason = 'Polling changes unavailable: this model requires a maintained software session, which is disabled by design.'
  if ([HaloShot]::Text([HaloShot]::GetDlgItem($dashboard,45)) -ne $reason) { throw 'Corsair unavailable reason differs from approved wording.' }
  foreach ($id in 40,41,42,43) {
    $control = [HaloShot]::GetDlgItem($dashboard,$id)
    if ($control -eq [IntPtr]::Zero -or [HaloShot]::IsWindowEnabled($control)) { throw "Corsair control $id must exist and be disabled." }
  }
  [HaloShot]::Save($dashboard,(Join-Path $folder 'corsair-keyboard.png'))
  foreach ($page in 2,6) {
    Invoke-CommandId $page
    $items = @(Get-ComboItems 10)
    if ($items.Count -ne 1 -or $items[0] -notlike '*Simulated mouse*') { throw "Page $page must offer only the battery mouse, saw: $($items -join '; ')" }
    [HaloShot]::Save($dashboard,(Join-Path $folder "battery-page-$page.png"))
  }
  Assert-BatteryStatus
  $samples = @{}
  foreach ($cycle in 1..40) {
    Invoke-CommandId 1
    Select-ComboText 10 'Huntsman V2'
    Assert-KeyboardControls
    Invoke-CommandId 2
    Invoke-CommandId 6
    [HaloShot]::PostMessage($dashboard,274,[UIntPtr]61536,[IntPtr]::Zero) | Out-Null
    $null = Wait-Window 'Halo Battery Next' $false
    if ([HaloShot]::FindWindow($null,'Halo Battery Next monitor') -eq [IntPtr]::Zero) { throw 'Closing dashboard lost monitor.' }
    if ($cycle -in 1,20,40) {
      Start-Sleep -Milliseconds 300
      [HaloShot]::SendMessage($monitor,0,[UIntPtr]::Zero,[IntPtr]::Zero) | Out-Null
      $samples["$cycle"] = Resource-Sample
    }
    if ($cycle -lt 40) {
      [HaloShot]::PostMessage($monitor,32776,[UIntPtr]::Zero,[IntPtr]::Zero) | Out-Null
      $script:dashboard = Wait-Window 'Halo Battery Next'
    }
  }
  $settled = @()
  $elapsed = 0
  foreach ($pause in 5,5,10) {
    Start-Sleep -Seconds $pause
    $elapsed += $pause
    $sample = Resource-Sample
    $sample.closed_seconds = $elapsed
    $settled += $sample
  }
  @{cold=$cold;cycles=$samples;settled=$settled;simulation_only=$true} | ConvertTo-Json -Depth 5 |
    Set-Content -LiteralPath (Join-Path $folder 'resource-cycles.json')
  if ($samples['40'].gdi -gt $samples['1'].gdi -or $samples['40'].user -gt ($samples['1'].user+2) -or $samples['40'].handles -gt ($samples['1'].handles+4)) { throw 'Native handles grew after warm keyboard dashboard baseline.' }
  if ($settled[-1].private -gt 30MB) { throw 'Settled simulation exceeds the 30 MiB private-memory engineering target.' }
  Assert-BatteryStatus
  [HaloShot]::PostMessage($monitor,32778,[UIntPtr]::Zero,[IntPtr]::Zero) | Out-Null
  if (!$p.WaitForExit(30000) -or $p.ExitCode -ne 0) { throw 'Simulated application did not quit cleanly.' }
  # Flush has completed. Verify persistent history never gained keyboard rows.
  $historyCheck = @'
import sqlite3,sys
with sqlite3.connect(sys.argv[1]) as db:
    devices=[r[0] for r in db.execute("SELECT DISTINCT device FROM readings")]
if devices != ["simulated:mouse"]:
    raise SystemExit("History contains a non-battery fixture: " + repr(devices))
print("History retains only the invented mouse battery fixture.")
'@
  python -c $historyCheck (Join-Path $folder 'history.db')
  if ($LASTEXITCODE -ne 0) { throw 'Battery-only persistent history check failed.' }
  Write-Output "Keyboard simulation passed: rename, Read/Apply 250/Restore, Corsair exclusion, battery-only pages/status/history, forty close/reopen cycles. Results: $folder"
  Write-Output ($settled | ConvertTo-Json -Depth 5)
} finally {
  # Only the simulation process started by this script may be terminated.
  $p.Refresh()
  if (!$p.HasExited) { Stop-Process -Id $p.Id }
}
