param(
  [Parameter(Mandatory=$true)][string]$Application,
  [Parameter(Mandatory=$true)][ValidateSet('select','cancel')][string]$Mode,
  [string]$Fixture,
  [int]$OwnerProcessId = 0
)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true') { throw 'Actual Office file dialog acceptance only runs in Actions' }
$applicationPath = [IO.Path]::GetFullPath($Application)
$owners = if ($OwnerProcessId -gt 0) { @(Get-Process -Id $OwnerProcessId -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $applicationPath }) } else {
  @(Get-Process -Name 'lilyco-office' -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $applicationPath })
}
if ($owners.Count -ne 1) { throw 'Expected one actual WebDriver-owned Office binary' }
if ($Mode -eq 'select' -and -not (Test-Path -LiteralPath $Fixture -PathType Leaf)) { throw 'Actual file dialog fixture is missing' }
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class OfficeFileDialog {
    public delegate bool Callback(IntPtr window, IntPtr data);
    [DllImport("user32.dll")] public static extern bool EnumWindows(Callback callback, IntPtr data);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr window, Callback callback, IntPtr data);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr window, StringBuilder text, int size);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr window, StringBuilder text, int size);
    [DllImport("user32.dll")] public static extern int GetDlgCtrlID(IntPtr window);
    [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr window, int id);
    [DllImport("user32.dll", EntryPoint="SendMessageTimeoutW", CharSet=CharSet.Unicode)] public static extern IntPtr SetText(IntPtr window, uint message, IntPtr w, string text, uint flags, uint timeout, out IntPtr result);
    [DllImport("user32.dll", EntryPoint="SendMessageTimeoutW", CharSet=CharSet.Unicode)] public static extern IntPtr ReadText(IntPtr window, uint message, IntPtr w, [Out] StringBuilder text, uint flags, uint timeout, out IntPtr result);
    [DllImport("user32.dll", EntryPoint="SendMessageTimeoutW")] public static extern IntPtr Send(IntPtr window, uint message, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr result);
    public static IntPtr Find(uint pid) {
        IntPtr found = IntPtr.Zero;
        int matches = 0;
        EnumWindows((window, unused) => {
            uint owner; GetWindowThreadProcessId(window, out owner);
            if (owner != pid) return true;
            var name = new StringBuilder(512); var type = new StringBuilder(128);
            GetWindowText(window, name, name.Capacity); GetClassName(window, type, type.Capacity);
            if (type.ToString() != "#32770" || name.ToString() != "选择要读取的办公文件") return true;
            matches++;
            found = window; return true;
        }, IntPtr.Zero);
        if (matches > 1) throw new InvalidOperationException("Ambiguous native Office file dialog");
        return found;
    }
    public static IntPtr Filename(IntPtr dialog) {
        IntPtr found = IntPtr.Zero;
        EnumChildWindows(dialog, (window, unused) => {
            var type = new StringBuilder(128); GetClassName(window, type, type.Capacity);
            int id = GetDlgCtrlID(window);
            if (type.ToString() == "Edit" && (id == 1148 || id == 1152)) found = window;
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
'@
$deadline = [DateTime]::UtcNow.AddSeconds(30)
$dialog = [IntPtr]::Zero
while ([DateTime]::UtcNow -lt $deadline) {
    $dialog = [OfficeFileDialog]::Find([uint32]$owners[0].Id)
    if ($dialog -ne [IntPtr]::Zero) { break }
    Start-Sleep -Milliseconds 100
}
if ($dialog -eq [IntPtr]::Zero) { throw 'Actual native file chooser did not appear' }
[IntPtr]$result = [IntPtr]::Zero
if ($Mode -eq 'select') {
    $edit = [OfficeFileDialog]::Filename($dialog)
    if ($edit -eq [IntPtr]::Zero) { throw 'Actual native filename field is missing' }
    if ([OfficeFileDialog]::SetText($edit,0x000C,[IntPtr]::Zero,[IO.Path]::GetFullPath($Fixture),2,2000,[ref]$result) -eq [IntPtr]::Zero) { throw 'Native filename field did not accept the fixture path' }
    $actualText = [Text.StringBuilder]::new(32768)
    # GetWindowText cannot read another process's edit control (Win32 contract).
    if ([OfficeFileDialog]::ReadText($edit,0x000D,[IntPtr]$actualText.Capacity,$actualText,2,2000,[ref]$result) -eq [IntPtr]::Zero) { throw 'Native filename field could not be read back' }
    # CI-owned fixture paths only; capture the real control before classifying
    # a mismatch, rather than guessing whether the shell normalized/reset it.
    @{filename_control_id=[OfficeFileDialog]::GetDlgCtrlID($edit);expected_text=[IO.Path]::GetFullPath($Fixture);observed_text=$actualText.ToString();copied_characters=$result.ToInt64();owner_pid=$owners[0].Id} | ConvertTo-Json -Compress | Write-Output
    if ($actualText.ToString() -ne [IO.Path]::GetFullPath($Fixture)) { throw 'Native filename field differs from the requested fixture' }
}
$commandId = if ($Mode -eq 'select') { 1 } else { 2 }
$button = [OfficeFileDialog]::GetDlgItem($dialog, $commandId)
if ($button -eq [IntPtr]::Zero) { throw 'Actual native Open/Cancel button is missing' }
# Send the actual dialog's BN_CLICKED command. BM_CLICK on an inactive button
# can return without accepting the dialog. Never replay an uncertain action.
$sent = [OfficeFileDialog]::Send($dialog,0x0111,[IntPtr]$commandId,$button,2,2000,[ref]$result) -ne [IntPtr]::Zero
$deadline = [DateTime]::UtcNow.AddSeconds(15)
while ([DateTime]::UtcNow -lt $deadline -and [OfficeFileDialog]::Find([uint32]$owners[0].Id) -ne [IntPtr]::Zero) { Start-Sleep -Milliseconds 100 }
if ([OfficeFileDialog]::Find([uint32]$owners[0].Id) -ne [IntPtr]::Zero) { throw "Native $Mode command did not close its actual dialog (message acknowledged: $sent)" }
if ($owners[0].HasExited) { throw 'Dialog owner exited before the selection could be delivered' }
@{mode=$Mode;actual_dialog_closed=$true;message_acknowledged=$sent;filename_verified=($Mode -eq 'select')} | ConvertTo-Json -Compress | Write-Output
Write-Output "Used actual native Office file dialog: $Mode"
