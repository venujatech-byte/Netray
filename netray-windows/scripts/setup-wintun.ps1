# WinTun Driver Setup Helper for Netray Windows
[CmdletBinding()]
param (
    [string]$DestinationDir = (Get-Location).Path
)

$ErrorActionPreference = "Stop"

Write-Host "==========================================" -ForegroundColor Cyan
Write-Host "     Netray WinTun Driver Setup Helper    " -ForegroundColor Cyan
Write-Host "==========================================" -ForegroundColor Cyan

$wintunZipUrl = "https://www.wintun.net/builds/wintun-0.14.1.zip"
$tempZip = Join-Path $env:TEMP "wintun.zip"
$tempExtract = Join-Path $env:TEMP "wintun-extracted"

Write-Host "[Netray] Downloading official WinTun release from $wintunZipUrl..." -ForegroundColor Green
Invoke-WebRequest -Uri $wintunZipUrl -OutFile $tempZip

Write-Host "[Netray] Extracting WinTun driver package..." -ForegroundColor Green
Expand-Archive -Path $tempZip -DestinationPath $tempExtract -Force

$arch = if ([Environment]::Is64BitOperatingSystem) { "amd64" } else { "x86" }
$sourceDll = Join-Path $tempExtract "wintun\bin\$arch\wintun.dll"
$targetDll = Join-Path $DestinationDir "wintun.dll"

Write-Host "[Netray] Copying $arch wintun.dll to $targetDll..." -ForegroundColor Green
Copy-Item -Path $sourceDll -Destination $targetDll -Force

Remove-Item -Path $tempZip -Force -ErrorAction SilentlyContinue
Remove-Item -Path $tempExtract -Recurse -Force -ErrorAction SilentlyContinue

Write-Host "[Netray] WinTun setup complete! wintun.dll is ready in $DestinationDir" -ForegroundColor Green
