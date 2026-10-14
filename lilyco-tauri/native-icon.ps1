param(
  [Parameter(Mandatory=$true)][string]$Binary,
  [Parameter(Mandatory=$true)][string]$Output
)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true') { throw 'Native Office icon inspection only runs in Actions' }
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class OfficeWindowIcon {
    public delegate bool WindowCallback(IntPtr window, IntPtr data);
    [DllImport("user32.dll")] public static extern bool EnumWindows(WindowCallback callback, IntPtr data);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll", SetLastError=true)] public static extern IntPtr SendMessageTimeout(IntPtr window, uint message, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr result);
    [DllImport("user32.dll", EntryPoint="GetClassLongPtrW")] public static extern IntPtr GetClassLongPtr(IntPtr window, int index);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window, uint message, IntPtr w, IntPtr l);
    public static IntPtr FindOwnedWindow(uint pid) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((window, data) => {
            uint owner;
            GetWindowThreadProcessId(window, out owner);
            if (owner != pid) return true;
            var title = new StringBuilder(512);
            GetWindowText(window, title, title.Capacity);
            if (!title.ToString().StartsWith("lilyco office")) return true;
            found = window;
            return false;
        }, IntPtr.Zero);
        return found;
    }
}
'@
$binaryPath = [IO.Path]::GetFullPath($Binary)
$outputPath = [IO.Path]::GetFullPath($Output)
[IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($outputPath)) | Out-Null
$start = [Diagnostics.ProcessStartInfo]::new($binaryPath)
$start.UseShellExecute = $false
$start.CreateNoWindow = $true
$start.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
$process = [Diagnostics.Process]::Start($start)
try {
    $deadline = [DateTime]::UtcNow.AddSeconds(60)
    $window = [IntPtr]::Zero
    while ([DateTime]::UtcNow -lt $deadline) {
        $process.Refresh()
        if ($process.HasExited) { throw "Office exited before constructing its native window: $($process.ExitCode)" }
        $window = $process.MainWindowHandle
        if ($window -eq [IntPtr]::Zero) { $window = [OfficeWindowIcon]::FindOwnedWindow([uint32]$process.Id) }
        if ($window -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($window -eq [IntPtr]::Zero) { throw 'Office native window did not appear within 60 seconds' }
    [uint32]$owner = 0
    [void][OfficeWindowIcon]::GetWindowThreadProcessId($window,[ref]$owner)
    if ($owner -ne $process.Id) { throw 'Icon inspection must target the launched product process' }
    $iconHandle = [IntPtr]::Zero
    $iconDeadline = [DateTime]::UtcNow.AddSeconds(15)
    while ([DateTime]::UtcNow -lt $iconDeadline -and $iconHandle -eq [IntPtr]::Zero) {
        # Windows may set only the per-window small icon, and it may be assigned
        # after HWND creation. Read BIG, SMALL and SMALL2 before class fallbacks.
        foreach ($kind in @(1,0,2)) {
            [void][OfficeWindowIcon]::SendMessageTimeout($window,0x007F,[IntPtr]$kind,[IntPtr]::Zero,2,1000,[ref]$iconHandle)
            if ($iconHandle -ne [IntPtr]::Zero) { break }
        }
        foreach ($index in @(-14,-34)) {
            if ($iconHandle -eq [IntPtr]::Zero) { $iconHandle = [OfficeWindowIcon]::GetClassLongPtr($window,$index) }
        }
        if ($iconHandle -eq [IntPtr]::Zero) { Start-Sleep -Milliseconds 100 }
    }
    if ($iconHandle -eq [IntPtr]::Zero) { throw 'Actual native Office window has no big or small icon after initialization' }
    # Read the OS-owned icon; this test never sets a window icon.
    $icon = [Drawing.Icon]::FromHandle($iconHandle).Clone()
    try {
        $bitmap = $icon.ToBitmap()
        try { $bitmap.Save($outputPath,[Drawing.Imaging.ImageFormat]::Png) } finally { $bitmap.Dispose() }
    } finally { $icon.Dispose() }
    [void][OfficeWindowIcon]::PostMessage($window,0x0010,[IntPtr]::Zero,[IntPtr]::Zero)
    if (-not $process.WaitForExit(30000)) { throw 'Office did not exit after native WM_CLOSE' }
    if ($process.ExitCode -ne 0) { throw "Office failed normal close: $($process.ExitCode)" }
    Write-Output 'Read actual Office window icon and closed its process normally.'
} finally {
    if (-not $process.HasExited) { $process.Kill($true); $process.WaitForExit() }
    $process.Dispose()
}
