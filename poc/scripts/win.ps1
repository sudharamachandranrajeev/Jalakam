# Window helpers for scripted preview tests (resize, screenshot, close).
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class JkWin {
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int cx, int cy, uint f);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, System.Text.StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint f);
  public struct RECT { public int L, T, R, B; }
  public struct POINT { public int X, Y; }
}
"@
[void][JkWin]::SetProcessDPIAware()
function Get-JkTitle($h) { $sb = New-Object System.Text.StringBuilder 1024; [void][JkWin]::GetWindowText($h, $sb, 1024); $sb.ToString() }
function Set-JkSize($h, $w, $ht) { [void][JkWin]::SetWindowPos($h, [IntPtr]::Zero, 100, 100, $w, $ht, 0x0004) }
function Save-JkClient($h, $path) {
  # PrintWindow with PW_CLIENTONLY|PW_RENDERFULLCONTENT captures the window's
  # own flip-model content even when other windows cover it.
  $r = New-Object JkWin+RECT; [void][JkWin]::GetClientRect($h, [ref]$r)
  $bmp = New-Object System.Drawing.Bitmap ($r.R), ($r.B)
  $g = [System.Drawing.Graphics]::FromImage($bmp); $hdc = $g.GetHdc()
  [void][JkWin]::PrintWindow($h, $hdc, 3); $g.ReleaseHdc($hdc); $g.Dispose()
  $bmp.Save($path); $bmp.Dispose(); "$($r.R)x$($r.B)"
}
function Send-JkKey($h, [char]$k) { [void][JkWin]::PostMessage($h, 0x0100, [IntPtr][int]$k, [IntPtr]0) }
function Close-JkWin($h) { [void][JkWin]::PostMessage($h, 0x0010, [IntPtr]0, [IntPtr]0) }
