# Netray Windows PowerShell Launcher with UAC Elevation Check
[CmdletBinding()]
param (
    [switch]$NoElevation
)

$ErrorActionPreference = "Stop"

function Test-IsAdmin {
    $currentIdentity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($currentIdentity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

$isAdmin = Test-IsAdmin
if (-not $isAdmin -and -not $NoElevation) {
    Write-Host "[Netray] Elevating to Administrator permissions for WinTun / Proxy support..." -ForegroundColor Cyan
    Start-Process -FilePath "powershell.exe" -ArgumentList "-NoProfile -ExecutionPolicy Bypass -File `"$PSCommandPath`"" -Verb RunAs
    exit
}

Write-Host "==========================================" -ForegroundColor Green
Write-Host "         Netray Windows Launcher          " -ForegroundColor Green
Write-Host "==========================================" -ForegroundColor Green

$rootDir = Split-Path -Parent $PSScriptRoot
$releaseExe = Join-Path $rootDir "target\release\netray-windows.exe"
$debugExe = Join-Path $rootDir "target\debug\netray-windows.exe"

if (Test-Path $releaseExe) {
    Write-Host "[Netray] Launching Release build: $releaseExe" -ForegroundColor Cyan
    Start-Process -FilePath $releaseExe
} elseif (Test-Path $debugExe) {
    Write-Host "[Netray] Launching Debug build: $debugExe" -ForegroundColor Cyan
    Start-Process -FilePath $debugExe
} else {
    Write-Host "[Netray] Executable not found. Running with cargo..." -ForegroundColor Yellow
    Push-Location $rootDir
    cargo run --release
    Pop-Location
}
