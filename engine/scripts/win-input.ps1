# Window helpers for engine/scripts/acceptance.ts on Windows.
#   win-input.ps1 rect <hwnd>            -> "left top right bottom"
#   win-input.ps1 front <hwnd>           brings the window to the front
#   win-input.ps1 click <x> <y>          left click at screen coordinates
#   win-input.ps1 key <esc|enter>        one key press to the focused window
#   win-input.ps1 shot <hwnd> <png>      screenshot of the window's rectangle
param([string]$Command, [string]$A, [string]$B)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class JaalWin {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint f, UIntPtr e);
}
"@
# Physical pixels everywhere, so window rects, clicks and screenshots agree on
# a scaled display.
[void][JaalWin]::SetProcessDPIAware()
switch ($Command) {
  "rect" {
    $r = New-Object JaalWin+RECT
    [void][JaalWin]::GetWindowRect([IntPtr][long]$A, [ref]$r)
    "$($r.Left) $($r.Top) $($r.Right) $($r.Bottom)"
  }
  "front" {
    $h = [IntPtr][long]$A
    # An Alt tap lets this process move another window to the front.
    [JaalWin]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero)
    [JaalWin]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
    [void][JaalWin]::ShowWindow($h, 9)
    [void][JaalWin]::SetForegroundWindow($h)
  }
  "click" {
    [void][JaalWin]::SetCursorPos([int]$A, [int]$B)
    [JaalWin]::mouse_event(0x02, 0, 0, 0, [UIntPtr]::Zero)
    [JaalWin]::mouse_event(0x04, 0, 0, 0, [UIntPtr]::Zero)
  }
  "key" {
    $vk = @{ esc = 0x1B; enter = 0x0D }[$A]
    [JaalWin]::keybd_event([byte]$vk, 0, 0, [UIntPtr]::Zero)
    [JaalWin]::keybd_event([byte]$vk, 0, 2, [UIntPtr]::Zero)
  }
  "shot" {
    $r = New-Object JaalWin+RECT
    [void][JaalWin]::GetWindowRect([IntPtr][long]$A, [ref]$r)
    $w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
    $bmp = New-Object System.Drawing.Bitmap $w, $h
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($r.Left, $r.Top, 0, 0, $bmp.Size)
    $bmp.Save($B, [System.Drawing.Imaging.ImageFormat]::Png)
  }
}
