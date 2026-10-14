[CmdletBinding()]
param(
    [string]$Executable = '',
    [string]$OutputDirectory = '',
    [switch]$MeasureIdle
)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
if (!$Executable) { $Executable = Join-Path $root 'target\x86_64-pc-windows-msvc\release\CalendarRust.exe' }
$Executable = (Resolve-Path -LiteralPath $Executable).Path
$resources = Split-Path $Executable -Parent
if (!(Test-Path -LiteralPath (Join-Path $resources 'assets\calendar-data\manifest.json'))) { $resources = $root }
if (!$OutputDirectory) { $OutputDirectory = Join-Path $root "target\gui-smoke-$([guid]::NewGuid().ToString('N'))" }
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$user = Join-Path $OutputDirectory 'isolated-user'

function Start-Calendar([string[]]$Extra) {
    $arguments = @('--resources', $resources, '--data-dir', $user) + $Extra
    $quoted = ($arguments | ForEach-Object { '"' + $_ + '"' }) -join ' '
    Start-Process -FilePath $Executable -ArgumentList $quoted -WorkingDirectory $env:TEMP -PassThru
}

function Close-Calendar($Process, $Window) {
    if (!$Process.HasExited) {
        $owner = [uint32]0
        [CalendarRustSmokeNative]::GetWindowThreadProcessId($Window, [ref]$owner) | Out-Null
        if ($owner -ne $Process.Id) {
            Stop-Process -Id $Process.Id
            throw 'Refusing to post a message to a window not owned by the test process'
        }
        if (![CalendarRustSmokeNative]::PostMessage($Window, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)) {
            Stop-Process -Id $Process.Id
            throw 'Unable to request normal application shutdown'
        }
        if (!$Process.WaitForExit(15000)) {
            Stop-Process -Id $Process.Id
            throw 'Application failed to save and close normally'
        }
    }
    if ($Process.ExitCode) { throw "GUI exit code: $($Process.ExitCode)" }
}

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class CalendarRustSmokeNative {
    public delegate bool EnumWindow(IntPtr window, IntPtr state);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window, uint message, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindow callback, IntPtr state);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr window, StringBuilder text, int count);
    public static string[] Titles(int process) {
        var result = new List<string>();
        EnumWindows((window, state) => {
            uint owner;
            GetWindowThreadProcessId(window, out owner);
            if (owner == process && IsWindowVisible(window)) {
                var text = new StringBuilder(512);
                GetWindowText(window, text, text.Capacity);
                result.Add(text.ToString());
            }
            return true;
        }, IntPtr.Zero);
        return result.ToArray();
    }
    public static void ClickDetail(IntPtr window, int process) {
        uint owner;
        GetWindowThreadProcessId(window, out owner);
        if (owner != process) throw new InvalidOperationException("Not the test window");
        Rect rect;
        if (!GetClientRect(window, out rect)) throw new InvalidOperationException("No client rectangle");
        int x = (rect.Right - rect.Left) * 520 / 840;
        int y = (rect.Bottom - rect.Top) * 208 / 664;
        var point = new IntPtr((y << 16) | x);
        if (!PostMessage(window, 0x0200, IntPtr.Zero, point) ||
            !PostMessage(window, 0x0201, new IntPtr(1), point) ||
            !PostMessage(window, 0x0202, IntPtr.Zero, point))
            throw new InvalidOperationException("Unable to post test input");
    }
}
'@

foreach ($view in @('month', 'day', 'export')) {
    $image = Join-Path $OutputDirectory "CalendarRust-$view.png"
    $process = Start-Calendar -Extra @('--fixed-time','2026-10-14T15:34:00+08:00','--selected-date','2026-10-10','--view',$view,'--screenshot',$image)
    if (!$process.WaitForExit(30000)) {
        Stop-Process -Id $process.Id
        throw "Screenshot timed out: $view"
    }
    if ($process.ExitCode -or !(Test-Path -LiteralPath $image)) { throw "Screenshot failed: $view" }
}

$window = [IntPtr]::Zero
$process = Start-Calendar -Extra @('--fixed-time','2026-10-14T15:34:00+08:00','--selected-date','2026-10-10','--view','day')
try {
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    do {
        Start-Sleep -Milliseconds 100
        $process.Refresh()
        $window = $process.MainWindowHandle
    } while (!$window -and !$process.HasExited -and [DateTime]::UtcNow -lt $deadline)
    if (!$window -or $process.HasExited) { throw 'No native main window' }
    Start-Sleep -Seconds 2
    $process.Refresh()
    $window=$process.MainWindowHandle
    if (!([CalendarRustSmokeNative]::Titles($process.Id) | Where-Object { $_ -match '2026-10-14' })) {
        throw 'Native caption did not retain the real today'
    }
    [CalendarRustSmokeNative]::ClickDetail($window, $process.Id)
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    do {
        Start-Sleep -Milliseconds 200
        $titles = [CalendarRustSmokeNative]::Titles($process.Id)
    } while (!($titles | Where-Object { $_ -match '2026-10-10' }) -and [DateTime]::UtcNow -lt $deadline)
    if ($titles.Count -lt 2 -or !($titles | Where-Object { $_ -match '2026-10-10' })) {
        throw 'Selected-date almanac did not open as a separate native window'
    }
} finally {
    if (!$process.HasExited) {
        if ($window) { Close-Calendar $process $window } else { Stop-Process -Id $process.Id }
    }
}

if ($MeasureIdle) {
    $window = [IntPtr]::Zero
    $process = Start-Calendar -Extra @()
    try {
        Start-Sleep -Seconds 3
        $process.Refresh()
        if ($process.HasExited -or !$process.MainWindowHandle -or !$process.Responding) { throw 'Idle GUI not ready' }
        $window = $process.MainWindowHandle
        $cpu = $process.TotalProcessorTime.TotalSeconds
        $timer = [Diagnostics.Stopwatch]::StartNew()
        $maximum = 0L
        for ($sample = 0; $sample -lt 60; $sample++) {
            Start-Sleep -Seconds 1
            $process.Refresh()
            if ($process.HasExited -or !$process.Responding) { throw 'Idle GUI stopped responding' }
            $maximum = [Math]::Max($maximum, $process.WorkingSet64)
        }
        $report = @{
            elapsed_seconds = $timer.Elapsed.TotalSeconds
            logical_processors = [Environment]::ProcessorCount
            whole_machine_cpu_percent = 100 * ($process.TotalProcessorTime.TotalSeconds - $cpu) / $timer.Elapsed.TotalSeconds / [Environment]::ProcessorCount
            maximum_working_set_mib = $maximum / 1MB
            clock = 'real system clock'
            note = 'Current machine, not a substitute for the specified reference machine or mixed-DPI acceptance.'
        }
        [IO.File]::WriteAllText((Join-Path $OutputDirectory 'idle-metrics.json'), ($report | ConvertTo-Json), [Text.UTF8Encoding]::new($false))
        if ($report.whole_machine_cpu_percent -gt 1 -or $report.maximum_working_set_mib -gt 200) {
            throw 'Current-machine idle CPU or memory threshold exceeded; inspect idle-metrics.json'
        }
    } finally {
        if (!$process.HasExited) {
            if ($window) { Close-Calendar $process $window } else { Stop-Process -Id $process.Id }
        }
    }
}
Write-Output "Native GUI smoke artifacts: $OutputDirectory"
