$ErrorActionPreference = "Stop"

$version = "v26.10.21"
$asset = "Xray-windows-64.zip"
$releaseUrl = "https://github.com/XTLS/Xray-core/releases/download/$version"
$work = Join-Path ([IO.Path]::GetTempPath()) ("netray-xray-" + [guid]::NewGuid())
$package = Join-Path $PSScriptRoot "..\dist\netray-windows"
$exe = Join-Path $PSScriptRoot "..\target\release\netray-windows.exe"

try {
    if (-not (Test-Path $exe -PathType Leaf)) { throw "Netray executable not found: $exe" }
    New-Item -ItemType Directory -Path $work, $package -Force | Out-Null
    $zip = Join-Path $work $asset
    $dgst = Join-Path $work "$asset.dgst"
    Invoke-WebRequest "$releaseUrl/$asset" -OutFile $zip
    Invoke-WebRequest "$releaseUrl/$asset.dgst" -OutFile $dgst

    $match = [regex]::Match((Get-Content $dgst -Raw), '(?im)^SHA(?:2-)?256\s+\(.*?\)\s*=\s*([0-9a-f]{64})\s*$')
    if (-not $match.Success) { throw "Xray release checksum is missing or invalid." }
    if ((Get-FileHash $zip -Algorithm SHA256).Hash -ne $match.Groups[1].Value.ToUpperInvariant()) {
        throw "Xray release checksum does not match."
    }

    $extract = Join-Path $work "extract"
    Expand-Archive -Path $zip -DestinationPath $extract
    Copy-Item (Join-Path $extract "*") -Destination $package -Recurse -Force
    Copy-Item $exe -Destination $package -Force
    Set-Content (Join-Path $package "THIRD-PARTY-NOTICES.txt") "Xray-core $version is distributed under the Mozilla Public License 2.0. See LICENSE and https://github.com/XTLS/Xray-core for license and dependency notices."

    $archive = Join-Path (Split-Path $package -Parent) "netray-windows.zip"
    Compress-Archive -Path (Join-Path $package "*") -DestinationPath $archive -Force
    Write-Host "Portable release package: $archive"
} finally {
    Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
}
