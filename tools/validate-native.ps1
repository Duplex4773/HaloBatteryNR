param(
  [int]$Seconds = 180,
  [switch]$Animation,
  [switch]$Hardware,
  [switch]$Keyboards,
  [switch]$PollingControls,
  [int]$Cycles = 20,
  [string]$Executable,
  [string]$Label,
  [string]$SamplerPython
)
$ErrorActionPreference = 'Stop'
if ($Keyboards -and $Hardware) { throw 'Keyboard resource fixtures require simulation.' }
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$exe = if ($Executable) { (Resolve-Path -LiteralPath $Executable).Path } else { Join-Path $repo 'target/x86_64-pc-windows-msvc/release/HaloBatteryNext.exe' }
if (!(Test-Path -LiteralPath $exe)) { throw 'Release build required.' }
$data = Join-Path $repo "validation-local/$([guid]::NewGuid())"
[IO.Directory]::CreateDirectory($data) | Out-Null
@{ animation = [bool]$Animation; status_file = $true; polling_controls = [bool]$PollingControls } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $data 'config.json')
if (!("HaloNativeTest" -as [type])) {
  Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class HaloNativeTest {
 public delegate bool EnumProc(IntPtr w, IntPtr l);
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc p, IntPtr l);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr w, out uint pid);
 [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr w, System.Text.StringBuilder s, int n);
 public static IntPtr Window(int process, string title) {
  IntPtr result=IntPtr.Zero;
  EnumWindows((w,l)=> { uint pid; GetWindowThreadProcessId(w,out pid);
   if(pid==(uint)process){var text=new System.Text.StringBuilder(1024);GetWindowText(w,text,1024);if(text.ToString()==title){result=w;return false;}}
   return true; },IntPtr.Zero);return result;
 }
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr w, uint m, UIntPtr p, IntPtr l);
 [DllImport("user32.dll")] public static extern uint GetGuiResources(IntPtr p, uint kind);
 [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern uint RegisterWindowMessage(string name);
}
'@
}
$arguments = @('--background', '--data-dir', "`"$data`"")
if (!$Hardware) { $arguments += '--simulate' }
if ($Keyboards) { $arguments += '--simulate-keyboards' }
$process = Start-Process -FilePath $exe -ArgumentList $arguments -WindowStyle Hidden -PassThru
try {
  $deadline = [DateTime]::UtcNow.AddSeconds(30)
  $monitor = [IntPtr]::Zero
  do {
    Start-Sleep -Milliseconds 100
    $monitor = [HaloNativeTest]::Window($process.Id, 'Halo Battery Next monitor')
    if ($process.HasExited) { throw "App exited with $($process.ExitCode)." }
  } while ($monitor -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline)
  if ($monitor -eq [IntPtr]::Zero) { throw 'Monitor window missing.' }
  Start-Sleep -Seconds 5
  $process.Refresh()
  $cold = @{ private_bytes=$process.PrivateMemorySize64; gdi=[HaloNativeTest]::GetGuiResources($process.Handle,0); user=[HaloNativeTest]::GetGuiResources($process.Handle,1); threads=$process.Threads.Count }
  # One complete warmup initializes process-wide DirectWrite/IME/COM caches.
  # Retain the cold sample and compare subsequent complete cycles for leaks.
  for ($i=-1; $i -lt $Cycles; $i++) {
    [HaloNativeTest]::PostMessage($monitor,32776,[UIntPtr]::Zero,[IntPtr]::Zero) | Out-Null
    Start-Sleep -Milliseconds 400
    $window = [HaloNativeTest]::Window($process.Id, 'Halo Battery Next')
    if ($window -eq [IntPtr]::Zero) { throw "Dashboard cycle $i failed to open." }
    # History exercises Direct2D allocation; Settings exercises native controls.
    [HaloNativeTest]::PostMessage($window,273,[UIntPtr]2,[IntPtr]::Zero) | Out-Null
    Start-Sleep -Milliseconds 200
    [HaloNativeTest]::PostMessage($window,273,[UIntPtr]3,[IntPtr]::Zero) | Out-Null
    Start-Sleep -Milliseconds 200
    [HaloNativeTest]::PostMessage($monitor,32777,[UIntPtr]::Zero,[IntPtr]::Zero) | Out-Null
    Start-Sleep -Milliseconds 200
    if ($i -eq -1) {
      $process.Refresh()
      $before = @{ private_bytes=$process.PrivateMemorySize64; gdi=[HaloNativeTest]::GetGuiResources($process.Handle,0); user=[HaloNativeTest]::GetGuiResources($process.Handle,1); threads=$process.Threads.Count }
    }
  }
  $taskbar = [HaloNativeTest]::RegisterWindowMessage('TaskbarCreated')
  [HaloNativeTest]::PostMessage($monitor,$taskbar,[UIntPtr]::Zero,[IntPtr]::Zero) | Out-Null
  # A resume notification exercises event-driven refresh without suspending the machine.
  [HaloNativeTest]::PostMessage($monitor,536,[UIntPtr]18,[IntPtr]::Zero) | Out-Null
  Start-Sleep -Seconds 2
  $process.Refresh()
  $after = @{ private_bytes=$process.PrivateMemorySize64; gdi=[HaloNativeTest]::GetGuiResources($process.Handle,0); user=[HaloNativeTest]::GetGuiResources($process.Handle,1); threads=$process.Threads.Count }
  $cpu = $process.TotalProcessorTime.TotalSeconds
  # Reject an interactive/settings-changing run as a background comparison.
  $measurementConfigHash = (Get-FileHash -LiteralPath (Join-Path $data 'config.json') -Algorithm SHA256).Hash
  $clock = [Diagnostics.Stopwatch]::StartNew()
  $samples = @()
  if ($SamplerPython) {
    $treePath = Join-Path $data 'process-tree.json'
    & $SamplerPython (Join-Path $PSScriptRoot 'sample-process-tree.py') --pid $process.Id --duration $Seconds --output $treePath | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Process-tree sampler failed.' }
    if ($process.HasExited) { throw 'Monitoring exited during measurement.' }
    $tree = Get-Content -Raw -LiteralPath $treePath | ConvertFrom-Json
    $samples = @($tree.samples | ForEach-Object private_bytes)
    $duration = $tree.duration_seconds
    $cpuPercent = $tree.tree_cpu_one_core_percent
  } else {
    while ($clock.Elapsed.TotalSeconds -lt $Seconds) {
      Start-Sleep -Seconds ([Math]::Min(5,[Math]::Max(1,$Seconds-[int]$clock.Elapsed.TotalSeconds)))
      if ($process.HasExited) { throw 'Monitoring exited during measurement.' }
      $process.Refresh()
      $samples += $process.PrivateMemorySize64
    }
    $process.Refresh()
    $duration = $clock.Elapsed.TotalSeconds
    $cpuPercent = ($process.TotalProcessorTime.TotalSeconds-$cpu)/$duration*100
  }
  $process.Refresh()
  if ((Get-FileHash -LiteralPath (Join-Path $data 'config.json') -Algorithm SHA256).Hash -ne $measurementConfigHash) {
    throw 'Settings changed during resource sampling; repeat without dashboard interaction.'
  }
  if ([HaloNativeTest]::Window($process.Id, 'Halo Battery Next') -ne [IntPtr]::Zero) {
    throw 'Dashboard was open at the end of resource sampling; repeat with the dashboard closed.'
  }
  $result = @{
    mode = $(if($Hardware){'hardware'}else{'simulated'}); animation = [bool]$Animation; polling_controls = [bool]$PollingControls;
    keyboard_fixtures = [bool]$Keyboards;
    duration_seconds = $duration;
    one_core_cpu_percent = $cpuPercent;
    private_bytes_average = ($samples | Measure-Object -Average).Average;
    private_bytes_peak = ($samples | Measure-Object -Maximum).Maximum;
    executable_bytes = (Get-Item -LiteralPath $exe).Length;
    executable_sha256 = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash;
    label = $Label;
    cold_start = $cold; before_cycles = $before; after_cycles = $after; dashboard_cycles = $Cycles;
    explorer_recovery = 'TaskbarCreated replay'; resume = 'PBT_APMRESUMEAUTOMATIC replay';
    timestamp_utc = [DateTime]::UtcNow.ToString('o'); data_folder = $data
  }
  $runningStatus = Get-Content -Raw -LiteralPath (Join-Path $data 'status.json') | ConvertFrom-Json
  if (!$runningStatus.running -or !$runningStatus.devices.Count) { throw 'No devices recorded while monitoring.' }
  $result.devices_while_running = $runningStatus.devices.Count
  [HaloNativeTest]::PostMessage($monitor,32778,[UIntPtr]::Zero,[IntPtr]::Zero) | Out-Null
  if (!$process.WaitForExit(30000)) { throw 'Graceful shutdown timed out.' }
  $status = Get-Content -Raw -LiteralPath (Join-Path $data 'status.json') | ConvertFrom-Json
  if ($status.running) { throw 'Shutdown status was not flushed.' }
  if ($status.devices.Count) { throw 'Shutdown status did not clear devices.' }
  # The upstream status contract clears devices on graceful exit. Assert a
  # successful reading from the running sample captured before quitting instead.
  $result.shutdown = 'graceful; status.running=false'
  $path = Join-Path $data 'measurement.json'
  $result | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $path
  $result | ConvertTo-Json -Depth 6
  if ($after.gdi -gt $before.gdi+2 -or $after.user -gt $before.user+2) { throw 'Native handle count grew across dashboard cycles.' }
} finally {
  if (!$process.HasExited) {
    [HaloNativeTest]::PostMessage($monitor,32778,[UIntPtr]::Zero,[IntPtr]::Zero) | Out-Null
    if (!$process.WaitForExit(30000)) { $process.Kill() }
  }
}
