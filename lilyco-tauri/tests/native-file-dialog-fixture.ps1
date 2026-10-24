param([Parameter(Mandatory=$true)][string]$ResultPath)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true') { throw 'Native dialog fixture only runs in Actions' }
Add-Type -AssemblyName System.Windows.Forms
$dialog = [Windows.Forms.OpenFileDialog]::new()
try {
    $dialog.Title = '选择要读取的办公文件'
    $dialog.Filter = 'Office documents|*.docx|All files|*.*'
    $dialog.CheckFileExists = $true
    $answer = $dialog.ShowDialog()
    $value = if ($answer -eq [Windows.Forms.DialogResult]::OK) { $dialog.FileName } else { '<cancel>' }
    [IO.File]::WriteAllText($ResultPath,$value,[Text.UTF8Encoding]::new($false))
    # Keep the real dialog owner alive until the helper records its receipt,
    # matching the product, whose main window remains open after selection.
    $deadline = [DateTime]::UtcNow.AddSeconds(60)
    while (-not (Test-Path -LiteralPath ($ResultPath + '.release'))) {
        if ([DateTime]::UtcNow -ge $deadline) { throw 'Dialog receipt was not acknowledged' }
        Start-Sleep -Milliseconds 100
    }
} finally { $dialog.Dispose() }
