# Run after building release: pwsh -NoProfile -File tools/validate-insights-ui.ps1
# Only this script's --simulate process receives messages. Existing apps are never closed.
$ErrorActionPreference='Stop'
if(Get-Process HaloBatteryNext -ErrorAction SilentlyContinue){throw 'Close the existing app before this isolated simulation test.'}
$repo=(Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$output=Join-Path $repo 'validation-local'
$folder=Join-Path $output ('insights-run-'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($folder)|Out-Null
$seed=@'
import json, sqlite3, sys, time
from pathlib import Path
folder=Path(sys.argv[1]).resolve()
assert 'validation-local' in [p.name for p in (folder,*folder.parents)]
start=int(time.time())-11*3600
with sqlite3.connect(folder/'history.db') as db:
    db.execute('CREATE TABLE readings(device TEXT NOT NULL, ts INTEGER NOT NULL, level INTEGER, payload TEXT NOT NULL, PRIMARY KEY(device,ts))')
    db.execute('CREATE TABLE usage_metadata(device TEXT NOT NULL, ts INTEGER NOT NULL, polling_rate INTEGER, session TEXT, PRIMARY KEY(device,ts))')
    for cycle in range(10):
        hz=1000 if cycle%2==0 else 8000
        for minute in range(61):
            ts=start+cycle*3660+minute*60
            charging=cycle>0 and minute==0
            level=95 if charging else 90-minute//5
            r=dict(key='simulated:mouse',name='Simulated mouse',source='simulation',timestamp=ts,level=level,connection='online',charging=charging,charging_inferred=False,precision='exact',approx=None,kind='mouse',via='',serial=None,container=None)
            db.execute('INSERT INTO readings VALUES(?,?,?,?)',(r['key'],ts,level,json.dumps(r)))
            db.execute('INSERT INTO usage_metadata VALUES(?,?,?,?)',(r['key'],ts,hz,str(cycle+1)))
print('Seeded invented two-rate discharge data, observed charges and a partial cycle.')
'@
$seedPath=Join-Path $folder 'seed.py'
[IO.File]::WriteAllText($seedPath,$seed)
python $seedPath $folder
if($LASTEXITCODE-ne0){throw 'Synthetic fixture failed.'}
@{animation=$false;notify=$false;polling_controls=$false;interval=3600}|ConvertTo-Json|Set-Content (Join-Path $folder 'config.json')
Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
public static class InsightsShot {
 public static int TargetPid;
 public delegate bool EnumProc(IntPtr h,IntPtr p);
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc c,IntPtr p);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h,StringBuilder b,int n);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr w,uint m,UIntPtr p,IntPtr l);
 [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr w,uint m,UIntPtr p,IntPtr l);
 [DllImport("user32.dll",CharSet=CharSet.Unicode,EntryPoint="SendMessageW")] public static extern IntPtr SendText(IntPtr w,uint m,UIntPtr p,StringBuilder text);
 [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr w,int i);
 [DllImport("user32.dll")] public static extern uint GetGuiResources(IntPtr p,uint kind);
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr dpi);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out R r);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint f);
 public struct R {public int left,top,right,bottom;}
 public static string Text(IntPtr window){uint id;GetWindowThreadProcessId(window,out id);if(id!=TargetPid)return "";var b=new StringBuilder(4096);SendText(window,13,(UIntPtr)4096,b);return b.ToString();}
 public static IntPtr Find(string title){IntPtr found=IntPtr.Zero;EnumWindows((h,p)=>{uint id;GetWindowThreadProcessId(h,out id);if(id==TargetPid&&Text(h)==title){found=h;return false;}return true;},IntPtr.Zero);return found;}
 public static void Save(IntPtr hwnd,string file){R r;GetWindowRect(hwnd,out r);using(var b=new Bitmap(r.right-r.left,r.bottom-r.top)){using(var g=Graphics.FromImage(b)){var dc=g.GetHdc();PrintWindow(hwnd,dc,2);g.ReleaseHdc(dc);}b.Save(file,ImageFormat.Png);}}
}
"@ -ReferencedAssemblies System.Drawing.Common,System.Runtime,System.Drawing.Primitives,System.Runtime.InteropServices,System.Private.Windows.GdiPlus,System.Private.Windows.Core,System.Text.Encoding.Extensions
[InsightsShot]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
$exe=Join-Path $repo 'target/x86_64-pc-windows-msvc/release/HaloBatteryNext.exe'
if(!(Test-Path $exe)){throw 'Build release before this test.'}
$p=Start-Process $exe -ArgumentList @('--background','--simulate','--data-dir',"`"$folder`"") -PassThru -WindowStyle Hidden
[InsightsShot]::TargetPid=$p.Id
function Message([IntPtr]$window,[uint32]$message,[uint64]$value=0,[int64]$parameter=0){[InsightsShot]::PostMessage($window,$message,[UIntPtr]$value,[IntPtr]$parameter)|Out-Null}
function Wait-Window([string]$title){
 $deadline=[DateTime]::UtcNow.AddSeconds(10)
 do{Start-Sleep -Milliseconds 100;$window=[InsightsShot]::Find($title)}while($window-eq[IntPtr]::Zero-and[DateTime]::UtcNow-lt$deadline)
 if($window-eq[IntPtr]::Zero){throw "Missing simulation window: $title"};return $window
}
function Wait-Insights([IntPtr]$dashboard){
 $deadline=[DateTime]::UtcNow.AddSeconds(5)
 do{Start-Sleep -Milliseconds 100;$text=[InsightsShot]::Text([InsightsShot]::GetDlgItem($dashboard,78))}while($text-notlike'Local data refreshed*'-and[DateTime]::UtcNow-lt$deadline)
 if($text-notlike'Local data refreshed*'){throw "Insights query did not complete: $text"}
}
try{
 $monitor=Wait-Window 'Halo Battery Next monitor'
 Start-Sleep -Milliseconds 600
 Message $monitor 32776
 $dashboard=Wait-Window 'Halo Battery Next'
 # WM_SYSCHAR with Alt context exercises the real Alt+I mnemonic.
 Message $dashboard 262 105 536870912
 Wait-Insights $dashboard
 foreach($id in 6,10,4,70,71,74,76){if([InsightsShot]::GetDlgItem($dashboard,$id)-eq[IntPtr]::Zero){throw "Insights control $id missing."}}
 $rates=[InsightsShot]::GetDlgItem($dashboard,70);$cycles=[InsightsShot]::GetDlgItem($dashboard,71)
 if([InsightsShot]::SendMessage($rates,395,[UIntPtr]::Zero,[IntPtr]::Zero).ToInt32()-ne2){throw 'Expected exactly two rate comparisons.'}
 if([InsightsShot]::SendMessage($cycles,395,[UIntPtr]::Zero,[IntPtr]::Zero).ToInt32()-ne10){throw 'Expected ten scrollable charge summaries.'}
 foreach($entry in @(@{index=0;hz=1000},@{index=1;hz=8000})){
  [InsightsShot]::SendMessage($rates,390,[UIntPtr]$entry.index,[IntPtr]::Zero)|Out-Null
  Message $dashboard 273 (65536+70);Start-Sleep -Milliseconds 100
  $text=[InsightsShot]::Text([InsightsShot]::GetDlgItem($dashboard,74))
  if($text-notlike"$($entry.hz) Hz*"-or$text-notlike'*samples*observed drops*'-or$text-notlike'*Estimated full-charge use:*'-or$text-notlike'*confidence*'){throw "Rate evidence missing: $text"}
 }
 foreach($entry in @(@{index=0;evidence='Observed charge'},@{index=9;evidence='Partial cycle'})){
  [InsightsShot]::SendMessage($cycles,390,[UIntPtr]$entry.index,[IntPtr]::Zero)|Out-Null
  Message $dashboard 273 (65536+71);Start-Sleep -Milliseconds 100
  $text=[InsightsShot]::Text([InsightsShot]::GetDlgItem($dashboard,76))
  if($text-notlike"*$($entry.evidence)*"-or$text-notlike'*Average observed drain:*percentage points/h*'-or$text-notlike'*estimated awake time*'){throw "Charge evidence missing: $text"}
 }
 [InsightsShot]::SendMessage([InsightsShot]::GetDlgItem($dashboard,10),334,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null
 Message $dashboard 273 (65536+10);Wait-Insights $dashboard
 Message $dashboard 273 4;Wait-Insights $dashboard
 $coverage=[InsightsShot]::Text([InsightsShot]::GetDlgItem($dashboard,78))
 if($coverage-notlike'*readings*discharging*counted use*confirmed rate*'-or$coverage-notlike'*intervals excluded*unreadable rows*'){throw "Coverage summary missing: $coverage"}
 [InsightsShot]::Save($dashboard,(Join-Path $output 'insights-native.png'))
 Message $monitor 32777;Start-Sleep -Milliseconds 100
 $samples=@{}
 foreach($cycle in 1..40){
  Message $monitor 32776;$dashboard=Wait-Window 'Halo Battery Next'
  Message $dashboard 273 6;Wait-Insights $dashboard
  Message $monitor 32777;Start-Sleep -Milliseconds 100
  if($cycle-in1,20,40){$p.Refresh();$samples["$cycle"]=@{user=[InsightsShot]::GetGuiResources($p.Handle,1);gdi=[InsightsShot]::GetGuiResources($p.Handle,0);private=$p.PrivateMemorySize64}}
 }
 if($samples['40'].gdi-gt$samples['1'].gdi-or$samples['40'].user-gt($samples['1'].user+2)){throw 'Native resources grew after the warm Insights lifecycle baseline.'}
 $samples|ConvertTo-Json -Depth 5|Set-Content (Join-Path $output 'insights-resource-cycles.json')
 Message $monitor 32778
 if(!$p.WaitForExit(10000)){throw 'Simulation quit timed out.'}
 Write-Output 'Insights Alt+I, two rates, ten scrollable cycles, evidence/device selection, Refresh and forty resource cycles passed.'
 Write-Output ($samples|ConvertTo-Json -Depth 5)
 Write-Output "Screenshot: $(Join-Path $output 'insights-native.png')"
}finally{if(!$p.HasExited){Stop-Process -Id $p.Id}}
