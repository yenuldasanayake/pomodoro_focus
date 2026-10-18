$ErrorActionPreference = "Stop"
$root = $PSScriptRoot

Write-Host "Building release..." -ForegroundColor Cyan
Push-Location $root
try { cargo build --release } finally { Pop-Location }

Write-Host "Compiling installer..." -ForegroundColor Cyan
$iscc = "C:\Program Files\Inno Setup 7\ISIDE.exe"
if (-not (Test-Path $iscc)) {
    throw "Inno Setup not found at $iscc. Install it from https://jrsoftware.org/isdl.php"
}
& $iscc "$root\installer.iss"

Write-Host "Done." -ForegroundColor Green
Write-Host "Installer: $root\Output\PomodoroFocus-Setup-1.0.0.exe"