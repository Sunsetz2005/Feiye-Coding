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

function Find-NamedElement {
  param(
    [Parameter(Mandatory = $true)]
    [System.Windows.Automation.AutomationElement]$Root,

    [Parameter(Mandatory = $true)]
    [string[]]$Names,

    [int]$TimeoutSeconds = 20
  )

  $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
  do {
    foreach ($name in $Names) {
      $condition = [System.Windows.Automation.PropertyCondition]::new(
        [System.Windows.Automation.AutomationElement]::NameProperty,
        $name
      )
      $element = $Root.FindFirst(
        [System.Windows.Automation.TreeScope]::Descendants,
        $condition
      )
      if ($null -ne $element -and -not $element.Current.IsOffscreen) {
        return $element
      }
    }
    Start-Sleep -Milliseconds 250
  } while ([DateTime]::UtcNow -lt $deadline)

  throw "Timed out waiting for UI element: $($Names -join ' | ')"
}

function Wait-NamedElementAbsent {
  param(
    [Parameter(Mandatory = $true)]
    [System.Windows.Automation.AutomationElement]$Root,

    [Parameter(Mandatory = $true)]
    [string[]]$Names,

    [int]$TimeoutSeconds = 10
  )

  $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
  do {
    $visible = $false
    foreach ($name in $Names) {
      $condition = [System.Windows.Automation.PropertyCondition]::new(
        [System.Windows.Automation.AutomationElement]::NameProperty,
        $name
      )
      $element = $Root.FindFirst(
        [System.Windows.Automation.TreeScope]::Descendants,
        $condition
      )
      if ($null -ne $element -and -not $element.Current.IsOffscreen) {
        $visible = $true
        break
      }
    }
    if (-not $visible) {
      return
    }
    Start-Sleep -Milliseconds 200
  } while ([DateTime]::UtcNow -lt $deadline)

  throw "UI element remained visible: $($Names -join ' | ')"
}

function Invoke-Element {
  param(
    [Parameter(Mandatory = $true)]
    [System.Windows.Automation.AutomationElement]$Element
  )

  $pattern = $null
  if ($Element.TryGetCurrentPattern(
      [System.Windows.Automation.InvokePattern]::Pattern,
      [ref]$pattern
    )) {
    ([System.Windows.Automation.InvokePattern]$pattern).Invoke()
    return
  }
  if ($Element.TryGetCurrentPattern(
      [System.Windows.Automation.TogglePattern]::Pattern,
      [ref]$pattern
    )) {
    ([System.Windows.Automation.TogglePattern]$pattern).Toggle()
    return
  }
  throw "Element '$($Element.Current.Name)' exposes no invoke or toggle pattern"
}

function Wait-FocusedName {
  param(
    [Parameter(Mandatory = $true)]
    [string[]]$Names,

    [int]$TimeoutSeconds = 10
  )

  $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
  do {
    $focused = [System.Windows.Automation.AutomationElement]::FocusedElement
    if ($null -ne $focused -and $Names -contains $focused.Current.Name) {
      return
    }
    Start-Sleep -Milliseconds 200
  } while ([DateTime]::UtcNow -lt $deadline)

  $actual = [System.Windows.Automation.AutomationElement]::FocusedElement
  $actualName = if ($null -eq $actual) { "<none>" } else { $actual.Current.Name }
  throw "Focus was '$actualName'; expected one of: $($Names -join ' | ')"
}

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
$fakeRuntime = Join-Path $dataRoot "grok.cmd"
Set-Content -Path $fakeRuntime -Encoding Ascii -Value "@echo off`r`necho sunsetz-runtime 1.0.0"

$settings = @{
  theme = "light"
  locale = "en"
  sessionDataMode = "independent"
  manualCliPath = $fakeRuntime
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

$env:SUNSETZ_HOME = $dataRoot
$env:SUNSETZ_ACP = "mock"
$process = Start-Process -FilePath $resolvedExecutable -PassThru

try {
  $root = [System.Windows.Automation.AutomationElement]::RootElement
  $windowCondition = [System.Windows.Automation.PropertyCondition]::new(
    [System.Windows.Automation.AutomationElement]::ProcessIdProperty,
    $process.Id
  )
  $deadline = [DateTime]::UtcNow.AddSeconds(30)
  do {
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

  Find-NamedElement -Root $window -Names @("New session", "新建会话") | Out-Null

  $resourceToggle = Find-NamedElement -Root $window -Names @(
    "Show files",
    "显示文件"
  )
  Invoke-Element $resourceToggle
  Find-NamedElement -Root $window -Names @("Hide files", "隐藏文件") | Out-Null
  Find-NamedElement -Root $window -Names @(
    "No file open",
    "尚未打开文件"
  ) | Out-Null
  Save-DesktopScreenshot -Path (Join-Path $OutputDirectory "resources-open.png")

  $resourceToggle = Find-NamedElement -Root $window -Names @(
    "Hide files",
    "隐藏文件"
  )
  Invoke-Element $resourceToggle
  Wait-NamedElementAbsent -Root $window -Names @(
    "No file open",
    "尚未打开文件"
  )
  Wait-FocusedName -Names @("Show files", "显示文件")

  $sidebarToggle = Find-NamedElement -Root $window -Names @(
    "Hide sidebar",
    "隐藏侧栏"
  )
  Invoke-Element $sidebarToggle
  Find-NamedElement -Root $window -Names @(
    "Show sidebar",
    "显示侧栏"
  ) | Out-Null
  Wait-NamedElementAbsent -Root $window -Names @(
    "New session",
    "新建会话"
  )
  Wait-FocusedName -Names @("Show sidebar", "显示侧栏")

  Write-Host "Windows native smoke passed at $($bounds.Width)x$($bounds.Height)."
}
finally {
  if (-not $process.HasExited) {
    Stop-Process -Id $process.Id -Force
  }
}
