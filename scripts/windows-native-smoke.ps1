param(
  [Parameter(Mandatory = $true)]
  [string]$Executable,

  [Parameter(Mandatory = $true)]
  [string]$OutputDirectory
)

$ErrorActionPreference = "Stop"

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

function Save-DesktopScreenshot {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Path
  )

  $bounds = [System.Windows.Forms.SystemInformation]::VirtualScreen
  $bitmap = [System.Drawing.Bitmap]::new($bounds.Width, $bounds.Height)
  $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
  try {
    $graphics.CopyFromScreen(
      $bounds.Left,
      $bounds.Top,
      0,
      0,
      $bitmap.Size
    )
    $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
  }
  finally {
    $graphics.Dispose()
    $bitmap.Dispose()
  }
}

$resolvedExecutable = (Resolve-Path $Executable).Path
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null

$dataRoot = Join-Path $env:RUNNER_TEMP "sunsetz-native-smoke"
New-Item -ItemType Directory -Force -Path $dataRoot | Out-Null
$probeRuntimeDirectory = Join-Path $env:USERPROFILE ".grok\bin"
New-Item -ItemType Directory -Force -Path $probeRuntimeDirectory | Out-Null
$probeRuntime = Join-Path $probeRuntimeDirectory "grok.cmd"
@"
@echo off
echo grok native-smoke
"@ | Set-Content -Path $probeRuntime

$settings = @{
  theme = "light"
  locale = "en"
  sessionDataMode = "independent"
  manualCliPath = $probeRuntime
  permissionPolicy = "ask"
  modelId = $null
  effort = "medium"
  mode = "agent"
  onboardingDone = $true
  setupSkipped = $true
  setupWizardCompleted = $true
  authSetupDeferred = $true
  defaultOpenTarget = "explorer"
  composerPrefsScope = "global"
  acpServerAddr = $null
  maxConcurrentAgents = 3
  agentIdleMinutes = 30
  streamStallSeconds = 120
  storeApiKeysInKeychain = $false
}
$settings | ConvertTo-Json | Set-Content -Path (Join-Path $dataRoot "settings.json")

$stdoutPath = Join-Path $OutputDirectory "sunsetz.stdout.log"
$stderrPath = Join-Path $OutputDirectory "sunsetz.stderr.log"
$startInfo = [System.Diagnostics.ProcessStartInfo]::new()
$startInfo.FileName = $resolvedExecutable
$startInfo.UseShellExecute = $false
$startInfo.RedirectStandardOutput = $true
$startInfo.RedirectStandardError = $true
$startInfo.Environment["SUNSETZ_HOME"] = $dataRoot
$startInfo.Environment["SUNSETZ_ACP"] = "mock"

$process = [System.Diagnostics.Process]::new()
$process.StartInfo = $startInfo
if (-not $process.Start()) {
  throw "Failed to start Sunsetz"
}
$stdoutTask = $process.StandardOutput.ReadToEndAsync()
$stderrTask = $process.StandardError.ReadToEndAsync()

try {
  $root = [System.Windows.Automation.AutomationElement]::RootElement
  $windowCondition = [System.Windows.Automation.PropertyCondition]::new(
    [System.Windows.Automation.AutomationElement]::ProcessIdProperty,
    $process.Id
  )
  $deadline = [DateTime]::UtcNow.AddSeconds(30)
  do {
    if ($process.HasExited) {
      throw "Sunsetz exited before exposing its native window"
    }
    $window = $root.FindFirst(
      [System.Windows.Automation.TreeScope]::Children,
      $windowCondition
    )
    if ($null -ne $window) {
      break
    }
    Start-Sleep -Milliseconds 250
  } while ([DateTime]::UtcNow -lt $deadline)
  if ($null -eq $window) {
    throw "Sunsetz did not expose a native window within 30 seconds"
  }

  $bounds = $window.Current.BoundingRectangle
  if ($bounds.Width -lt 900 -or $bounds.Height -lt 600) {
    throw "Unexpected native window size: $($bounds.Width)x$($bounds.Height)"
  }
  $nativeHandle = [System.IntPtr]::new($window.Current.NativeWindowHandle)
  $workingArea = [System.Windows.Forms.Screen]::FromHandle($nativeHandle).WorkingArea
  if (
    $bounds.Left -lt $workingArea.Left -or
    $bounds.Top -lt $workingArea.Top -or
    $bounds.Right -gt $workingArea.Right -or
    $bounds.Bottom -gt $workingArea.Bottom
  ) {
    throw "Native window $bounds exceeded monitor work area $workingArea"
  }

  & node ./scripts/windows-native-webview-smoke.mjs $OutputDirectory
  if ($LASTEXITCODE -ne 0) {
    throw "Native WebView smoke failed with exit code $LASTEXITCODE"
  }

  Save-DesktopScreenshot -Path (Join-Path $OutputDirectory "native-window.png")
  Write-Host "Windows native smoke passed at $($bounds.Width)x$($bounds.Height)."
}
catch {
  Save-DesktopScreenshot -Path (Join-Path $OutputDirectory "native-window-failure.png")
  throw
}
finally {
  if (-not $process.HasExited) {
    Stop-Process -Id $process.Id -Force
    $process.WaitForExit()
  }
  $stdoutTask.GetAwaiter().GetResult() | Set-Content -Path $stdoutPath
  $stderrTask.GetAwaiter().GetResult() | Set-Content -Path $stderrPath
  $process.Dispose()
}
