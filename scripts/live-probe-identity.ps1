# Read-only Windows host probe for scripts/live-probe.py (taskq#2094): which existing tools and routes the bounded
# live scenario can use. It installs, grants, trusts and starts nothing; every value stays local.
#   powershell -NoProfile -ExecutionPolicy Bypass -File live-probe-identity.ps1 -Repo CHECKOUT -Install INSTALL -Parent DIR
param([Parameter(Mandatory)][string]$Repo, [Parameter(Mandatory)][string]$Install, [Parameter(Mandatory)][string]$Parent)

function Tool($name, $arg = '--version') {
    $c = Get-Command $name -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $c) { return @{ path = $null; version = 'not found' } }
    $v = try { (& $c.Source $arg 2>&1 | Select-Object -First 1) -as [string] } catch { "unknown ($_)" }
    @{ path = $c.Source; version = $v; windowsapps_alias = $c.Source -like '*\WindowsApps\*' }
}
function Key($p) { try { (Resolve-Path $p).Path.Replace('\', '/').ToLower().TrimEnd('/') } catch { $p.Replace('\', '/').ToLower().TrimEnd('/') } }

$trust = try {
    $j = Get-Content "$HOME\.claude.json" -Raw | ConvertFrom-Json
    $want = (Key $Parent) + '/'
    @($j.projects.PSObject.Properties | Where-Object { $_.Value.hasTrustDialogAccepted -and $want.StartsWith((Key ($_.Name -replace '^//\?/', '')) + '/') } | ForEach-Object Name)
} catch { "unknown ($_)" }
$grants = try {
    @((Get-Content "$Install\worker-policy.json" -Raw | ConvertFrom-Json).grants | Where-Object { (Key $_.project) -eq (Key $Repo) } | ForEach-Object { "$($_.runtime):$($_.mode)" })
} catch { "unknown ($_)" }
$hostJson = try { Get-Content "$Install\host.json" -Raw | ConvertFrom-Json } catch { $null }
$tasks = @(Get-ScheduledTask -ErrorAction SilentlyContinue | Where-Object { $_.TaskName -like '*TaskQ*' } | ForEach-Object {
    @{ name = $_.TaskName; state = "$($_.State)"; logon = "$($_.Principal.LogonType)"; user = $_.Principal.UserId }
})
$py = Tool python
$pyl = Tool py
[ordered]@{
    hostname = $env:COMPUTERNAME; user = $env:USERNAME; os = [Environment]::OSVersion.VersionString
    console_user = (Get-CimInstance Win32_ComputerSystem).UserName
    python = $py; py_launcher = $pyl; node = Tool node; git = Tool git; claude = Tool claude; codex = Tool codex
    codex_login = try { (& codex login status 2>&1 | Select-Object -Last 1) -as [string] } catch { 'unknown' }
    board_token_file = Test-Path "$Install\token"; permission_mode = $hostJson.permission_mode
    claude_trust_covering_parent = $trust; checkout_grants = $grants; taskq_scheduled_tasks = $tasks
    # live-probe.py needs a real Python 3; a WindowsApps alias opens the Store instead of running it
    python_ready = ($py.path -and -not $py.windowsapps_alias -and $py.version -match '^Python 3') -or ($pyl.path -and $pyl.version -match '^Python 3')
} | ConvertTo-Json -Depth 4
