# Win32 helpers for driving/verifying the pet window (dot-source this file).
#
#   . .\scripts\ui-probe.ps1
#   $h = Find-PetWindow "kasumi-live_default"
#   Get-WindowRect $h
#   Invoke-Click 2100 900
#
# All coordinates are physical pixels; the helpers call SetProcessDPIAware once.

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public class PetProbe {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, IntPtr e);
  [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int idx);
  [DllImport("user32.dll", SetLastError=true)] public static extern IntPtr SendMessageTimeoutW(IntPtr h, uint msg, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr result);
  public struct RECT { public int Left, Top, Right, Bottom; }

  public static List<string> Titles() {
    SetProcessDPIAware();
    var list = new List<string>();
    EnumWindows((h,l) => {
      var t = new StringBuilder(256); GetWindowTextW(h, t, 256);
      var title = t.ToString();
      if (title.Length > 0) { RECT r; GetWindowRect(h, out r);
        list.Add(title + "|" + r.Left + "," + r.Top + "," + (r.Right-r.Left) + "x" + (r.Bottom-r.Top) + "|vis=" + IsWindowVisible(h)); }
      return true;
    }, IntPtr.Zero);
    return list;
  }
  public static IntPtr Find(string title) {
    SetProcessDPIAware();
    IntPtr found = IntPtr.Zero;
    EnumWindows((h,l) => {
      var t = new StringBuilder(256); GetWindowTextW(h, t, 256);
      if (t.ToString() == title) { found = h; return false; }
      return true;
    }, IntPtr.Zero);
    return found;
  }
  public static string RectOf(IntPtr h) { RECT r; GetWindowRect(h, out r); return r.Left + "," + r.Top + "," + (r.Right-r.Left) + "x" + (r.Bottom-r.Top); }
  public static long ExStyle(IntPtr h) { return (long)GetWindowLongPtrW(h, -20); }
  public static bool Responsive(IntPtr h) { IntPtr r; return SendMessageTimeoutW(h, 0, IntPtr.Zero, IntPtr.Zero, 2, 2000, out r) != IntPtr.Zero; }
  public static void Click(int x, int y) {
    SetCursorPos(x, y); System.Threading.Thread.Sleep(110);
    mouse_event(0x0002, 0, 0, 0, IntPtr.Zero); System.Threading.Thread.Sleep(110);
    mouse_event(0x0004, 0, 0, 0, IntPtr.Zero);
  }
  public static void RightClick(int x, int y) {
    SetCursorPos(x, y); System.Threading.Thread.Sleep(110);
    mouse_event(0x0008, 0, 0, 0, IntPtr.Zero); System.Threading.Thread.Sleep(110);
    mouse_event(0x0010, 0, 0, 0, IntPtr.Zero);
  }
  public static void Drag(int x0, int y0, int x1, int y1, int steps) {
    SetCursorPos(x0, y0); System.Threading.Thread.Sleep(140);
    mouse_event(0x0002, 0, 0, 0, IntPtr.Zero); System.Threading.Thread.Sleep(140);
    for (int i = 1; i <= steps; i++) {
      SetCursorPos(x0 + (x1-x0)*i/steps, y0 + (y1-y0)*i/steps);
      System.Threading.Thread.Sleep(40);
    }
    System.Threading.Thread.Sleep(140); mouse_event(0x0004, 0, 0, 0, IntPtr.Zero);
  }
}
"@ -ErrorAction SilentlyContinue

function Find-PetWindow { param([string]$Title) [PetProbe]::Find($Title) }
function Get-WindowRect { param([IntPtr]$Handle) [PetProbe]::RectOf($Handle) }
function Test-WindowVisible { param([string]$Title) $h = [PetProbe]::Find($Title); $h -ne [IntPtr]::Zero -and [PetProbe]::IsWindowVisible($h) }
function Test-WindowResponsive { param([IntPtr]$Handle) [PetProbe]::Responsive($Handle) }
function Get-ExStyle { param([IntPtr]$Handle) [PetProbe]::ExStyle($Handle) }
function Test-ClickThrough { param([IntPtr]$Handle) ((Get-ExStyle $Handle) -band 0x20) -ne 0 }
function Move-CursorTo { param([int]$X, [int]$Y) [PetProbe]::SetCursorPos($X, $Y) | Out-Null }
function Invoke-Click { param([int]$X, [int]$Y) [PetProbe]::Click($X, $Y) }
function Invoke-RightClick { param([int]$X, [int]$Y) [PetProbe]::RightClick($X, $Y) }
function Invoke-PetDrag {
    param([int]$FromX, [int]$FromY, [int]$ToX, [int]$ToY, [int]$Steps = 8)
    [PetProbe]::Drag($FromX, $FromY, $ToX, $ToY, $Steps)
}
function Save-ScreenRegion {
    param([int]$X, [int]$Y, [int]$Width, [int]$Height, [string]$Path)
    $bmp = New-Object System.Drawing.Bitmap($Width, $Height)
    $graphics = [System.Drawing.Graphics]::FromImage($bmp)
    $graphics.CopyFromScreen($X, $Y, 0, 0, (New-Object System.Drawing.Size($Width, $Height)))
    $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    $graphics.Dispose(); $bmp.Dispose()
    $Path
}
