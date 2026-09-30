$ErrorActionPreference='Stop'
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
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr w);
 [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr w,int i);
 [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr w,uint m,UIntPtr p,IntPtr l);
 public delegate bool EnumProc(IntPtr h,IntPtr p);
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc c,IntPtr p);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h,StringBuilder b,int n);
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
[HaloShot]::SendMessage([HaloShot]::GetDlgItem($dashboard,101),241,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null;[HaloShot]::PostMessage($dashboard,273,[UIntPtr]210,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 300;$config=Get-Content (Join-Path $folder 'config.json') -Raw|ConvertFrom-Json;if($config.full_alert){throw 'Full alert checkbox save failed'}
[HaloShot]::SendMessage([HaloShot]::GetDlgItem($dashboard,101),241,[UIntPtr]1,[IntPtr]::Zero)|Out-Null;[HaloShot]::PostMessage($dashboard,273,[UIntPtr]210,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 300;[HaloShot]::PostMessage($dashboard,262,[UIntPtr]100,[IntPtr]536870912)|Out-Null;Start-Sleep -Milliseconds 150;
$icons=[HaloShot]::GetDlgItem($dashboard,14);if([HaloShot]::SendMessage($icons,326,[UIntPtr]::Zero,[IntPtr]::Zero).ToInt32()-ne8){throw 'Missing automatic/pictogram choices'};if([HaloShot]::SendMessage($icons,327,[UIntPtr]::Zero,[IntPtr]::Zero).ToInt32()-ne0){throw 'Default pictogram must be automatic'}
[HaloShot]::SendText([HaloShot]::GetDlgItem($dashboard,11),12,[UIntPtr]::Zero,'Renamed simulation')|Out-Null;[HaloShot]::SendMessage($icons,334,[UIntPtr]5,[IntPtr]::Zero)|Out-Null;[HaloShot]::PostMessage($dashboard,273,[UIntPtr]15,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 300;$config=Get-Content (Join-Path $folder 'config.json') -Raw|ConvertFrom-Json;$device=$config.devices.'simulated:mouse';if($device.name-ne'Renamed simulation'-or$device.icon-ne'bluetooth'){throw 'Rename/pictogram persistence failed'}
[HaloShot]::SendMessage($icons,334,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null;[HaloShot]::PostMessage($dashboard,273,[UIntPtr]15,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 300;$config=Get-Content (Join-Path $folder 'config.json') -Raw|ConvertFrom-Json;$device=$config.devices.'simulated:mouse';if($null-ne$device.icon-or$device.name-ne'Renamed simulation'){throw 'Automatic icon should retain rename'}
[HaloShot]::SendMessage([HaloShot]::GetDlgItem($dashboard,12),241,[UIntPtr]1,[IntPtr]::Zero)|Out-Null;[HaloShot]::PostMessage($dashboard,273,[UIntPtr]15,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 300;$config=Get-Content (Join-Path $folder 'config.json') -Raw|ConvertFrom-Json;if(!$config.devices.'simulated:mouse'.hidden){throw 'Hide setting save failed'}
[HaloShot]::SendMessage([HaloShot]::GetDlgItem($dashboard,12),241,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null;[HaloShot]::PostMessage($dashboard,273,[UIntPtr]15,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 300;Write-Output 'Full charge toggle,8iconchoices,rename,pictogram override/automatic and hide/unhide persisted.'
[HaloShot]::PostMessage($monitor,32777,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 200
if([HaloShot]::FindWindow($null,'Halo Battery Next')-ne[IntPtr]::Zero){throw 'Dashboard did not close'}
if([HaloShot]::FindWindow($null,'Halo Battery Next monitor')-eq[IntPtr]::Zero){throw 'Monitor lost on dashboard close'}
 $samples=@{};foreach($cycle in 1..40){[HaloShot]::PostMessage($monitor,32776,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 80;$d=[HaloShot]::FindWindow($null,'Halo Battery Next');[HaloShot]::PostMessage($d,273,[UIntPtr]2,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 80;[HaloShot]::PostMessage($d,273,[UIntPtr]3,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 60;[HaloShot]::PostMessage($monitor,32777,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 80;if($cycle -in 1,20,40){$p.Refresh();$samples["$cycle"]=@{user=[HaloShot]::GetGuiResources($p.Handle,1);gdi=[HaloShot]::GetGuiResources($p.Handle,0);private=$p.PrivateMemorySize64}}}
@{cold=$cold;cycles=$samples}|ConvertTo-Json -Depth 5|Set-Content (Join-Path $folder 'resource-cycles.json')
Write-Output ($samples|ConvertTo-Json -Depth 5)
[HaloShot]::PostMessage($monitor,32778,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null
if(!$p.WaitForExit(30000)){throw 'Quit timeout'}
Write-Output "Screenshots saved. Closed dashboard retains monitor; quit exit=$($p.ExitCode)."
}finally{if(!$p.HasExited){Stop-Process -Id $p.Id}}
