$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or -not $env:RUNNER_TEMP) { throw 'Real Office installer acceptance only runs in Actions' }
$workspace = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$temporaryRoot = [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\','/')
$installation = [IO.Path]::GetFullPath((Join-Path $temporaryRoot 'lilyco office installer acceptance'))
if (-not $installation.StartsWith($temporaryRoot + [IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)) { throw 'Installation escaped runner temp' }
if (Test-Path -LiteralPath $installation) { throw 'Fresh installer fixture path already exists' }
$installers = @(Get-ChildItem -LiteralPath (Join-Path $workspace 'target/release/bundle/nsis') -Filter '*-setup.exe')
if ($installers.Count -ne 1) { throw 'Expected one actual current NSIS bundle' }
$installer = $installers[0].FullName
$installed = Start-Process -FilePath $installer -ArgumentList @('/S',"/D=$installation") -WindowStyle Hidden -PassThru
if (-not $installed.WaitForExit(180000)) { throw 'Actual Office installer exceeded 180 seconds' }
if ($installed.ExitCode -ne 0) { throw "Actual Office installer failed: $($installed.ExitCode)" }
$binary = Join-Path $installation 'lilyco-office.exe'
$engine = Join-Path $installation 'lbin.exe'
foreach ($payload in @($binary,$engine)) {
    if (-not (Test-Path -LiteralPath $payload)) { throw "Installed binary missing: $payload" }
}
# Tauri patches the application with the NSIS marker and restores the build
# directory copy. Require that exact expected image and unchanged sidecar bytes.
& python (Join-Path $workspace 'lilyco-tauri/tests/nsis_payload.py') `
    --original-app (Join-Path $workspace 'target/release/lilyco-office.exe') `
    --installed-app $binary --original-engine (Join-Path $workspace 'target/release/lbin.exe') `
    --installed-engine $engine --report (Join-Path $workspace 'lilyco-tauri/tests/evidence/installed-payload.json')
if ($LASTEXITCODE -ne 0) { throw 'Installed Office payload differs from the reviewed NSIS image or original sidecar' }
foreach ($name in @('ICON-LICENSE.md','icon-source.png','icon-source.json','FRONTEND-LICENSES.json','RUST-LICENSES.json','MICROSOFT-WEBVIEW2-SDK-LICENSE.txt')) {
    $copies = @(Get-ChildItem -LiteralPath $installation -Recurse -File -Filter $name)
    if ($copies.Count -ne 1) { throw "Installed original/notice missing or duplicate: $name" }
    $original = Join-Path $workspace "lilyco-tauri/resources/$name"
    if ((Get-FileHash -LiteralPath $copies[0].FullName -Algorithm SHA256).Hash -ne (Get-FileHash -LiteralPath $original -Algorithm SHA256).Hash) { throw "Installed resource bytes changed: $name" }
}
$ownedFiles = @(Get-ChildItem -LiteralPath $installation -Recurse -File | ForEach-Object FullName)
# Test data intentionally lives beside the program: uninstall must retain it.
$customerFile = Join-Path $installation 'customer-original.docx'
$fixture = Get-ChildItem -LiteralPath (Join-Path $workspace 'lilyco-binfmt/tests/fixtures/office') -Filter '*.docx' | Sort-Object Name | Select-Object -First 1
Copy-Item -LiteralPath $fixture.FullName -Destination $customerFile
$customerHash = (Get-FileHash -LiteralPath $customerFile -Algorithm SHA256).Hash
$startMenu = [Environment]::GetFolderPath('Programs')
$shell = New-Object -ComObject WScript.Shell
$shortcuts = @(Get-ChildItem -LiteralPath $startMenu -Recurse -File -Filter 'lilyco office.lnk' | Where-Object {
    [IO.Path]::GetFullPath($shell.CreateShortcut($_.FullName).TargetPath) -eq [IO.Path]::GetFullPath($binary)
})
if ($shortcuts.Count -lt 1) { throw 'Real start menu shortcut to installed program is missing' }
. (Join-Path $PSScriptRoot 'shortcut_icon.ps1')
$shortcutEvidence = @()
foreach ($shortcut in $shortcuts) {
    $link = $shell.CreateShortcut($shortcut.FullName)
    $shortcutEvidence += @{target=$link.TargetPath;icon_location=$link.IconLocation}
    $shortcutEvidence | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $workspace 'lilyco-tauri/tests/evidence/installed-shortcuts.json') -Encoding utf8
    if (-not (Test-InstalledShortcutIcon $link.IconLocation $link.TargetPath $binary)) { throw 'Shortcut icon does not reference the installed icon-bearing program at index zero' }
}
$registryRoots = @('HKCU:/Software/Microsoft/Windows/CurrentVersion/Uninstall','HKLM:/Software/Microsoft/Windows/CurrentVersion/Uninstall','HKLM:/Software/WOW6432Node/Microsoft/Windows/CurrentVersion/Uninstall')
$entries = @()
foreach ($root in $registryRoots) {
    if (Test-Path -LiteralPath $root) {
        foreach ($key in Get-ChildItem -LiteralPath $root) {
            $entry = Get-ItemProperty -LiteralPath $key.PSPath
            if ($entry.InstallLocation -and $entry.InstallLocation.TrimEnd('\','/') -eq $installation) { $entries += $key.PSPath }
        }
    }
}
if ($entries.Count -lt 1) { throw 'Installed program is absent from Add/Remove Programs' }
# A second actual native IPC round trip, now from installed bytes and sidecar.
& python (Join-Path $workspace 'lilyco-tauri/tests/native_workbench.py') --binary $binary --engine $engine --label installed-windows
if ($LASTEXITCODE -ne 0) { throw 'Installed Office UI/native/CLI acceptance failed' }
$uninstaller = Join-Path $installation 'uninstall.exe'
if (-not (Test-Path -LiteralPath $uninstaller)) { throw 'Actual installed uninstaller missing' }
$waitCopy = [IO.Path]::GetFullPath((Join-Path $temporaryRoot 'office-uninstaller-for-wait.exe'))
if (-not $waitCopy.StartsWith($temporaryRoot + [IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)) { throw 'Uninstaller copy escaped runner temp' }
Copy-Item -LiteralPath $uninstaller -Destination $waitCopy
if ((Get-FileHash -LiteralPath $waitCopy).Hash -ne (Get-FileHash -LiteralPath $uninstaller).Hash) { throw 'Uninstaller copy differs' }
# NSIS _?= disables the temporary parent/child hop, so this waits for real cleanup.
$removed = Start-Process -FilePath $waitCopy -ArgumentList @('/S',"_?=$installation") -WindowStyle Hidden -PassThru
if (-not $removed.WaitForExit(120000)) { throw 'Actual Office uninstaller exceeded 120 seconds' }
if ($removed.ExitCode -ne 0) { throw "Actual uninstaller failed: $($removed.ExitCode)" }
foreach ($path in $ownedFiles) { if (Test-Path -LiteralPath $path) { throw "Owned installed payload remains: $path" } }
foreach ($shortcut in $shortcuts) { if (Test-Path -LiteralPath $shortcut.FullName) { throw 'Owned shortcut remains after uninstall' } }
foreach ($key in $entries) { if (Test-Path -LiteralPath $key) { throw 'Owned uninstall registry entry remains' } }
if (-not (Test-Path -LiteralPath $customerFile) -or (Get-FileHash -LiteralPath $customerFile).Hash -ne $customerHash) { throw 'Uninstaller deleted or changed customer data' }
$evidence = Join-Path $workspace 'lilyco-tauri/tests/evidence'
[IO.Directory]::CreateDirectory($evidence) | Out-Null
$report = [ordered]@{
    installer_sha256 = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
    actual_nsis_install = $true
    installed_exact_nsis_marked_app_and_original_sidecar_bytes = $true
    installed_full_icon_and_frontend_notices = $true
    installed_original_rust_and_sdk_notices = $true
    start_menu_target_and_icon = $true
    actual_installed_native_ui_to_sidecar = $true
    waited_for_actual_uninstaller = $true
    owned_payloads_removed = $true
    shortcut_and_registry_removed = $true
    customer_original_preserved = $true
}
[IO.File]::WriteAllText((Join-Path $evidence 'installer-acceptance.json'), ($report | ConvertTo-Json) + "`n", [Text.UTF8Encoding]::new($false))
Write-Output 'Actual Office NSIS install, installed native UI and safe uninstall passed.'
