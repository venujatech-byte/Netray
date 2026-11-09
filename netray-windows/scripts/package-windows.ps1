param([string]$Version = "1.0.0")

$ErrorActionPreference = "Stop"
if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "Version must use MAJOR.MINOR.PATCH format." }

$xrayVersion = "v26.10.21"
$asset = "Xray-windows-64.zip"
$releaseUrl = "https://github.com/XTLS/Xray-core/releases/download/$xrayVersion"
$mirrorUrl = "https://sourceforge.net/projects/xray-core.mirror/files/$xrayVersion"
$expectedArchiveSha256 = "fc58c39ee2f2717b4c54d28d374362389e5319b20ae44aea76ea45a5243ec827"
$work = Join-Path ([IO.Path]::GetTempPath()) ("netray-xray-" + [guid]::NewGuid())
$package = Join-Path $PSScriptRoot "..\dist\netray-windows"
$exe = Join-Path $PSScriptRoot "..\target\release\netray-windows.exe"

function Get-ReleaseAsset([string]$PrimaryUrl, [string]$FallbackUrl, [string]$Path, [string]$ExpectedSha256) {
    foreach ($source in @($PrimaryUrl, $FallbackUrl)) {
        & curl.exe --fail --location --silent --show-error --retry 2 --retry-all-errors `
            --retry-delay 2 --connect-timeout 20 --max-time 180 --output $Path $source
        if ($LASTEXITCODE -eq 0) {
            $actualSha256 = (Get-FileHash $Path -Algorithm SHA256).Hash
            if ($actualSha256 -eq $ExpectedSha256.ToUpperInvariant()) {
                return
            }
            Write-Warning "Checksum mismatch from $source (got $actualSha256); trying the next source."
        } else {
            Write-Warning "Download from $source failed with curl exit code $LASTEXITCODE; trying the next source."
        }
        Remove-Item $Path -Force -ErrorAction SilentlyContinue
    }
    throw "Could not download and verify the pinned Xray $xrayVersion archive from GitHub or SourceForge."
}

try {
    if (-not (Test-Path $exe -PathType Leaf)) { throw "Netray executable not found: $exe" }
    New-Item -ItemType Directory -Path $work, $package -Force | Out-Null
    $zip = Join-Path $work $asset
    Get-ReleaseAsset "$mirrorUrl/$asset/download" "$releaseUrl/$asset" $zip $expectedArchiveSha256

    $extract = Join-Path $work "extract"
    Expand-Archive -Path $zip -DestinationPath $extract
    Copy-Item (Join-Path $extract "*") -Destination $package -Recurse -Force
    python (Join-Path $PSScriptRoot "..\..\scripts\build-xray.py") (Join-Path $package "xray.exe")
    if ($LASTEXITCODE -ne 0) { throw "Patched Xray build failed." }
    Copy-Item $exe -Destination $package -Force
    Remove-Item (Join-Path $package "config.json") -Force -ErrorAction SilentlyContinue
    Remove-Item (Join-Path $package "netray_data") -Recurse -Force -ErrorAction SilentlyContinue
    Set-Content (Join-Path $package "THIRD-PARTY-NOTICES.txt") @"
Netray bundles Xray-core v26.10.21 (1b1647cd9ddf) with a Netray TLS compatibility modification.
License: Mozilla Public License 2.0; see the accompanying LICENSE file.
Corresponding modified source: XRAY-SOURCE.tar.gz; patch: patch-xray-tls.py
Upstream source: https://github.com/XTLS/Xray-core/tree/1b1647cd9ddf0abc34b981d4611fc03bcac1ca74
Upstream dependency list: https://github.com/XTLS/Xray-core/blob/1b1647cd9ddf0abc34b981d4611fc03bcac1ca74/go.mod
"@

    $archive = Join-Path (Split-Path $package -Parent) "netray-windows-$Version.zip"
    Compress-Archive -Path (Join-Path $package "*") -DestinationPath $archive -Force
    Write-Host "Portable release package: $archive"
} finally {
    Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
}
