$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true') { throw 'Shortcut contracts only run in Actions' }
. (Join-Path $PSScriptRoot 'shortcut_icon.ps1')
$binary = Join-Path $env:RUNNER_TEMP 'Office shortcut contract/lilyco-office.exe'
foreach ($location in @('', ',0', "$binary,0", ('"'+$binary+'",0'))) {
    if (-not (Test-InstalledShortcutIcon $location $binary $binary)) { throw "Valid icon representation rejected: $location" }
}
foreach ($location in @("$binary,1", "$binary.bak,0", 'relative.exe,0', 'other.exe,0')) {
    if (Test-InstalledShortcutIcon $location $binary $binary) { throw 'Wrong icon target or index accepted' }
}
if (Test-InstalledShortcutIcon ',0' ($binary+'.bak') $binary) { throw 'Implicit icon from wrong target accepted' }
$installDirectory = Split-Path -Parent $binary
foreach ($location in @($installDirectory, ('"'+$installDirectory+'"'), ($installDirectory+'\'))) {
    if (-not (Test-InstalledLocation $location $installDirectory)) { throw 'Valid quoted or unquoted InstallLocation rejected' }
}
foreach ($location in @('', 'relative', ($installDirectory+'-other'), ('"'+$installDirectory+'" --extra'), ('"'+$installDirectory))) {
    if (Test-InstalledLocation $location $installDirectory) { throw 'Different or malformed InstallLocation accepted' }
}
# Exercise WSH itself, not only string fixtures.
$linkPath = Join-Path $env:RUNNER_TEMP 'office-shortcut-icon-contract.lnk'
$shell = New-Object -ComObject WScript.Shell
try {
    $link = $shell.CreateShortcut($linkPath); $link.TargetPath = $binary; $link.Save()
    $actual = $shell.CreateShortcut($linkPath)
    if (-not (Test-InstalledShortcutIcon $actual.IconLocation $actual.TargetPath $binary)) { throw 'Actual WSH default icon rejected' }
    Write-Output ('WSH default IconLocation: '+$actual.IconLocation)
} finally { if (Test-Path -LiteralPath $linkPath) { Remove-Item -LiteralPath $linkPath } }
Write-Output 'Shortcut icon path/index and real WSH default contracts passed.'
