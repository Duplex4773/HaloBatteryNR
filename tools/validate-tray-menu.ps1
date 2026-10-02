# Native simulation only; no physical HID access or system appearance changes.
param([string]$Executable = '')
$ErrorActionPreference = 'Stop'
if (Get-Process HaloBatteryNext -ErrorAction SilentlyContinue) {
  throw 'Close the existing app before running this isolated menu test.'
}
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$folder = Join-Path $repo ('validation-local/tray-menu-' + [Guid]::NewGuid().ToString())
[IO.Directory]::CreateDirectory($folder) | Out-Null
@{animation=$false;status_file=$true} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $folder 'config.json')
if (-not ('HaloShot' -as [type])) {
  $source = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'validate-ui.ps1') -Raw
  $helper = [regex]::Match($source, '(?s)Add-Type -TypeDefinition @"\r?\n(.*?)\r?\n"@ -ReferencedAssemblies')
  if (!$helper.Success) { throw 'Native UI helper not found.' }
  Add-Type -TypeDefinition $helper.Groups[1].Value -ReferencedAssemblies System.Drawing.Common,System.Runtime,System.Drawing.Primitives,System.Runtime.InteropServices,System.Private.Windows.GdiPlus,System.Private.Windows.Core,System.Text.Encoding.Extensions
}
if (-not ('HaloMenuProbe' -as [type])) {
  Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class HaloMenuProbe {
 public delegate bool EnumProc(IntPtr w,IntPtr l);
 [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc callback,IntPtr l);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr w,out uint pid);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr w,StringBuilder b,int n);
 public static IntPtr Popup(int process) {
  IntPtr result=IntPtr.Zero;
  EnumWindows((w,l)=>{uint pid;GetWindowThreadProcessId(w,out pid);
   if(pid==(uint)process){var b=new StringBuilder(256);GetClassName(w,b,256);
    if(b.ToString()=="#32768"){result=w;return false;}}
   return true;},IntPtr.Zero);
  return result;
 }
}
'@
}
[HaloShot]::SetThreadDpiAwarenessContext([IntPtr](-4)) | Out-Null
if (!$Executable) { $Executable = Join-Path $repo 'target/x86_64-pc-windows-msvc/release/HaloBatteryNext.exe' }
$p = Start-Process $Executable -ArgumentList @('--background','--simulate','--data-dir',"`"$folder`"") -PassThru -WindowStyle Hidden
[HaloShot]::TargetPid = $p.Id
try {
  $monitor = [IntPtr]::Zero
  $deadline = [DateTime]::UtcNow.AddSeconds(15)
  do {
    Start-Sleep -Milliseconds 100
    $monitor = [HaloShot]::FindWindow($null,'Halo Battery Next monitor')
  } while ($monitor -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline)
  if ($monitor -eq [IntPtr]::Zero) { throw 'Monitor missing.' }
  Start-Sleep -Seconds 5
  $before = [HaloShot]::GetGuiResources($p.Handle,0)
  for ($cycle = 0; $cycle -lt 8; $cycle++) {
    [HaloShot]::PostMessage($monitor,32769,[UIntPtr]::Zero,[IntPtr]123) | Out-Null
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    do {
      Start-Sleep -Milliseconds 50
      $menu = [HaloMenuProbe]::Popup($p.Id)
    } while ($menu -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline)
    if ($menu -eq [IntPtr]::Zero) { throw 'Native popup did not open.' }
    if ($cycle -eq 0) {
      [HaloShot]::Save($menu,(Join-Path $folder 'menu.png'))
      # Sample away from the native border and the padded text.
      $image = [Drawing.Bitmap]::new((Join-Path $folder 'menu.png'))
      try { $actual = $image.GetPixel(8,12).ToArgb() -band 0xffffff } finally { $image.Dispose() }
      $light = (Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize' -Name AppsUseLightTheme -ErrorAction SilentlyContinue).AppsUseLightTheme
      if (-not [HaloShot]::HighContrast()) {
        $expected = if ($light -eq 0) { 0x2d2d2d } else { 0xffffff }
        if ($actual -ne $expected) { throw ('Popup palette mismatch: actual {0:X6}, expected {1:X6}' -f $actual,$expected) }
      }
    }
    [HaloShot]::PostMessage($monitor,31,[UIntPtr]::Zero,[IntPtr]::Zero) | Out-Null
    Start-Sleep -Milliseconds 150
    if ([HaloMenuProbe]::Popup($p.Id) -ne [IntPtr]::Zero) { throw 'Menu cancellation failed.' }
    if ($cycle -eq 0) { $before = [HaloShot]::GetGuiResources($p.Handle,0) }
  }
  $after = [HaloShot]::GetGuiResources($p.Handle,0)
  if ($after -gt $before+2) { throw "Menu GDI resources grew across warmed cycles ($before -> $after)." }
  "Tray popup matches system app appearance; eight cycles retain GDI resources ($before -> $after)."
  "Screenshot: $folder/menu.png"
} finally {
  if (!$p.HasExited) {
    [HaloShot]::PostMessage($monitor,31,[UIntPtr]::Zero,[IntPtr]::Zero) | Out-Null
    [HaloShot]::PostMessage($monitor,32778,[UIntPtr]::Zero,[IntPtr]::Zero) | Out-Null
    if (!$p.WaitForExit(10000)) { $p.Kill() }
  }
}
