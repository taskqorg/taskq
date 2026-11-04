# A4 retest native watcher through UI Automation and WMI. It only reads. It logs a change, plus a heartbeat every 5 minutes.
# Usage: powershell -File retest-watch.ps1 -Hwnd <Conversations window> -Log <file> -Attachment <file name> [-Hours 12]
param([string]$Hwnd,[string]$Log,[string]$Attachment,[double]$Hours=12)
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$A=[System.Windows.Automation.AutomationElement];$TS=[System.Windows.Automation.TreeScope];$E=[System.Windows.Automation.PropertyCondition]
$steer=New-Object $E($A::AutomationIdProperty,'steer-draft');$answer=New-Object $E($A::AutomationIdProperty,'answer-draft')
$remove=New-Object $E($A::NameProperty,"Remove $Attachment");$history=New-Object $E($A::NameProperty,'Worker history')
$doc=New-Object $E($A::ControlTypeProperty,[System.Windows.Automation.ControlType]::Document)
$row=New-Object $E($A::NameProperty,"6 steps $([char]0xB7) 6 commands")
$chipName=New-Object $E($A::NameProperty,"$Attachment $([char]0xB7) 63 bytes")
$end=(Get-Date).AddHours($Hours);$last='';$beat=[DateTime]::MinValue;$procAt=[DateTime]::MinValue;$procs=''
function Y($x){if($x){[int]$x.Current.BoundingRectangle.Y}else{'none'}}
while((Get-Date) -lt $end){
 $s=[ordered]@{}
 try{$w=$A::FromHandle([IntPtr][long]$Hwnd);$d=$w.FindFirst($TS::Descendants,$doc)
  $s.page=if($d){($d.GetRuntimeId() -join '.')}else{'none'}
  $c=$w.FindFirst($TS::Descendants,$steer);$s.composer='steer-draft';if(!$c){$c=$w.FindFirst($TS::Descendants,$answer);$s.composer='answer-draft'};if(!$c){$s.composer='none'}
  $s.draft=if($c){$c.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value}else{$null}
  $s.chip=[bool]$w.FindFirst($TS::Descendants,$chipName);$s.remove=[bool]$w.FindFirst($TS::Descendants,$remove)
  $s.history_y=Y($w.FindFirst($TS::Descendants,$history));$s.anchor_row_y=Y($w.FindFirst($TS::Descendants,$row));$s.composer_y=Y($c)
 }catch{$s.error=$_.Exception.GetType().Name}
 $now=[DateTime]::UtcNow
 if(($now-$procAt).TotalSeconds -ge 30){$procAt=$now;$all=Get-CimInstance Win32_Process -Property ProcessId,ParentProcessId,Name,CreationDate
  $procs=(($all|?{$_.Name -match '^(taskq|taskqw|claude|codex)\.exe$'}|Sort-Object ProcessId|%{"$($_.ProcessId)<$($_.ParentProcessId):$($_.Name):$($_.CreationDate.ToUniversalTime().ToString('HH:mm:ss.fff'))"}) -join ' ')}
 $s.procs=$procs
 $k=$s|ConvertTo-Json -Compress
 if($k -ne $last -or ($now-$beat).TotalSeconds -ge 300){$ch=$k -ne $last;$last=$k;$beat=$now;Add-Content -Path $Log -Value ($now.ToString('o')+" changed=$ch "+$k) -Encoding UTF8}
 Start-Sleep -Milliseconds 1000
}
