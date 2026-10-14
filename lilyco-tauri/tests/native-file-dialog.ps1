param(
  [Parameter(Mandatory=$true)][string]$Application,
  [Parameter(Mandatory=$true)][ValidateSet('select','cancel')][string]$Mode,
  [string]$Fixture
)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true') { throw 'Actual Office file dialog acceptance only runs in Actions' }
$applicationPath = [IO.Path]::GetFullPath($Application)
$owners = @(Get-Process -Name 'lilyco-office' -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $applicationPath })
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
    [DllImport("user32.dll", EntryPoint="SendMessageTimeoutW")] public static extern IntPtr Send(IntPtr window, uint message, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr result);
    public static IntPtr Find(uint pid) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((window, unused) => {
            uint owner; GetWindowThreadProcessId(window, out owner);
            if (owner != pid) return true;
            var name = new StringBuilder(512); var type = new StringBuilder(128);
            GetWindowText(window, name, name.Capacity); GetClassName(window, type, type.Capacity);
            if (type.ToString() != "#32770" || name.ToString() != "选择要读取的办公文件") return true;
            if (found != IntPtr.Zero) throw new InvalidOperationException("Ambiguous native Office file dialog");
            found = window; return true;
        }, IntPtr.Zero);
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
}
$button = [OfficeFileDialog]::GetDlgItem($dialog, $(if ($Mode -eq 'select') { 1 } else { 2 }))
if ($button -eq [IntPtr]::Zero) { throw 'Actual native Open/Cancel button is missing' }
if ([OfficeFileDialog]::Send($button,0x00F5,[IntPtr]::Zero,[IntPtr]::Zero,2,2000,[ref]$result) -eq [IntPtr]::Zero) { throw 'Native Open/Cancel button did not respond' }
Write-Output "Used actual native Office file dialog: $Mode"
