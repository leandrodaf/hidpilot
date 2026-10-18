# Installs the Flow agent on Windows: hidpilot-flow.exe, started hidden at login, plus the
# AudioDeviceCmdlets module so the headset's sound can follow the mouse.
# Windows needs no driver for the Logi Bolt receiver or the Zone Vibe.
#
#   powershell -ExecutionPolicy Bypass -File scripts\install-windows.ps1            # build from this checkout
#   powershell -ExecutionPolicy Bypass -File scripts\install-windows.ps1 -Exe C:\path\hidpilot-flow.exe
#   powershell -ExecutionPolicy Bypass -File scripts\install-windows.ps1 -Uninstall
param(
    [string]$Exe,
    [switch]$Uninstall
)
$ErrorActionPreference = "Stop"

$Dir = Join-Path $env:LOCALAPPDATA "hidpilot"
$Target = Join-Path $Dir "hidpilot-flow.exe"
$Startup = Join-Path ([Environment]::GetFolderPath("Startup")) "hidpilot-flow.vbs"
$Shortcut = Join-Path ([Environment]::GetFolderPath("Programs")) "Orbit Flow.lnk"
$Icon = Join-Path $Dir "orbit.ico"
$Repo = Split-Path -Parent $PSScriptRoot

function Stop-Flow { Get-Process hidpilot-flow -ErrorAction SilentlyContinue | Stop-Process -Force }

if ($Uninstall) {
    Stop-Flow
    Remove-Item -Force -ErrorAction SilentlyContinue $Startup, $Shortcut, $Target, $Icon
    Write-Host "hidpilot-flow removed (pairings kept in $env:APPDATA\hidpilot)."
    exit 0
}

Write-Host "hidpilot will install on this PC:" -ForegroundColor Cyan
Write-Host "  - hidpilot-flow in $Dir, started at login (hidden)"
Write-Host "  - 'Orbit Flow' in the Start menu: opens the Flow page"
Write-Host "  - AudioDeviceCmdlets (PowerShell Gallery), so the headset's sound can move with the mouse"
Write-Host ""

if (-not $Exe) {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Write-Host "Rust is needed to build: install it from https://rustup.rs (with the Visual Studio C++ build tools it offers)," -ForegroundColor Yellow
        Write-Host "or download hidpilot-flow.exe from https://github.com/leandrodaf/hidpilot/releases/latest and pass it with -Exe." -ForegroundColor Yellow
        exit 1
    }
    Write-Host "Building (a few minutes the first time)..."
    Push-Location $Repo
    try { cargo build --release -p hidpilot-flow; if ($LASTEXITCODE) { throw "build failed" } } finally { Pop-Location }
    $Exe = Join-Path $Repo "target\release\hidpilot-flow.exe"
}

Stop-Flow
New-Item -ItemType Directory -Force $Dir | Out-Null
Copy-Item -Force $Exe $Target
Copy-Item -Force (Join-Path $Repo "packaging\icons\hidpilot.ico") $Icon -ErrorAction SilentlyContinue

# A Start menu entry so the Flow page has a visible place; it just runs "hidpilot-flow ui".
$Shell = New-Object -ComObject WScript.Shell
$Lnk = $Shell.CreateShortcut($Shortcut)
$Lnk.TargetPath = $Target
$Lnk.Arguments = "ui"
$Lnk.WorkingDirectory = $Dir
$Lnk.Description = "Orbit Flow: mouse, keyboard and sound across your computers"
if (Test-Path $Icon) { $Lnk.IconLocation = "$Icon,0" }
$Lnk.Save()

if (-not (Get-Module -ListAvailable AudioDeviceCmdlets)) {
    Install-PackageProvider -Name NuGet -MinimumVersion 2.8.5.201 -Scope CurrentUser -Force | Out-Null
    Install-Module AudioDeviceCmdlets -Scope CurrentUser -Force
}

# Creates the identity and settings on first run.
& $Target peers | Out-Null

# A hidden launcher in the Startup folder: the agent runs in your session (it moves the
# pointer and reads the clipboard) without a console window.
"CreateObject(""WScript.Shell"").Run """"""$Target"""" run"", 0, False" | Set-Content -Encoding ASCII $Startup
Start-Process wscript.exe -ArgumentList "`"$Startup`""
Start-Sleep -Seconds 2

if (Get-Process hidpilot-flow -ErrorAction SilentlyContinue) {
    Write-Host "Done: hidpilot-flow is running and starts with your login." -ForegroundColor Green
} else {
    Write-Host "hidpilot-flow did not start; run `"$Target`" run to see why." -ForegroundColor Red
    exit 1
}

Write-Host @"

The Flow page is opening in your browser: pick the Linux computer under "On this network",
click where its screen is, and type the code it shows. That's it.
(Reopen it any time: "Orbit Flow" in the Start menu, or: "$Target" ui)

  - When Windows asks, allow hidpilot-flow on private networks.
  - For the sound to come along, pair the Zone Vibe with this PC over Bluetooth once.
"@
& $Target ui
