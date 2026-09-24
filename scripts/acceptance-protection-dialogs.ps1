# Answers the app's own native dialogs for the protection acceptance run.
# Only a #32770 dialog owned by -ProcessId is touched. Controls are found by
# their standard dialog IDs, so this works in any Windows display language:
#   1 = OK / Yes / Select Folder, 2 = Cancel, 6 = Yes, 7 = No, 1152 = picker folder box.
#   -Action Inspect                   report title, text, buttons and the default button
#   -Action Press -ControlId <id>     press one button (BM_CLICK to that exact window)
#   -Action PickFolder -Folder <p>    type a folder into the picker, then press ID 1
param(
  [Parameter(Mandatory)] [int]$ProcessId,
  [Parameter(Mandatory)] [ValidateSet("Inspect", "Press", "PickFolder")] [string]$Action,
  [int]$ControlId,
  [string]$Folder,
  [int]$TimeoutSeconds = 30
)
$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
Add-Type @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class DialogNative {
  public delegate bool EnumProc(IntPtr hwnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr parent, EnumProc cb, IntPtr lParam);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr hwnd, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr hwnd, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern int GetDlgCtrlID(IntPtr hwnd);
  [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr dlg, int id);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr hwnd, uint msg, IntPtr w, string l);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr hwnd, uint msg, IntPtr w, IntPtr l);
  public static string Class(IntPtr h) { var s = new StringBuilder(256); GetClassName(h, s, 256); return s.ToString(); }
  public static string Text(IntPtr h) { var s = new StringBuilder(2048); GetWindowText(h, s, 2048); return s.ToString(); }
  public static IntPtr FindDialog(uint pid) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, _) => {
      uint owner; GetWindowThreadProcessId(h, out owner);
      if (owner == pid && IsWindowVisible(h) && Class(h) == "#32770") { found = h; return false; }
      return true;
    }, IntPtr.Zero);
    return found;
  }
  public static List<IntPtr> Children(IntPtr parent) {
    var list = new List<IntPtr>();
    EnumChildWindows(parent, (h, _) => { list.Add(h); return true; }, IntPtr.Zero);
    return list;
  }
}
"@

$deadline = (Get-Date).AddSeconds($TimeoutSeconds)
$dialog = [IntPtr]::Zero
while ((Get-Date) -lt $deadline) {
  $dialog = [DialogNative]::FindDialog([uint32]$ProcessId)
  if ($dialog -ne [IntPtr]::Zero) { break }
  Start-Sleep -Milliseconds 200
}
if ($dialog -eq [IntPtr]::Zero) { throw "no dialog owned by process $ProcessId within $TimeoutSeconds s" }
Start-Sleep -Milliseconds 400

$DM_GETDEFID = 0x0400
$BM_CLICK = 0x00F5
$WM_SETTEXT = 0x000C
$buttons = @(); $texts = @()
foreach ($child in [DialogNative]::Children($dialog)) {
  $class = [DialogNative]::Class($child); $text = [DialogNative]::Text($child)
  if ($class -eq "Button") { $buttons += [ordered]@{ id = [DialogNative]::GetDlgCtrlID($child); text = $text } }
  elseif ($class -eq "Static" -and $text) { $texts += $text }
}
$defId = [DialogNative]::SendMessage($dialog, $DM_GETDEFID, [IntPtr]::Zero, [IntPtr]::Zero).ToInt64()
$report = [ordered]@{
  utc = (Get-Date).ToUniversalTime().ToString("o")
  title = [DialogNative]::Text($dialog)
  text = $texts
  buttons = $buttons
  # DM_GETDEFID returns DC_HASDEFID (0x534B) in the high word when a default exists.
  defaultButtonId = if (($defId -shr 16) -eq 0x534B) { $defId -band 0xFFFF } else { $null }
  action = $Action
}

function Press([int]$id) {
  $button = [DialogNative]::GetDlgItem($dialog, $id)
  if ($button -eq [IntPtr]::Zero) { throw "control $id not found; buttons: $(($buttons | ForEach-Object { "$($_.id)=$($_.text)" }) -join ', ')" }
  [void][DialogNative]::SendMessage($button, $BM_CLICK, [IntPtr]::Zero, [IntPtr]::Zero)
}

switch ($Action) {
  "Press" { Press $ControlId; $report.pressedId = $ControlId }
  "PickFolder" {
    $box = [DialogNative]::GetDlgItem($dialog, 1152)
    if ($box -eq [IntPtr]::Zero) { throw "folder box (1152) not found" }
    [void][DialogNative]::SendMessage($box, $WM_SETTEXT, [IntPtr]::Zero, $Folder)
    Start-Sleep -Milliseconds 300
    Press 1
    $report.pickedFolder = $Folder
  }
}
$report | ConvertTo-Json -Depth 5
