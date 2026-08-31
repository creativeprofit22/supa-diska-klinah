$ErrorActionPreference = "Stop"
$tempDirectory = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid())
New-Item -ItemType Directory -Path $tempDirectory | Out-Null

function Invoke-NativeSmoke {
  param([string]$Mode)

  $env:SMOKE_FIXTURE_MODE = $Mode
  try {
    $output = & "$PSScriptRoot/smoke-native.ps1" -Target x86_64-pc-windows-msvc -Directory $tempDirectory -LaunchOnly
    return @{ Error = $null; Output = ($output -join "`n") }
  }
  catch {
    return @{ Error = $_.Exception.Message; Output = $null }
  }
  finally {
    Remove-Item Env:SMOKE_FIXTURE_MODE -ErrorAction SilentlyContinue
  }
}

try {
  $exe = Join-Path $tempDirectory "supa-diska-klinah.exe"
  Add-Type -TypeDefinition @"
using System;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;

public static class NativeSmokeFixture
{
    private delegate IntPtr WindowProcedure(IntPtr window, uint message, IntPtr word, IntPtr longWord);
    private static readonly WindowProcedure Procedure = DefWindowProc;

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct WindowClass
    {
        public uint style;
        public WindowProcedure windowProcedure;
        public int classExtra;
        public int windowExtra;
        public IntPtr instance;
        public IntPtr icon;
        public IntPtr cursor;
        public IntPtr background;
        public string menuName;
        public string className;
    }

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr GetModuleHandle(string moduleName);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern ushort RegisterClass(ref WindowClass windowClass);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr CreateWindowEx(
        uint extendedStyle, string className, string windowName, uint style,
        int x, int y, int width, int height, IntPtr parent, IntPtr menu, IntPtr instance, IntPtr parameter);

    [DllImport("user32.dll")]
    private static extern bool ShowWindow(IntPtr window, int command);

    [DllImport("user32.dll")]
    private static extern IntPtr DefWindowProc(IntPtr window, uint message, IntPtr word, IntPtr longWord);

    private static int HoldWindows(bool mainVisible, bool toolVisible)
    {
        const string className = "NativeSmokeFixtureWindow";
        IntPtr instance = GetModuleHandle(null);
        WindowClass windowClass = new WindowClass
        {
            windowProcedure = Procedure,
            instance = instance,
            className = className
        };
        if (RegisterClass(ref windowClass) == 0)
            return 24;

        uint mainStyle = 0x00CF0000u | (mainVisible ? 0x10000000u : 0u);
        IntPtr mainWindow = CreateWindowEx(0, className, "Native smoke fixture", mainStyle, 0, 0, 320, 200, IntPtr.Zero, IntPtr.Zero, instance, IntPtr.Zero);
        if (mainWindow == IntPtr.Zero)
            return 25;
        if (mainVisible)
        {
            ShowWindow(mainWindow, 5);
            ShowWindow(mainWindow, 5);
        }

        if (toolVisible)
        {
            const uint taoToolWindowExtendedStyle = 0x080800A0u;
            const uint visiblePopupStyle = 0x90000000u;
            IntPtr toolWindow = CreateWindowEx(taoToolWindowExtendedStyle, className, "", visiblePopupStyle, 0, 0, 0, 0, IntPtr.Zero, IntPtr.Zero, instance, IntPtr.Zero);
            if (toolWindow == IntPtr.Zero)
                return 26;
            ShowWindow(toolWindow, 5);
            ShowWindow(toolWindow, 5);
        }

        Thread.Sleep(30000);
        return 0;
    }

    public static int Main(string[] arguments)
    {
        if (arguments.Length == 1 && arguments[0] == "hold-handles")
        {
            Thread.Sleep(30000);
            return 0;
        }

        string mode = Environment.GetEnvironmentVariable("SMOKE_FIXTURE_MODE");
        if (mode == "blocked-visible")
            return HoldWindows(true, false);
        if (mode == "visible-tool")
            return HoldWindows(false, true);
        if (mode == "stable-hidden")
            return HoldWindows(false, false);

        Process child = Process.Start(new ProcessStartInfo
        {
            FileName = Process.GetCurrentProcess().MainModule.FileName,
            Arguments = "hold-handles",
            CreateNoWindow = true,
            UseShellExecute = false
        });
        File.WriteAllText(Environment.GetEnvironmentVariable("SMOKE_DESCENDANT_PID_PATH"), child.Id.ToString());
        Console.Out.WriteLine("fixture stdout");
        Console.Error.WriteLine("fixture stderr");
        return 23;
    }
}
"@ -OutputAssembly $exe -OutputType ConsoleApplication
  New-Item -ItemType File -Path (Join-Path $tempDirectory "supa-diska-klinah-privileged-helper.exe") | Out-Null

  $descendantPidPath = Join-Path $tempDirectory "descendant.pid"
  $env:SMOKE_DESCENDANT_PID_PATH = $descendantPidPath
  $earlyExit = Invoke-NativeSmoke -Mode early-exit
  Remove-Item Env:SMOKE_DESCENDANT_PID_PATH

  $fixtureChildId = [int](Get-Content -Path $descendantPidPath -Raw)
  if (Get-Process -Id $fixtureChildId -ErrorAction SilentlyContinue) {
    throw "Native smoke left descendant process $fixtureChildId holding redirected output handles."
  }

  foreach ($expected in @("code 23", "stdout:`nfixture stdout", "stderr:`nfixture stderr")) {
    if (-not $earlyExit.Error.Contains($expected)) {
      throw "Expected native smoke failure output to contain '$expected', got: $($earlyExit.Error)"
    }
  }

  $visible = Invoke-NativeSmoke -Mode blocked-visible
  if ($visible.Error -ne "Native executable became visible during the smoke window.") {
    throw "Visible normal fixture must fail with the visibility diagnostic, got: $($visible.Error)"
  }

  $tool = Invoke-NativeSmoke -Mode visible-tool
  if ($tool.Error) {
    throw "Visible Tao-style tool window must pass the native smoke, got: $($tool.Error)"
  }

  $hidden = Invoke-NativeSmoke -Mode stable-hidden
  if ($hidden.Error) {
    throw "Hidden fixture must pass the native smoke, got: $($hidden.Error)"
  }

  $smokeSource = Get-Content -Path "$PSScriptRoot/smoke-native.ps1" -Raw
  if (-not $smokeSource.Contains("-WindowStyle Hidden")) {
    throw "Native smoke must prevent the CI desktop from showing its process window."
  }
  if (-not $smokeSource.Contains("taskkill.exe")) {
    throw "A live native process tree must be terminated atomically by Windows."
  }
  if ($smokeSource.IndexOf('$env:SUPA_DISKA_KLINAH_SMOKE_MINIMIZED') -gt $smokeSource.IndexOf('$process = Start-Process')) {
    throw "Native smoke must configure hidden startup before launching the executable."
  }
  $projectSmokeSource = Get-Content -Path "$PSScriptRoot/smoke-project-discovery.ps1" -Raw
  foreach ($required in @("Runtime.evaluate", "Page.captureScreenshot", "requestSubmit", "fixtureBefore", "fixtureAfter", "LocalApplicationData")) {
    if (-not $projectSmokeSource.Contains($required)) {
      throw "Native project discovery smoke is missing required packaged-WebView evidence: $required"
    }
  }

  Write-Output "Native smoke launch guards, process cleanup, WebView interaction, and evidence capture verified."
}
finally {
  if ($fixtureChildId) {
    Stop-Process -Id $fixtureChildId -Force -ErrorAction SilentlyContinue
    Wait-Process -Id $fixtureChildId -ErrorAction SilentlyContinue
  }
  Remove-Item Env:SMOKE_DESCENDANT_PID_PATH -ErrorAction SilentlyContinue
  Remove-Item Env:SMOKE_FIXTURE_MODE -ErrorAction SilentlyContinue
  Remove-Item -Path $tempDirectory -Recurse -Force
}