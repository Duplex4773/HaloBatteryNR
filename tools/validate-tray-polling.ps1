# Native simulation only. Uses actual TrackPopupMenu selection, never WM_COMMAND.
# All saved settings and screenshots belong to an isolated validation fixture.
param([string]$Executable = '', [switch]$Startup)
$ErrorActionPreference = 'Stop'
if (Get-Process HaloBatteryNext -ErrorAction SilentlyContinue) {
  throw 'Close the existing app before running this isolated polling test.'
}
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$folder = Join-Path $repo ('validation-local/tray-polling-' + [Guid]::NewGuid().ToString())
[IO.Directory]::CreateDirectory($folder) | Out-Null
$fixture = @{animation=$false;status_file=$true;polling_controls=$true;restore_polling_on_startup=[bool]$Startup}
if ($Startup) { $fixture.devices = @{'simulated:mouse'=@{requested_polling_rate=2000}} }
$fixture | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $folder 'config.json')
# Reuse the same screenshot/process-scoped helpers as validate-tray-menu.ps1.
if (-not ('HaloShot' -as [type])) {
  $source = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'validate-ui.ps1') -Raw
  $helper = [regex]::Match($source, '(?s)Add-Type -TypeDefinition @"\r?\n(.*?)\r?\n"@ -ReferencedAssemblies')
  if (!$helper.Success) { throw 'Native UI helper not found.' }
  Add-Type -TypeDefinition $helper.Groups[1].Value -ReferencedAssemblies System.Drawing.Common,System.Runtime,System.Drawing.Primitives,System.Runtime.InteropServices,System.Private.Windows.GdiPlus,System.Private.Windows.Core,System.Text.Encoding.Extensions
}
if (-not ('HaloPollingProbe' -as [type])) {
  Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class HaloPollingProbe {
 public delegate bool EnumProc(IntPtr window,IntPtr data);
 [StructLayout(LayoutKind.Sequential)] public struct Rect {public int left,top,right,bottom;}
 [StructLayout(LayoutKind.Sequential)] public struct MenuBarInfo {public uint size;public Rect rect;public IntPtr menu,menuWindow;public uint flags;}
 [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc callback,IntPtr data);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window,out uint pid);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr window,StringBuilder value,int length);
 [DllImport("user32.dll")] static extern bool GetMenuBarInfo(IntPtr window,int objectId,int item,ref MenuBarInfo info);
 [DllImport("user32.dll")] public static extern IntPtr GetSubMenu(IntPtr menu,int position);
 [DllImport("user32.dll")] public static extern int GetMenuItemCount(IntPtr menu);
 [DllImport("user32.dll")] public static extern uint GetMenuItemID(IntPtr menu,int position);
 [DllImport("user32.dll")] public static extern uint GetMenuState(IntPtr menu,uint item,uint flags);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetMenuString(IntPtr menu,uint item,StringBuilder text,int length,uint flags);
 public static IntPtr[] Popups(int process) {
  var result=new List<IntPtr>();
  EnumWindows((w,l)=>{uint pid;GetWindowThreadProcessId(w,out pid);if(pid==(uint)process){var b=new StringBuilder(256);GetClassName(w,b,256);if(b.ToString()=="#32768")result.Add(w);}return true;},IntPtr.Zero);
  return result.ToArray();
 }
 public static IntPtr Menu(IntPtr window) {var info=new MenuBarInfo();info.size=(uint)Marshal.SizeOf<MenuBarInfo>();return GetMenuBarInfo(window,-4,0,ref info)?info.menu:IntPtr.Zero;}
 public static string Text(IntPtr menu,int position) {var b=new StringBuilder(512);GetMenuString(menu,(uint)position,b,512,0x400);return b.ToString();}
 public static int Position(IntPtr menu,uint id) {for(int i=0;i<GetMenuItemCount(menu);i++)if(GetMenuItemID(menu,i)==id)return i;return -1;}
 public static bool Enabled(IntPtr menu,uint id) {uint state=GetMenuState(menu,id,0);return state!=0xffffffff&&(state&3)==0;}
 public static bool Checked(IntPtr menu,uint id) {return (GetMenuState(menu,id,0)&8)!=0;}
 public static uint SimulationIcon() {unchecked {ulong b=0x84222325cbf29ce4;foreach(byte x in Encoding.UTF8.GetBytes("simulated:mouse"))b=(b^x)*0x100000001b3;return (uint)b;}}
}
'@
}
[HaloShot]::SetThreadDpiAwarenessContext([IntPtr](-4)) | Out-Null
if (!$Executable) { $Executable = Join-Path $repo 'target/x86_64-pc-windows-msvc/release/HaloBatteryNext.exe' }
$p = Start-Process $Executable -ArgumentList @('--background','--simulate','--data-dir',"`"$folder`"") -PassThru -WindowStyle Hidden
[HaloShot]::TargetPid = $p.Id
$monitor = [IntPtr]::Zero
function Assert-DashboardClosed {
  if ([HaloShot]::FindWindow($null,'Halo Battery Next') -ne [IntPtr]::Zero) { throw 'Tray polling unexpectedly opened the dashboard.' }
}
function Close-Menu {
  [HaloShot]::PostMessage($monitor,31,[UIntPtr]::Zero,[IntPtr]::Zero) | Out-Null
  $deadline = [DateTime]::UtcNow.AddSeconds(3)
  while ([HaloPollingProbe]::Popups($p.Id).Count -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 50 }
  if ([HaloPollingProbe]::Popups($p.Id).Count) { throw 'Native menu did not close.' }
}
function Open-PollingMenu {
  Assert-DashboardClosed
  [HaloShot]::PostMessage($monitor,32769,[UIntPtr]([HaloPollingProbe]::SimulationIcon()),[IntPtr]123) | Out-Null
  $deadline = [DateTime]::UtcNow.AddSeconds(5)
  $root = [IntPtr]::Zero
  do {
    Start-Sleep -Milliseconds 50
    foreach ($window in [HaloPollingProbe]::Popups($p.Id)) {
      $menu = [HaloPollingProbe]::Menu($window)
      if ($menu -ne [IntPtr]::Zero -and [HaloPollingProbe]::Text($menu,4) -eq '&Polling rate') { $root=$menu;break }
    }
  } while ($root -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline)
  if ($root -eq [IntPtr]::Zero) { throw 'Simulation device tray menu or Polling rate submenu missing.' }
  $submenu = [HaloPollingProbe]::GetSubMenu($root,4)
  # WM_CHAR activates the real menu mnemonic inside TrackPopupMenu's loop.
  [HaloShot]::PostMessage($monitor,258,[UIntPtr]112,[IntPtr]1) | Out-Null
  $deadline = [DateTime]::UtcNow.AddSeconds(3)
  do {
    Start-Sleep -Milliseconds 50
    $popup = [IntPtr]::Zero
    foreach ($window in [HaloPollingProbe]::Popups($p.Id)) {
      if ([HaloPollingProbe]::Menu($window) -eq $submenu) { $popup=$window;break }
    }
  } while ($popup -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline)
  if ($popup -eq [IntPtr]::Zero) { throw 'Native polling submenu did not open through its mnemonic.' }
  Assert-DashboardClosed
  return @{window=$popup;menu=$submenu}
}
function Select-MenuAction($popup,[uint32]$id) {
  if (![HaloPollingProbe]::Enabled($popup.menu,$id)) { throw "Menu command $id is unavailable." }
  $position = [HaloPollingProbe]::Position($popup.menu,$id)
  if ($position -lt 0) { throw "Menu command $id is missing." }
  # Navigate the active native popup and observe MF_HILITE before Enter.
  # Disabled information rows can still receive the menu highlight; do not
  # assume navigation skips them or depends on the real mouse cursor.
  $limit = [HaloPollingProbe]::GetMenuItemCount($popup.menu)+2
  for ($i=0;$i -lt $limit;$i++) {
    if (([HaloPollingProbe]::GetMenuState($popup.menu,[uint32]$position,1024) -band 128) -ne 0) { break }
    [HaloShot]::PostMessage($monitor,256,[UIntPtr]40,[IntPtr]1) | Out-Null
    [HaloShot]::PostMessage($monitor,257,[UIntPtr]40,[IntPtr]([long]3221225473)) | Out-Null
    Start-Sleep -Milliseconds 100
  }
  if (([HaloPollingProbe]::GetMenuState($popup.menu,[uint32]$position,1024) -band 128) -eq 0) { throw "Native popup could not highlight command $id." }
  [HaloShot]::PostMessage($monitor,256,[UIntPtr]13,[IntPtr]1) | Out-Null
  [HaloShot]::PostMessage($monitor,257,[UIntPtr]13,[IntPtr]([long]3221225473)) | Out-Null
  $deadline = [DateTime]::UtcNow.AddSeconds(5)
  while ([HaloPollingProbe]::Popups($p.Id).Count -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 50 }
  if ([HaloPollingProbe]::Popups($p.Id).Count) { throw "Native menu selection $id did not return." }
  Assert-DashboardClosed
}
function Wait-Confirmed([int]$hz) {
  $deadline = [DateTime]::UtcNow.AddSeconds(10)
  do {
    Start-Sleep -Milliseconds 200
    $popup = Open-PollingMenu
    $label = [HaloPollingProbe]::Text($popup.menu,0)
    if ($label -eq "Last confirmed: $hz Hz") { return $popup }
    Close-Menu
  } while ([DateTime]::UtcNow -lt $deadline)
  throw "Configured rate readback did not reach $hz Hz; last row: $label"
}
try {
  $deadline = [DateTime]::UtcNow.AddSeconds(15)
  do { Start-Sleep -Milliseconds 100;$monitor=[HaloShot]::FindWindow($null,'Halo Battery Next monitor') } while ($monitor -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline)
  if ($monitor -eq [IntPtr]::Zero) { throw 'Simulation monitor missing.' }
  Start-Sleep -Seconds 3
  if ($Startup) {
    $popup = Wait-Confirmed 2000
    if (![HaloPollingProbe]::Checked($popup.menu,523)) { throw 'Startup did not confirm saved 2000 Hz.' }
    [HaloShot]::Save($popup.window,(Join-Path $folder 'startup-confirmed.png'))
    Select-MenuAction $popup 522
    $popup = Wait-Confirmed 1000
    if (![HaloPollingProbe]::Checked($popup.menu,522)) { throw 'Explicit change after startup failed.' }
    Close-Menu
    Assert-DashboardClosed
    'Saved 2000 Hz restored and confirmed at startup without opening dashboard; later explicit 1000 Hz change succeeded.'
    return
  }
  $popup = Open-PollingMenu
  Close-Menu
  Start-Sleep -Milliseconds 500
  $popup = Open-PollingMenu
  if ([HaloPollingProbe]::Text($popup.menu,0) -ne 'Last confirmed: 1000 Hz') { throw 'Startup read did not populate the tray rate without dashboard interaction.' }
  if ([HaloPollingProbe]::Position($popup.menu,511) -ge 0) { throw 'A read-only startup must not offer Restore previous.' }
  $rates = @(125,500,1000,2000,4000,8000)
  for ($i=0;$i -lt $rates.Count;$i++) {
    $id = [uint32](520+$i)
    $position = [HaloPollingProbe]::Position($popup.menu,$id)
    if ($position -lt 0 -or [HaloPollingProbe]::Text($popup.menu,$position) -ne "$($rates[$i]) Hz" -or ![HaloPollingProbe]::Enabled($popup.menu,$id)) { throw 'Submenu does not immediately offer the candidate simulation rates.' }
    if ([HaloPollingProbe]::Checked($popup.menu,$id) -ne ($rates[$i] -eq 1000)) { throw 'Startup readback must check only the confirmed 1000 Hz rate.' }
  }
  [HaloShot]::Save($popup.window,(Join-Path $folder 'polling-submenu.png'))
  $image = [Drawing.Bitmap]::new((Join-Path $folder 'polling-submenu.png'))
  try { $actual=$image.GetPixel(8,12).ToArgb() -band 0xffffff } finally { $image.Dispose() }
  $light = (Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize' -Name AppsUseLightTheme -ErrorAction SilentlyContinue).AppsUseLightTheme
  if (![HaloShot]::HighContrast()) {
    $expected = if ($light -eq 0) { 0x2d2d2d } else { 0xffffff }
    if ($actual -ne $expected) { throw ('Submenu palette mismatch: actual {0:X6}, expected {1:X6}' -f $actual,$expected) }
  }
  Select-MenuAction $popup 523 # Direct explicit 2000 Hz choice without Refresh.
  $popup = Wait-Confirmed 2000
  if (![HaloPollingProbe]::Checked($popup.menu,523)) { throw 'Reopened menu did not check confirmed 2000 Hz.' }
  $restore = [HaloPollingProbe]::Position($popup.menu,511)
  if ($restore -lt 0 -or [HaloPollingProbe]::Text($popup.menu,$restore) -ne 'Restore &previous (1000 Hz)') { throw 'Restore previous does not target verified 1000 Hz.' }
  [HaloShot]::Save($popup.window,(Join-Path $folder 'polling-confirmed.png'))
  Select-MenuAction $popup 511
  $popup = Wait-Confirmed 1000
  if (![HaloPollingProbe]::Checked($popup.menu,522)) { throw 'Restore previous failed to restore the confirmed-rate checkmark.' }
  [HaloShot]::Save($popup.window,(Join-Path $folder 'polling-restored.png'))
  # Refresh remains optional and uses the same real popup selection path.
  Select-MenuAction $popup 510
  $popup = Wait-Confirmed 1000
  if (![HaloPollingProbe]::Checked($popup.menu,522)) { throw 'Optional Refresh lost the confirmed restored rate.' }
  Close-Menu
  Assert-DashboardClosed
  'Actual native tray selection directly applied and confirmed 2000 Hz without Refresh, restored the observed previous 1000 Hz, then optionally refreshed; dashboard remained closed.'
  'Submenu matched the system app palette (high contrast preserves system styling). All operations used simulation and isolated settings.'
  "Screenshots: $folder"
} finally {
  if (!$p.HasExited) {
    if ($monitor -ne [IntPtr]::Zero) {
      [HaloShot]::PostMessage($monitor,31,[UIntPtr]::Zero,[IntPtr]::Zero) | Out-Null
      [HaloShot]::PostMessage($monitor,32778,[UIntPtr]::Zero,[IntPtr]::Zero) | Out-Null
    }
    if (!$p.WaitForExit(10000)) { $p.Kill();$p.WaitForExit() }
  }
}
