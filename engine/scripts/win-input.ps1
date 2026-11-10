# Window and input helpers for the engine acceptance scripts on Windows.
#   win-input.ps1 rect <hwnd>              -> "left top right bottom"
#   win-input.ps1 client <hwnd>            -> the client area, same form, in screen pixels
#   win-input.ps1 front <hwnd>             brings the window to the front
#   win-input.ps1 click <x> <y>            left click at screen coordinates
#   win-input.ps1 move <x> <y>             moves the pointer there
#   win-input.ps1 wheel <x> <y> <up|down> <notches>   mouse wheel there
#   win-input.ps1 key <chord>              one key or chord to the focused window:
#                                          esc, enter, f11, a, ctrl+t, alt+f4, win+up
#   win-input.ps1 type <text>              types the text into the focused window
#   win-input.ps1 place <hwnd> <x> <y> <w> <h>   moves and resizes the window
#   win-input.ps1 show <hwnd> <minimize|maximize|restore>
#   win-input.ps1 state <hwnd>             -> minimized, maximized or normal
#   win-input.ps1 foreground               -> the foreground window's handle
#   win-input.ps1 shot <hwnd> <png>        screenshot of the window's rectangle
#   win-input.ps1 wake                     closes a running screen saver, so input
#                                          and screenshots reach the desktop
param([string]$Command, [string]$A, [string]$B, [string]$C, [string]$D, [string]$E)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class JaalWin {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
  [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort Vk, Scan; public uint Flags, Time; public IntPtr Extra; }
  // INPUT with its union padded to the size of MOUSEINPUT, the largest member.
  [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint Type; public KEYBDINPUT Key; public long Padding; }
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int height, uint flags);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, int d, UIntPtr e);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint f, UIntPtr e);
  [DllImport("user32.dll")] public static extern uint SendInput(uint n, INPUT[] inputs, int size);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr OpenDesktopW(string name, uint flags, bool inherit, uint access);
  [DllImport("user32.dll")] public static extern bool EnumDesktopWindows(IntPtr desk, EnumProc proc, IntPtr l);
  [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool CloseDesktop(IntPtr d);
  // A screen saver runs on its own desktop, where a WM_CLOSE ends it. While it
  // runs, SendInput and screen copies from the default desktop fail.
  public static int CloseScreenSaver() {
    IntPtr desk = OpenDesktopW("Screen-saver", 0, false, 0x0001 | 0x0040 | 0x0080);
    if (desk == IntPtr.Zero) return 0;
    int closed = 0;
    EnumDesktopWindows(desk, delegate(IntPtr h, IntPtr l) {
      if (IsWindowVisible(h)) { PostMessageW(h, 0x0010, IntPtr.Zero, IntPtr.Zero); closed++; }
      return true;
    }, IntPtr.Zero);
    CloseDesktop(desk);
    return closed;
  }
  // Types one UTF-16 unit as a key press that carries the character itself,
  // so the keyboard layout doesn't matter.
  public static void TypeUnit(char unit) {
    INPUT[] inputs = new INPUT[2];
    inputs[0].Type = 1; inputs[0].Key.Scan = unit; inputs[0].Key.Flags = 0x0004;
    inputs[1].Type = 1; inputs[1].Key.Scan = unit; inputs[1].Key.Flags = 0x0004 | 0x0002;
    SendInput(2, inputs, Marshal.SizeOf(typeof(INPUT)));
  }
}
"@
# Physical pixels everywhere, so window rects, clicks and screenshots agree on
# a scaled display.
[void][JaalWin]::SetProcessDPIAware()
$keys = @{
  esc = 0x1B; enter = 0x0D; tab = 0x09; space = 0x20; backspace = 0x08; delete = 0x2E
  up = 0x26; down = 0x28; left = 0x25; right = 0x27; home = 0x24; end = 0x23
  ctrl = 0x11; shift = 0x10; alt = 0x12; win = 0x5B
}
function VirtualKey([string]$name) {
  if ($keys.ContainsKey($name)) { return $keys[$name] }
  if ($name -match '^f(\d+)$') { return 0x6F + [int]$Matches[1] }
  if ($name.Length -eq 1) { return [int][char]$name.ToUpper() }
  throw "unknown key $name"
}
switch ($Command) {
  "wake" {
    if ([JaalWin]::CloseScreenSaver() -gt 0) { Start-Sleep -Milliseconds 1500 }
  }
  "rect" {
    $r = New-Object JaalWin+RECT
    [void][JaalWin]::GetWindowRect([IntPtr][long]$A, [ref]$r)
    "$($r.Left) $($r.Top) $($r.Right) $($r.Bottom)"
  }
  "client" {
    $h = [IntPtr][long]$A
    $r = New-Object JaalWin+RECT
    [void][JaalWin]::GetClientRect($h, [ref]$r)
    $p = New-Object JaalWin+POINT
    [void][JaalWin]::ClientToScreen($h, [ref]$p)
    "$($p.X) $($p.Y) $($p.X + $r.Right) $($p.Y + $r.Bottom)"
  }
  "front" {
    $h = [IntPtr][long]$A
    # An Alt tap lets this process move another window to the front.
    [JaalWin]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero)
    [JaalWin]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
    if ([JaalWin]::IsIconic($h)) { [void][JaalWin]::ShowWindow($h, 9) }
    [void][JaalWin]::SetForegroundWindow($h)
  }
  "foreground" { [long][JaalWin]::GetForegroundWindow() }
  "click" {
    [void][JaalWin]::SetCursorPos([int]$A, [int]$B)
    Start-Sleep -Milliseconds 60
    [JaalWin]::mouse_event(0x02, 0, 0, 0, [UIntPtr]::Zero)
    [JaalWin]::mouse_event(0x04, 0, 0, 0, [UIntPtr]::Zero)
  }
  "move" { [void][JaalWin]::SetCursorPos([int]$A, [int]$B) }
  "wheel" {
    [void][JaalWin]::SetCursorPos([int]$A, [int]$B)
    Start-Sleep -Milliseconds 60
    # A direction word, not a signed count: PowerShell reads an argument like
    # -12 as a parameter name.
    if ($C -ne "up" -and $C -ne "down") { throw "wheel takes up or down, got '$C'" }
    $step = if ($C -eq "down") { -120 } else { 120 }
    for ($i = 0; $i -lt [int]$D; $i++) {
      [JaalWin]::mouse_event(0x0800, 0, 0, $step, [UIntPtr]::Zero)
      Start-Sleep -Milliseconds 15
    }
  }
  "key" {
    $parts = $A.ToLower().Split("+") | ForEach-Object { VirtualKey $_ }
    foreach ($vk in $parts) { [JaalWin]::keybd_event([byte]$vk, 0, 0, [UIntPtr]::Zero) }
    [array]::Reverse($parts)
    foreach ($vk in $parts) { [JaalWin]::keybd_event([byte]$vk, 0, 2, [UIntPtr]::Zero) }
  }
  "type" {
    foreach ($unit in $A.ToCharArray()) { [JaalWin]::TypeUnit($unit); Start-Sleep -Milliseconds 12 }
  }
  "place" {
    # SWP_NOZORDER | SWP_NOACTIVATE
    [void][JaalWin]::SetWindowPos([IntPtr][long]$A, [IntPtr]::Zero, [int]$B, [int]$C, [int]$D, [int]$E, 0x0004 -bor 0x0010)
  }
  "show" {
    $cmd = @{ minimize = 6; maximize = 3; restore = 9 }[$B]
    [void][JaalWin]::ShowWindow([IntPtr][long]$A, $cmd)
  }
  "state" {
    $h = [IntPtr][long]$A
    if ([JaalWin]::IsIconic($h)) { "minimized" } elseif ([JaalWin]::IsZoomed($h)) { "maximized" } else { "normal" }
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
  default { throw "unknown command $Command" }
}
