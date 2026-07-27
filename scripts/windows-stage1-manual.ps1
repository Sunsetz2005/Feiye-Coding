param(
  [string]$Executable,
  [string]$EvidenceDirectory = (
    Join-Path (Get-Location) (
      "test-results/windows-stage1-manual-{0}" -f (Get-Date -Format "yyyyMMdd-HHmmss")
    )
  ),
  [switch]$ValidateOnly
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if ($env:CI -and -not $ValidateOnly) {
  throw "This checklist requires an interactive physical Windows session."
}

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System.Runtime.InteropServices;
public static class SunsetzDpi {
  [DllImport("user32.dll")]
  public static extern uint GetDpiForSystem();
}
"@

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

function Read-Result {
  while ($true) {
    $answer = (Read-Host "Result [PASS/FAIL]").Trim().ToUpperInvariant()
    if ($answer -eq "PASS" -or $answer -eq "FAIL") {
      return $answer
    }
    Write-Host "Enter PASS or FAIL." -ForegroundColor Yellow
  }
}

function Escape-MarkdownCell {
  param([AllowEmptyString()][string]$Value)
  return ($Value -replace "\r?\n", " " -replace "\|", "\|")
}

$checks = @(
  @{
    id = "display"
    title = "Display configuration evidence"
    instructions = @(
      "Open Windows Settings > System > Display on the monitor used for Sunsetz.",
      "Keep the Scale control and its 200% value visible for the evidence screenshot.",
      "Confirm Sunsetz will be tested on this same monitor."
    )
  },
  @{
    id = "scale"
    title = "200% scale and work-area fit"
    instructions = @(
      "Open an empty Sunsetz task and keep the window restored, not maximized.",
      "Confirm the complete titlebar, sidebar, composer, and window controls remain inside the work area.",
      "Confirm there is no horizontal scrollbar or overlap with the taskbar."
    )
  },
  @{
    id = "keyboard"
    title = "Keyboard path and visible focus"
    instructions = @(
      "Use only Tab, Shift+Tab, Enter, Space, and Escape.",
      "Traverse the composer, topbar, account entry, task rows, project rows, and main navigation.",
      "Confirm every focused control has a visible focus indicator and no hidden control receives focus.",
      "Confirm project disclosure responds to both Enter and Space."
    )
  },
  @{
    id = "resources"
    title = "Resource panel lifecycle"
    instructions = @(
      "Open the resource panel from the topbar and confirm its contents are visible.",
      "Close it using the panel close button.",
      "Confirm focus returns to Show files and hidden resource controls cannot be reached with Tab.",
      "Reopen it once to confirm the trigger remains usable."
    )
  },
  @{
    id = "sidebar"
    title = "Sidebar lifecycle"
    instructions = @(
      "Hide the sidebar using the topbar control.",
      "Confirm focus returns to Show sidebar and hidden sidebar controls cannot be reached with Tab.",
      "Press Space on Show sidebar and confirm the sidebar reopens."
    )
  },
  @{
    id = "responsive"
    title = "Narrow-window overlay and scroll retention"
    instructions = @(
      "Resize the restored window to its minimum usable size.",
      "Open the resource panel and confirm it overlays without trapping focus behind the panel.",
      "Scroll the conversation, close and reopen the panel, and confirm the conversation position is retained.",
      "Confirm the composer remains visible and the layout has no horizontal overflow."
    )
  },
  @{
    id = "themes"
    title = "Theme and project-selection states"
    instructions = @(
      "Repeat the empty workbench check in light, dark, and high-contrast themes.",
      "Confirm text, focus indicators, window controls, and panel boundaries remain readable.",
      "Confirm selected projects use a neutral row background with no coral or orange left border."
    )
  }
)

if ($ValidateOnly) {
  [void][SunsetzDpi]::GetDpiForSystem()
  [void][System.Windows.Forms.Screen]::AllScreens
  Write-Host "Windows stage 1 manual acceptance prerequisites are available."
  exit 0
}

if ([string]::IsNullOrWhiteSpace($Executable) -or -not (Test-Path -PathType Leaf $Executable)) {
  throw "Pass -Executable with the Sunsetz executable being tested."
}
$resolvedExecutable = (Resolve-Path $Executable).Path
$executableItem = Get-Item $resolvedExecutable
$executableHash = (Get-FileHash -Algorithm SHA256 $resolvedExecutable).Hash

Write-Host ""
Write-Host "Sunsetz stage 1 physical Windows acceptance" -ForegroundColor Cyan
Write-Host "Screenshots may contain private information. Close unrelated windows and use an empty task."
$safeToCapture = (Read-Host "Ready to capture a sanitized desktop? [YES]").Trim()
if ($safeToCapture -ne "YES") {
  throw "Acceptance cancelled before capturing evidence."
}

New-Item -ItemType Directory -Force -Path $EvidenceDirectory | Out-Null
$resolvedEvidenceDirectory = (Resolve-Path $EvidenceDirectory).Path

$screens = @(
  [System.Windows.Forms.Screen]::AllScreens | ForEach-Object {
    [ordered]@{
      deviceName = $_.DeviceName
      primary = $_.Primary
      bounds = "$($_.Bounds.X),$($_.Bounds.Y),$($_.Bounds.Width),$($_.Bounds.Height)"
      workingArea = "$($_.WorkingArea.X),$($_.WorkingArea.Y),$($_.WorkingArea.Width),$($_.WorkingArea.Height)"
    }
  }
)
$systemDpi = [SunsetzDpi]::GetDpiForSystem()
$commit = $null
if ($null -ne (Get-Command git -ErrorAction SilentlyContinue)) {
  $commit = (& git rev-parse HEAD 2>$null)
  if ($LASTEXITCODE -ne 0) {
    $commit = $null
  }
}

$results = @()
foreach ($check in $checks) {
  Write-Host ""
  Write-Host "[$($check.id)] $($check.title)" -ForegroundColor Cyan
  foreach ($instruction in $check.instructions) {
    Write-Host " - $instruction"
  }
  Read-Host "Press Enter after completing the check"

  $screenshotName = "{0}.png" -f $check.id
  Save-DesktopScreenshot -Path (
    Join-Path $resolvedEvidenceDirectory $screenshotName
  )
  $result = Read-Result
  $notes = Read-Host "Notes (required for FAIL, optional for PASS)"
  if ($result -eq "FAIL" -and [string]::IsNullOrWhiteSpace($notes)) {
    $notes = "Failure observed; reproduce while reviewing the attached screenshot."
  }
  $results += [ordered]@{
    id = $check.id
    title = $check.title
    result = $result
    notes = $notes
    screenshot = $screenshotName
  }
}

$manifest = [ordered]@{
  schemaVersion = 1
  generatedAt = (Get-Date).ToString("o")
  os = (Get-CimInstance Win32_OperatingSystem).Caption
  osVersion = [System.Environment]::OSVersion.VersionString
  systemDpi = $systemDpi
  reportedScalePercent = [Math]::Round(($systemDpi / 96.0) * 100)
  gitCommit = $commit
  executable = [ordered]@{
    name = $executableItem.Name
    productVersion = $executableItem.VersionInfo.ProductVersion
    sha256 = $executableHash
  }
  screens = $screens
  results = $results
}
$manifest | ConvertTo-Json -Depth 8 | Set-Content (
  Join-Path $resolvedEvidenceDirectory "manifest.json"
)

$reportLines = @(
  "# Sunsetz stage 1 physical Windows acceptance",
  "",
  "- Generated: $($manifest.generatedAt)",
  "- OS: $($manifest.os)",
  "- System DPI: $systemDpi ($($manifest.reportedScalePercent)%)",
  "- Git commit: $($commit ?? 'unknown')",
  "- Executable: $($manifest.executable.name) $($manifest.executable.productVersion)",
  "- Executable SHA-256: $($manifest.executable.sha256)",
  "",
  "| Check | Result | Notes | Screenshot |",
  "|---|---|---|---|"
)
foreach ($entry in $results) {
  $reportLines += "| $(Escape-MarkdownCell $entry.title) | $($entry.result) | $(Escape-MarkdownCell $entry.notes) | [$($entry.screenshot)]($($entry.screenshot)) |"
}
$reportLines | Set-Content (
  Join-Path $resolvedEvidenceDirectory "report.md"
)

$failures = @($results | Where-Object { $_.result -eq "FAIL" })
Write-Host ""
Write-Host "Evidence: $resolvedEvidenceDirectory"
if ($failures.Count -gt 0) {
  Write-Host "$($failures.Count) check(s) failed." -ForegroundColor Red
  exit 1
}
Write-Host "All physical Windows checks passed." -ForegroundColor Green
