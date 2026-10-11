param([Parameter(Mandatory=$true)][string]$Binary)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true') { throw 'CI only' }
$binaryPath = [IO.Path]::GetFullPath($Binary)
# PyInstaller has a parent bootloader and child application. Identify windows by
# the actual executable path, never by a page title (WebView2 may change it).
$owners = @(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -eq $binaryPath } | ForEach-Object { [uint32]$_.ProcessId })
if ($owners.Count -eq 0) { throw 'No packaged application process found' }
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class StudioWindow {
    public delegate bool WindowCallback(IntPtr window, IntPtr data);
    [DllImport("user32.dll")] public static extern bool EnumWindows(WindowCallback callback, IntPtr data);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr window, StringBuilder name, int size);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window, uint message, IntPtr w, IntPtr l);
}
'@
$script:closed = 0
$callback = [StudioWindow+WindowCallback]{ param($window,$data)
    [uint32]$owner = 0
    [void][StudioWindow]::GetWindowThreadProcessId($window,[ref]$owner)
    if ($owners -contains $owner -and [StudioWindow]::IsWindowVisible($window)) {
        $class = [Text.StringBuilder]::new(256)
        [void][StudioWindow]::GetClassName($window,$class,256)
        if ($class.ToString() -ne 'ConsoleWindowClass') {
            if (-not [StudioWindow]::PostMessage($window,0x0010,[IntPtr]::Zero,[IntPtr]::Zero)) { throw 'WM_CLOSE failed' }
            $script:closed++
        }
    }
    return $true
}
[void][StudioWindow]::EnumWindows($callback,[IntPtr]::Zero)
if ($script:closed -eq 0) { throw 'No visible native application window found' }
Write-Output "Closed $script:closed packaged native window(s) normally"
