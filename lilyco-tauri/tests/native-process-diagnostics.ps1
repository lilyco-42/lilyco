param([Parameter(Mandatory = $true)][int]$ApplicationPid)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true') { throw 'Native diagnostics only run in GitHub Actions' }
# Read only the CI-owned application tree. Do not expose unrelated commands,
# user environment variables, profiles, or credentials, and never kill by name.
$nativeProcesses = @(Get-CimInstance Win32_Process)
$ownedIds = [System.Collections.Generic.HashSet[int]]::new()
[void]$ownedIds.Add($ApplicationPid)
do {
    $added = $false
    foreach ($nativeProcess in $nativeProcesses) {
        if ($ownedIds.Contains([int]$nativeProcess.ParentProcessId)) {
            if ($ownedIds.Add([int]$nativeProcess.ProcessId)) { $added = $true }
        }
    }
} while ($added)
$owned = @($nativeProcesses | Where-Object { $ownedIds.Contains([int]$_.ProcessId) } |
    Select-Object ProcessId, ParentProcessId, Name, CommandLine)
$runtimeVersions = @()
foreach ($registryRoot in @('HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients', 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients', 'HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients')) {
    if (Test-Path -LiteralPath $registryRoot) {
        foreach ($registration in Get-ChildItem -LiteralPath $registryRoot) {
            $properties = Get-ItemProperty -LiteralPath $registration.PSPath
            if ($properties.name -like '*WebView2*') {
                $runtimeVersions += @{ name = $properties.name; version = $properties.pv }
            }
        }
    }
}
$driverVersion = (& msedgedriver.exe --version | Out-String).Trim()
@{ application_pid = $ApplicationPid; owned_processes = $owned; webview2 = $runtimeVersions; driver_version = $driverVersion } |
    ConvertTo-Json -Depth 5
