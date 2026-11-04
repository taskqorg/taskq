param([string]$Hwnd,[string]$Log)
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$A=[System.Windows.Automation.AutomationElement];$TS=[System.Windows.Automation.TreeScope]
$idc=New-Object System.Windows.Automation.PropertyCondition($A::AutomationIdProperty,'steer-draft')
$nc=New-Object System.Windows.Automation.PropertyCondition($A::NameProperty,'Task info')
$rc=New-Object System.Windows.Automation.PropertyCondition($A::AutomationIdProperty,'RootWebArea')
$end=(Get-Date).AddMinutes(40)
while((Get-Date) -lt $end){
 $t=[DateTime]::UtcNow.ToString('o');$row="$t "
 try{$e=$A::FromHandle([IntPtr][long]$Hwnd)
  $d=$e.FindFirst($TS::Descendants,$idc);$i=$e.FindFirst($TS::Descendants,$nc)
  $row+="draft=["+$(if($d){$d.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value}else{'<none>'})+"] taskinfoY="+$(if($i){$i.Current.BoundingRectangle.Y}else{'<none>'})
 }catch{$row+="ERR "+$_.Exception.Message}
 Add-Content -Path $Log -Value $row -Encoding UTF8
 Start-Sleep -Milliseconds 800
}
