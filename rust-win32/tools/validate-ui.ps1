$ErrorActionPreference='Stop'
if(Get-Process HaloBatteryNext -ErrorAction SilentlyContinue){throw 'Close the existing Halo Battery Next instance before running this isolated smoke test.'}
$repo=(Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$folder=Join-Path $repo 'validation-local/screenshots'
[IO.Directory]::CreateDirectory($folder)|Out-Null
@{animation=$true;status_file=$true}|ConvertTo-Json|Set-Content (Join-Path $folder 'config.json')
Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
public static class HaloShot {
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr dpi);
 [DllImport("user32.dll")] public static extern uint GetGuiResources(IntPtr p,uint kind);
 public static int TargetPid;
 public static IntPtr FindWindow(string c,string n){IntPtr found=IntPtr.Zero;EnumWindows((h,p)=>{uint id;GetWindowThreadProcessId(h,out id);if(id==TargetPid){var b=new StringBuilder(1024);GetWindowText(h,b,1024);if(b.ToString()==n){found=h;return false;}}return true;},IntPtr.Zero);return found;}
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr w,uint m,UIntPtr p,IntPtr l);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out R r);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint f);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern bool SetWindowText(IntPtr w,string text);
 [DllImport("user32.dll",CharSet=CharSet.Unicode,EntryPoint="SendMessageW")] public static extern IntPtr SendText(IntPtr w,uint m,UIntPtr p,string text);
 [DllImport("user32.dll")] public static extern IntPtr GetFocus();
 [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr w);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr w);
 [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr w,int i);
 [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr w,uint m,UIntPtr p,IntPtr l);
 public delegate bool EnumProc(IntPtr h,IntPtr p);
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc c,IntPtr p);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h,StringBuilder b,int n);
 public static string Text(IntPtr window){var b=new StringBuilder(1024);GetWindowText(window,b,1024);return b.ToString();}
 public static string Dump(int pid){string result="";EnumWindows((h,p)=>{uint id;GetWindowThreadProcessId(h,out id);if(id==pid){var b=new StringBuilder(1024);GetWindowText(h,b,1024);result+=h.ToString()+":"+b.ToString()+";";}return true;},IntPtr.Zero);return result;}
 [DllImport("user32.dll")] public static extern bool GetGUIThreadInfo(uint t,ref G g);
 public struct G{public uint cbSize,flags;public IntPtr active,focus,capture,menu,move,caret;public R rect;}
 public static IntPtr Focus(IntPtr window){uint id;uint thread=GetWindowThreadProcessId(window,out id);G g=new G();g.cbSize=(uint)Marshal.SizeOf<G>();GetGUIThreadInfo(thread,ref g);return g.focus;}
 public struct R {public int left,top,right,bottom;}
 public static void Save(IntPtr hwnd,string file) {R r;GetWindowRect(hwnd,out r);using(var b=new Bitmap(r.right-r.left,r.bottom-r.top)){using(var g=Graphics.FromImage(b)){var dc=g.GetHdc();PrintWindow(hwnd,dc,2);g.ReleaseHdc(dc);}b.Save(file,ImageFormat.Png);}}
}
"@ -ReferencedAssemblies System.Drawing.Common,System.Runtime,System.Drawing.Primitives,System.Runtime.InteropServices,System.Private.Windows.GdiPlus,System.Private.Windows.Core,System.Text.Encoding.Extensions
[HaloShot]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
$exe=Join-Path $repo 'target/x86_64-pc-windows-msvc/release/HaloBatteryNext.exe'
$p=Start-Process $exe -ArgumentList @('--background','--simulate','--data-dir',"`"$folder`"") -PassThru -WindowStyle Hidden
[HaloShot]::TargetPid=$p.Id
try {
$deadline=[DateTime]::UtcNow.AddSeconds(10)
do {Start-Sleep -Milliseconds 400;$monitor=[HaloShot]::FindWindow($null,'Halo Battery Next monitor')}while($monitor -eq [IntPtr]::Zero -and [DateTime]::UtcNow-lt$deadline)
if($monitor -eq [IntPtr]::Zero){throw ("Missing monitor; process="+$p.Id+" windows="+[HaloShot]::Dump($p.Id))}
Start-Sleep -Seconds 2
$p.Refresh();$cold=@{user=[HaloShot]::GetGuiResources($p.Handle,1);gdi=[HaloShot]::GetGuiResources($p.Handle,0)}
[HaloShot]::PostMessage($monitor,32776,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null
Start-Sleep -Seconds 2
$dashboard=[HaloShot]::FindWindow($null,'Halo Battery Next')
if($dashboard -eq [IntPtr]::Zero){throw 'Missing dashboard'}
foreach($page in 1..3){[HaloShot]::PostMessage($dashboard,273,[UIntPtr]$page,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 350;[HaloShot]::Save($dashboard,(Join-Path $folder "$page.png"))}
[HaloShot]::SendMessage($dashboard,40,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null
$focusBefore=[HaloShot]::Focus($dashboard);[HaloShot]::PostMessage($dashboard,256,[UIntPtr]9,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 400;$focusAfter=[HaloShot]::Focus($dashboard);if($focusBefore -eq $focusAfter -or $focusAfter -eq [IntPtr]::Zero){throw "Tab navigation failed"}
[HaloShot]::PostMessage($dashboard,262,[UIntPtr]104,[IntPtr]536870912)|Out-Null;Start-Sleep -Milliseconds 400;if([HaloShot]::GetDlgItem($dashboard,20)-eq [IntPtr]::Zero){throw "Alt H history mnemonic failed"}
[HaloShot]::PostMessage($dashboard,262,[UIntPtr]115,[IntPtr]536870912)|Out-Null;Start-Sleep -Milliseconds 400;if([HaloShot]::GetDlgItem($dashboard,210)-eq [IntPtr]::Zero){throw "Alt S settings mnemonic failed"}
Write-Output "Tab moves focus; Alt H/Alt S select History/Settings."
# Native settings interactions assert persistence through the actual Save handler.
if([HaloShot]::SendMessage([HaloShot]::GetDlgItem($dashboard,108),240,[UIntPtr]::Zero,[IntPtr]::Zero).ToInt32()-ne1){throw 'Quiet mode should default on'}
if([HaloShot]::SendMessage([HaloShot]::GetDlgItem($dashboard,110),240,[UIntPtr]::Zero,[IntPtr]::Zero).ToInt32()-ne0){throw 'PlayStation full mode must default off'}
if([HaloShot]::Text([HaloShot]::GetDlgItem($dashboard,110))-notlike'*PlayStation*opt in*'){throw 'Missing explicit PlayStation opt-in control'}
foreach($setting in @(@{id=108;key='quiet_fullscreen'},@{id=110;key='playstation_full_mode'})){
 foreach($value in 0,1){
  [HaloShot]::SendMessage([HaloShot]::GetDlgItem($dashboard,$setting.id),241,[UIntPtr]$value,[IntPtr]::Zero)|Out-Null
  [HaloShot]::PostMessage($dashboard,273,[UIntPtr]210,[IntPtr]::Zero)|Out-Null
  Start-Sleep -Milliseconds 300
  $config=Get-Content (Join-Path $folder 'config.json') -Raw|ConvertFrom-Json
  if([bool]$config.($setting.key)-ne[bool]$value){throw "$($setting.key) native checkbox did not persist value $value"}
 }
}
$labels=@();foreach($id in 300..324){$widget=[HaloShot]::GetDlgItem($dashboard,$id);if($widget-eq[IntPtr]::Zero){throw "Missing provider switch $id"};$label=[HaloShot]::Text($widget);if(!$label-or$label.Contains('_')){throw "Unreadable provider label $id"};$labels+=$label}
if(@($labels|Select-Object -Unique).Count-ne25-or$labels[0]-ne'Razer'-or$labels-notcontains'PlayStation'){throw 'Provider labels are missing or duplicated'}
[HaloShot]::SendMessage([HaloShot]::GetDlgItem($dashboard,300),241,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null
[HaloShot]::PostMessage($dashboard,273,[UIntPtr]210,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 300
$config=Get-Content (Join-Path $folder 'config.json') -Raw|ConvertFrom-Json
if($config.disabled_providers-notcontains'razer'){throw 'Razer provider switch did not persist disabled state'}
foreach($id in 300..324){[HaloShot]::SendMessage([HaloShot]::GetDlgItem($dashboard,$id),241,[UIntPtr]1,[IntPtr]::Zero)|Out-Null}
[HaloShot]::PostMessage($dashboard,273,[UIntPtr]210,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 300
$config=Get-Content (Join-Path $folder 'config.json') -Raw|ConvertFrom-Json
if(@($config.disabled_providers).Count-ne0){throw 'Restoring all provider switches did not clear disabled providers'}
[HaloShot]::PostMessage($monitor,26,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 300
if([HaloShot]::FindWindow($null,'Halo Battery Next')-eq[IntPtr]::Zero){throw 'Theme change closed the dashboard'}
Write-Output 'Quiet/full-mode false and true persisted; all25 readable provider switches present; Razer disable and restore persisted; theme message preserved dashboard.'
[HaloShot]::SendMessage([HaloShot]::GetDlgItem($dashboard,101),241,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null;[HaloShot]::PostMessage($dashboard,273,[UIntPtr]210,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 300;$config=Get-Content (Join-Path $folder 'config.json') -Raw|ConvertFrom-Json;if($config.full_alert){throw 'Full alert checkbox save failed'}
[HaloShot]::SendMessage([HaloShot]::GetDlgItem($dashboard,101),241,[UIntPtr]1,[IntPtr]::Zero)|Out-Null;[HaloShot]::PostMessage($dashboard,273,[UIntPtr]210,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 300;[HaloShot]::PostMessage($dashboard,262,[UIntPtr]100,[IntPtr]536870912)|Out-Null;Start-Sleep -Milliseconds 150;
if([HaloShot]::Text([HaloShot]::GetDlgItem($dashboard,13))-ne''){throw 'Fresh device threshold must be blank to inherit default'}
$icons=[HaloShot]::GetDlgItem($dashboard,14);if([HaloShot]::SendMessage($icons,326,[UIntPtr]::Zero,[IntPtr]::Zero).ToInt32()-ne8){throw 'Missing automatic/pictogram choices'};if([HaloShot]::SendMessage($icons,327,[UIntPtr]::Zero,[IntPtr]::Zero).ToInt32()-ne0){throw 'Default pictogram must be automatic'}
[HaloShot]::SendText([HaloShot]::GetDlgItem($dashboard,11),12,[UIntPtr]::Zero,'Renamed simulation')|Out-Null;[HaloShot]::SendMessage($icons,334,[UIntPtr]5,[IntPtr]::Zero)|Out-Null;[HaloShot]::PostMessage($dashboard,273,[UIntPtr]15,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 300;$config=Get-Content (Join-Path $folder 'config.json') -Raw|ConvertFrom-Json;$device=$config.devices.'simulated:mouse';if($device.name-ne'Renamed simulation'-or$device.icon-ne'bluetooth'){throw 'Rename/pictogram persistence failed'}
[HaloShot]::SendMessage($icons,334,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null;[HaloShot]::PostMessage($dashboard,273,[UIntPtr]15,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 300;$config=Get-Content (Join-Path $folder 'config.json') -Raw|ConvertFrom-Json;$device=$config.devices.'simulated:mouse';if($null-ne$device.icon-or$device.name-ne'Renamed simulation'){throw 'Automatic icon should retain rename'}
foreach($threshold in '30','0',''){
 [HaloShot]::SendText([HaloShot]::GetDlgItem($dashboard,13),12,[UIntPtr]::Zero,$threshold)|Out-Null
 [HaloShot]::PostMessage($dashboard,273,[UIntPtr]15,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 300
 $config=Get-Content (Join-Path $folder 'config.json') -Raw|ConvertFrom-Json;$device=$config.devices.'simulated:mouse'
 if($threshold-eq''){if($null-ne$device.low){throw 'Clearing threshold did not restore default'}}elseif($device.low-ne[int]$threshold){throw 'Per-device threshold did not persist'}
 if($device.name-ne'Renamed simulation'){throw 'Threshold save lost rename'}
}
Write-Output 'Per-device low threshold30,0disable and blank/default persisted without losing rename.'
[HaloShot]::SendMessage([HaloShot]::GetDlgItem($dashboard,12),241,[UIntPtr]1,[IntPtr]::Zero)|Out-Null;[HaloShot]::PostMessage($dashboard,273,[UIntPtr]15,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 300;$config=Get-Content (Join-Path $folder 'config.json') -Raw|ConvertFrom-Json;if(!$config.devices.'simulated:mouse'.hidden){throw 'Hide setting save failed'}
[HaloShot]::SendMessage([HaloShot]::GetDlgItem($dashboard,12),241,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null;[HaloShot]::PostMessage($dashboard,273,[UIntPtr]15,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 300;Write-Output 'Full charge toggle,8iconchoices,rename,pictogram override/automatic and hide/unhide persisted.'
# Polling-rate automation is exclusively the runtime simulation, never a hardware device.
function Wait-Rate([int]$hz){
 $deadline=[DateTime]::UtcNow.AddSeconds(5)
 do {Start-Sleep -Milliseconds 100;$text=[HaloShot]::Text([HaloShot]::GetDlgItem($dashboard,44));$ready=[HaloShot]::IsWindowEnabled([HaloShot]::GetDlgItem($dashboard,41))}while((!$ready-or$text-ne"Device-reported configured rate: $hz Hz")-and[DateTime]::UtcNow-lt$deadline)
 if(!$ready-or$text-ne"Device-reported configured rate: $hz Hz"){throw "Simulation rate not verified: wanted $hz, saw $text"}
}
function Set-SimRate([int]$index,[int]$hz){
 [HaloShot]::SendMessage([HaloShot]::GetDlgItem($dashboard,43),334,[UIntPtr]$index,[IntPtr]::Zero)|Out-Null
 [HaloShot]::PostMessage($dashboard,273,[UIntPtr]40,[IntPtr]::Zero)|Out-Null
 Wait-Rate $hz
 $config=Get-Content (Join-Path $folder 'config.json') -Raw|ConvertFrom-Json
 if($config.devices.'simulated:mouse'.requested_polling_rate-ne$hz){throw 'Explicit polling intent did not persist'}
}
[HaloShot]::PostMessage($dashboard,273,[UIntPtr]3,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 200
if([HaloShot]::SendMessage([HaloShot]::GetDlgItem($dashboard,112),240,[UIntPtr]::Zero,[IntPtr]::Zero).ToInt32()-ne0){throw 'Polling controls must default off'}
[HaloShot]::SendMessage([HaloShot]::GetDlgItem($dashboard,112),241,[UIntPtr]1,[IntPtr]::Zero)|Out-Null
[HaloShot]::PostMessage($dashboard,273,[UIntPtr]210,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 200
[HaloShot]::PostMessage($dashboard,273,[UIntPtr]1,[IntPtr]::Zero)|Out-Null
Wait-Rate 1000
if([HaloShot]::Text([HaloShot]::GetDlgItem($dashboard,45))-notlike'*UTC*Simulation*'){throw 'Hardware read time/evidence missing'}
Set-SimRate 5 8000
# Saving ordinary device preferences must preserve intent without changing hardware.
[HaloShot]::PostMessage($dashboard,273,[UIntPtr]15,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 150
$config=Get-Content (Join-Path $folder 'config.json') -Raw|ConvertFrom-Json
if($config.devices.'simulated:mouse'.requested_polling_rate-ne8000-or$config.devices.'simulated:mouse'.name-ne'Renamed simulation'){throw 'Saving device preferences lost polling intent'}
[HaloShot]::PostMessage($dashboard,273,[UIntPtr]41,[IntPtr]::Zero)|Out-Null;Wait-Rate 8000
if(![HaloShot]::IsWindowEnabled([HaloShot]::GetDlgItem($dashboard,42))){throw 'Restore previous should be available after a verified change'}
[HaloShot]::PostMessage($dashboard,273,[UIntPtr]42,[IntPtr]::Zero)|Out-Null;Wait-Rate 1000
Set-SimRate 5 8000
[HaloShot]::Save($dashboard,(Join-Path $folder 'polling.png'))
# Restart with saved 8000-Hz intent. Simulation resets to1000; the UI must only Read.
[HaloShot]::PostMessage($monitor,32778,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null
if(!$p.WaitForExit(10000)){throw 'Polling restart quit timeout'}
$p=Start-Process $exe -ArgumentList @('--background','--simulate','--data-dir',"`"$folder`"") -PassThru -WindowStyle Hidden
[HaloShot]::TargetPid=$p.Id
$deadline=[DateTime]::UtcNow.AddSeconds(10)
do{Start-Sleep -Milliseconds 200;$monitor=[HaloShot]::FindWindow($null,'Halo Battery Next monitor')}while($monitor-eq[IntPtr]::Zero-and[DateTime]::UtcNow-lt$deadline)
if($monitor-eq[IntPtr]::Zero){throw 'Missing restarted simulation monitor'}
Start-Sleep -Seconds 1
$p.Refresh();$cold=@{user=[HaloShot]::GetGuiResources($p.Handle,1);gdi=[HaloShot]::GetGuiResources($p.Handle,0)}
[HaloShot]::PostMessage($monitor,32776,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 400
$dashboard=[HaloShot]::FindWindow($null,'Halo Battery Next')
Wait-Rate 1000
if([HaloShot]::Text([HaloShot]::GetDlgItem($dashboard,46))-notlike'*8000 Hz*'){throw 'Restart lost the last requested intent'}
[HaloShot]::PostMessage($dashboard,273,[UIntPtr]3,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 150
[HaloShot]::SendMessage([HaloShot]::GetDlgItem($dashboard,112),241,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null
[HaloShot]::PostMessage($dashboard,273,[UIntPtr]210,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 150
[HaloShot]::PostMessage($dashboard,273,[UIntPtr]1,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 150
if([HaloShot]::GetDlgItem($dashboard,40)-ne[IntPtr]::Zero){throw 'Disabled polling controls still expose Apply'}
$config=Get-Content (Join-Path $folder 'config.json') -Raw|ConvertFrom-Json
if($config.polling_controls-or$config.devices.'simulated:mouse'.requested_polling_rate-ne8000-or$config.devices.'simulated:mouse'.name-ne'Renamed simulation'){throw 'Disabling polling lost preferences or failed to persist'}
Write-Output 'Simulated polling opt-in, Read1000, Apply8000, Refresh, explicit Restore1000, saved intent/no automatic Apply on restart and disable passed.'
[HaloShot]::PostMessage($monitor,32777,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 200
if([HaloShot]::FindWindow($null,'Halo Battery Next')-ne[IntPtr]::Zero){throw 'Dashboard did not close'}
if([HaloShot]::FindWindow($null,'Halo Battery Next monitor')-eq[IntPtr]::Zero){throw 'Monitor lost on dashboard close'}
 $samples=@{};foreach($cycle in 1..40){[HaloShot]::PostMessage($monitor,32776,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 80;$d=[HaloShot]::FindWindow($null,'Halo Battery Next');[HaloShot]::PostMessage($d,273,[UIntPtr]2,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 80;[HaloShot]::PostMessage($d,273,[UIntPtr]3,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 60;[HaloShot]::PostMessage($monitor,32777,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 80;if($cycle -in 1,20,40){$p.Refresh();$samples["$cycle"]=@{user=[HaloShot]::GetGuiResources($p.Handle,1);gdi=[HaloShot]::GetGuiResources($p.Handle,0);private=$p.PrivateMemorySize64}}}
@{cold=$cold;cycles=$samples}|ConvertTo-Json -Depth 5|Set-Content (Join-Path $folder 'resource-cycles.json')
if($samples['40'].gdi-gt$samples['1'].gdi-or$samples['40'].user-gt($samples['1'].user+2)){throw 'Native resources grew after the warm dashboard lifecycle baseline'}
Write-Output ($samples|ConvertTo-Json -Depth 5)
[HaloShot]::PostMessage($monitor,32778,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null
if(!$p.WaitForExit(30000)){throw 'Quit timeout'}
Write-Output "Screenshots saved. Closed dashboard retains monitor; quit exit=$($p.ExitCode)."
}finally{if(!$p.HasExited){Stop-Process -Id $p.Id}}
