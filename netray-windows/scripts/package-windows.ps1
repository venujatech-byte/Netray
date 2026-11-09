param([string]$Version = "1.0.0")

$ErrorActionPreference = "Stop"
if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "Version must use MAJOR.MINOR.PATCH format." }

$xrayVersion = "v26.10.21"
$asset = "Xray-windows-64.zip"
$releaseUrl = "https://github.com/XTLS/Xray-core/releases/download/$xrayVersion"
$work = Join-Path ([IO.Path]::GetTempPath()) ("netray-xray-" + [guid]::NewGuid())
$package = Join-Path $PSScriptRoot "..\dist\netray-windows"
$exe = Join-Path $PSScriptRoot "..\target\release\netray-windows.exe"

function Get-ReleaseAsset([string]$Url, [string]$Path) {
    for ($attempt = 1; $attempt -le 5; $attempt++) {
        try {
            Invoke-WebRequest -Uri $Url -OutFile $Path -TimeoutSec 180
            return
        } catch {
            Remove-Item $Path -Force -ErrorAction SilentlyContinue
            if ($attempt -eq 5) { throw }
            Write-Warning "Download failed (attempt $attempt/5); retrying: $($_.Exception.Message)"
            Start-Sleep -Seconds ([Math]::Min(2 * $attempt, 10))
        }
    }
}

try {
    if (-not (Test-Path $exe -PathType Leaf)) { throw "Netray executable not found: $exe" }
    New-Item -ItemType Directory -Path $work, $package -Force | Out-Null
    $zip = Join-Path $work $asset
    $dgst = Join-Path $work "$asset.dgst"
    Get-ReleaseAsset "$releaseUrl/$asset" $zip
    Get-ReleaseAsset "$releaseUrl/$asset.dgst" $dgst

    $checksumText = Get-Content $dgst -Raw
    $match = [regex]::Match($checksumText, '(?im)^\s*sha(?:2-)?256\s*=\s*([0-9a-f]{64})\s*$|^\s*SHA(?:2-)?256\s+\([^)]*\)\s*=\s*([0-9a-f]{64})\s*$')
    if (-not $match.Success) { throw "Xray release checksum is missing or invalid." }
    $expected = if ($match.Groups[1].Success) { $match.Groups[1].Value } else { $match.Groups[2].Value }
    if ((Get-FileHash $zip -Algorithm SHA256).Hash -ne $expected.ToUpperInvariant()) {
        throw "Xray release checksum does not match."
    }

    $extract = Join-Path $work "extract"
    Expand-Archive -Path $zip -DestinationPath $extract
    Copy-Item (Join-Path $extract "*") -Destination $package -Recurse -Force
    python (Join-Path $PSScriptRoot "..\..\scripts\build-xray.py") (Join-Path $package "xray.exe")
    if ($LASTEXITCODE -ne 0) { throw "Patched Xray build failed." }
    Copy-Item $exe -Destination $package -Force
    Remove-Item (Join-Path $package "config.json") -Force -ErrorAction SilentlyContinue
    Remove-Item (Join-Path $package "netray_data") -Recurse -Force -ErrorAction SilentlyContinue
    Set-Content (Join-Path $package "THIRD-PARTY-NOTICES.txt") @"
Netray bundles Xray-core a4a3dff5b20c with a Netray TLS compatibility modification.
License: Mozilla Public License 2.0; see the accompanying LICENSE file.
Corresponding modified source: XRAY-SOURCE.tar.gz; patch: patch-xray-tls.py
Upstream source: https://github.com/XTLS/Xray-core/tree/a4a3dff5b20c
Upstream dependency list: https://github.com/XTLS/Xray-core/blob/a4a3dff5b20c/go.mod
"@

    $archive = Join-Path (Split-Path $package -Parent) "netray-windows-$Version.zip"
    Compress-Archive -Path (Join-Path $package "*") -DestinationPath $archive -Force
    Write-Host "Portable release package: $archive"
} finally {
    Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
}
