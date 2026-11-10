# Screenshot one app's main window as the screen shows it, owned windows
# (the gpui-a command bar) and their shadows included. Windows counterpart
# of window-shot.swift.
#
#   powershell -File scripts/window-shot.ps1 <pid> <out.png>
#
# Captures the screen inside the process's largest visible window, using
# DWM's frame bounds (GetWindowRect includes the invisible resize border).
# The window is made topmost for the capture, so other apps' windows don't
# cover it, and put back after. That moves no focus and sends no input.

param(
    [Parameter(Mandatory = $true)][int]$ProcessId,
    [Parameter(Mandatory = $true)][string]$Out
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class JaalShot {
    public delegate bool EnumProc(IntPtr hwnd, IntPtr lparam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr lparam);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr value);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr hwnd, int attr, out RECT rect, int size);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hwnd, IntPtr after, int x, int y, int cx, int cy, uint flags);

    // SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE
    const uint KeepPlace = 0x0001 | 0x0002 | 0x0010;
    public static void SetTopmost(IntPtr hwnd, bool on) {
        SetWindowPos(hwnd, new IntPtr(on ? -1 : -2), 0, 0, 0, 0, KeepPlace);
    }

    public static IntPtr MainWindow(uint pid, out RECT bounds) {
        IntPtr bestWindow = IntPtr.Zero;
        RECT best = new RECT();
        long bestArea = 0;
        EnumWindows((hwnd, _) => {
            uint owner;
            GetWindowThreadProcessId(hwnd, out owner);
            if (owner != pid || !IsWindowVisible(hwnd)) return true;
            RECT r;
            // DWMWA_EXTENDED_FRAME_BOUNDS
            if (DwmGetWindowAttribute(hwnd, 9, out r, Marshal.SizeOf(typeof(RECT))) != 0) return true;
            long area = (long)(r.Right - r.Left) * (r.Bottom - r.Top);
            if (area > bestArea) { bestArea = area; best = r; bestWindow = hwnd; }
            return true;
        }, IntPtr.Zero);
        bounds = best;
        return bestWindow;
    }
}
"@

# Physical pixels, the same space DWM reports bounds in.
[void][JaalShot]::SetProcessDpiAwarenessContext([IntPtr]::new(-4))
$r = New-Object JaalShot+RECT
$window = [JaalShot]::MainWindow([uint32]$ProcessId, [ref]$r)
$width = $r.Right - $r.Left
$height = $r.Bottom - $r.Top
if ($width -le 0 -or $height -le 0) { throw "no visible window for process $ProcessId" }
[JaalShot]::SetTopmost($window, $true)
# Let DWM compose a frame with the window on top.
Start-Sleep -Milliseconds 250
$bitmap = New-Object System.Drawing.Bitmap $width, $height
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
try {
    $graphics.CopyFromScreen($r.Left, $r.Top, 0, 0, $bitmap.Size)
} finally {
    [JaalShot]::SetTopmost($window, $false)
}
$bitmap.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$graphics.Dispose()
$bitmap.Dispose()
Write-Output "$Out ${width}x$height"
