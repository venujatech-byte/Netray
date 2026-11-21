param(
    [string]$Version = "2.0.0",
    [string]$StoreIdentityName = "",
    [string]$StorePublisher = "",
    [string]$StorePublisherDisplayName = "",
    [switch]$SkipMsi = $false,
    [switch]$SkipMsix = $false
)

$ErrorActionPreference = "Stop"
if ($Version -notmatch '^\d+\.\d+\.\d+(\.\d+)?$') {
    throw "Version must use MAJOR.MINOR.PATCH format (e.g. 1.0.4)."
}

$xrayVersion = "v26.10.21"
$asset = "Xray-windows-64.zip"
$releaseUrl = "https://github.com/XTLS/Xray-core/releases/download/$xrayVersion"
$work = Join-Path ([IO.Path]::GetTempPath()) ("netray-pkg-" + [guid]::NewGuid().ToString("N"))
$distDir = Join-Path $PSScriptRoot "..\dist"
$package = Join-Path $distDir "netray-windows"
$exe = Join-Path $PSScriptRoot "..\target\release\netray-windows.exe"
$packagingDir = Join-Path $PSScriptRoot "..\..\packaging\windows"
$iconPath = Join-Path $PSScriptRoot "..\resources\netray.ico"

# Microsoft Store Identity defaults / overrides
$finalStoreIdentity = if ($StoreIdentityName) { $StoreIdentityName } elseif ($env:MS_STORE_IDENTITY_NAME) { $env:MS_STORE_IDENTITY_NAME } else { "Netray" }
$finalStorePub = if ($StorePublisher) { $StorePublisher } elseif ($env:MS_STORE_PUBLISHER) { $env:MS_STORE_PUBLISHER } else { "CN=Netray" }
$finalStorePubDisplay = if ($StorePublisherDisplayName) { $StorePublisherDisplayName } elseif ($env:MS_STORE_PUBLISHER_DISPLAY_NAME) { $env:MS_STORE_PUBLISHER_DISPLAY_NAME } else { "VTechDev" }

function Find-WixBin {
    if (Get-Command candle.exe -ErrorAction SilentlyContinue) {
        return (Split-Path (Get-Command candle.exe).Source -Parent)
    }
    $candidates = @(
        "C:\Program Files (x86)\WiX Toolset v3.11\bin",
        "C:\Program Files\WiX Toolset v3.11\bin",
        "$env:WIX\bin"
    )
    foreach ($cand in $candidates) {
        if ($cand -and (Test-Path (Join-Path $cand "candle.exe"))) {
            return $cand
        }
    }
    # Attempt Chocolatey installation if available
    if (Get-Command choco.exe -ErrorAction SilentlyContinue) {
        Write-Host "WiX Toolset not found in PATH. Attempting install via Chocolatey..."
        choco install wixtoolset -y --no-progress | Out-Null
        $chocoCand = "C:\Program Files (x86)\WiX Toolset v3.11\bin"
        if (Test-Path (Join-Path $chocoCand "candle.exe")) {
            return $chocoCand
        }
    }
    return $null
}

function Find-SdkTool([string]$ToolName) {
    $cmd = Get-Command $ToolName -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    $sdkRoots = @(
        "C:\Program Files (x86)\Windows Kits\10\bin",
        "C:\Program Files\Windows Kits\10\bin"
    )
    foreach ($root in $sdkRoots) {
        if (Test-Path $root) {
            $bins = Get-ChildItem -Path $root -Filter $ToolName -Recurse -ErrorAction SilentlyContinue |
                Where-Object { $_.FullName -like "*\x64\*" } |
                Sort-Object FullName -Descending
            if ($bins -and $bins.Count -gt 0) {
                return $bins[0].FullName
            }
        }
    }
    return $null
}

try {
    if (-not (Test-Path $exe -PathType Leaf)) { throw "Netray executable not found: $exe" }
    New-Item -ItemType Directory -Path $work, $package, $distDir -Force | Out-Null

    # 1. Download official Xray release & verify checksum
    $zip = Join-Path $work $asset
    $dgst = Join-Path $work "$asset.dgst"
    Write-Host "Downloading Xray release assets ($xrayVersion)..."
    Invoke-WebRequest "$releaseUrl/$asset" -OutFile $zip
    Invoke-WebRequest "$releaseUrl/$asset.dgst" -OutFile $dgst

    $checksumText = Get-Content $dgst -Raw
    $match = [regex]::Match($checksumText, '(?im)^\s*sha(?:2-)?256\s*=\s*([0-9a-f]{64})\s*$|^\s*SHA(?:2-)?256\s+\([^)]*\)\s*=\s*([0-9a-f]{64})\s*$')
    if (-not $match.Success) { throw "Xray release checksum is missing or invalid." }
    $expected = if ($match.Groups[1].Success) { $match.Groups[1].Value } else { $match.Groups[2].Value }
    if ((Get-FileHash $zip -Algorithm SHA256).Hash -ne $expected.ToUpperInvariant()) {
        throw "Xray release checksum does not match."
    }

    # 2. Extract Xray bundle and compile patched xray.exe
    $extract = Join-Path $work "extract"
    Expand-Archive -Path $zip -DestinationPath $extract
    Copy-Item (Join-Path $extract "*") -Destination $package -Recurse -Force
    Write-Host "Building patched Xray core..."
    python (Join-Path $PSScriptRoot "..\..\scripts\build-xray.py") (Join-Path $package "xray.exe")
    if ($LASTEXITCODE -ne 0) { throw "Patched Xray build failed." }

    # 3. Copy Netray Windows binary and licenses
    Copy-Item $exe -Destination $package -Force
    Remove-Item (Join-Path $package "config.json") -Force -ErrorAction SilentlyContinue
    Remove-Item (Join-Path $package "netray_data") -Recurse -Force -ErrorAction SilentlyContinue

    $netrayLicense = Join-Path $PSScriptRoot "..\..\packaging\LICENSE-Netray.txt"
    if (Test-Path $netrayLicense) {
        Copy-Item $netrayLicense -Destination (Join-Path $package "LICENSE-Netray.txt") -Force
    }

    Set-Content (Join-Path $package "THIRD-PARTY-NOTICES.txt") @"
Netray bundles Xray-core 1b1647cd9ddf with a Netray TLS compatibility modification.
License: Mozilla Public License 2.0; see the accompanying LICENSE file.
Corresponding modified source: XRAY-SOURCE.tar.gz; patch: patch-xray-tls.py
Upstream source: https://github.com/XTLS/Xray-core/tree/1b1647cd9ddf
Upstream dependency list: https://github.com/XTLS/Xray-core/blob/1b1647cd9ddf/go.mod
"@

    # 4. Create portable ZIP package
    $zipArchive = Join-Path $distDir "netray-windows-$Version.zip"
    Compress-Archive -Path (Join-Path $package "*") -DestinationPath $zipArchive -Force
    Write-Host "Portable release package: $zipArchive"

    # 5. Build Windows MSI Installer
    if (-not $SkipMsi) {
        Write-Host "--- Building Windows MSI Installer ---"
        $wixBin = Find-WixBin
        if (-not $wixBin) {
            if ($env:GITHUB_ACTIONS -eq "true") {
                throw "WiX Toolset (candle.exe / light.exe) was not found in CI environment."
            } else {
                Write-Warning "WiX Toolset not found. Skipping MSI build. Install WiX v3.11 with: choco install wixtoolset"
            }
        } else {
            $candleExe = Join-Path $wixBin "candle.exe"
            $lightExe = Join-Path $wixBin "light.exe"
            $wxsTemplate = Join-Path $packagingDir "netray.wxs"
            $licenseRtf = Join-Path $packagingDir "license.rtf"
            $msiPath = Join-Path $distDir "netray-windows-$Version.msi"

            if (-not (Test-Path $wxsTemplate)) { throw "WiX template not found: $wxsTemplate" }
            if (-not (Test-Path $licenseRtf)) { throw "License RTF not found: $licenseRtf" }
            if (-not (Test-Path $iconPath)) { throw "Netray icon not found: $iconPath" }

            # Dynamically harvest package files into files.wxs
            $filesWxsPath = Join-Path $work "files.wxs"
            $wxsContent = New-Object System.Text.StringBuilder
            [void]$wxsContent.AppendLine('<?xml version="1.0" encoding="UTF-8"?>')
            [void]$wxsContent.AppendLine('<Wix xmlns="http://schemas.microsoft.com/wix/2006/wi">')
            [void]$wxsContent.AppendLine('  <Fragment>')
            [void]$wxsContent.AppendLine('    <ComponentGroup Id="NetrayAppFiles" Directory="INSTALLFOLDER">')

            $pkgFiles = Get-ChildItem -Path $package -File
            $fileIdx = 0
            foreach ($pf in $pkgFiles) {
                $fileIdx++
                $cleanName = [regex]::Replace($pf.Name, '[^a-zA-Z0-9_]', '_')
                $fileId = if ($pf.Name -ieq "netray-windows.exe") { "File_netray_windows_exe" } else { "File_${fileIdx}_$cleanName" }
                $compId = "Comp_${fileIdx}_$cleanName"
                [void]$wxsContent.AppendLine("      <Component Id=`"$compId`" Guid=`"*`" Win64=`"yes`">")
                [void]$wxsContent.AppendLine("        <File Id=`"$fileId`" Source=`"`$(var.SourceDir)\$($pf.Name)`" KeyPath=`"yes`" />")
                [void]$wxsContent.AppendLine("      </Component>")
            }

            [void]$wxsContent.AppendLine('    </ComponentGroup>')
            [void]$wxsContent.AppendLine('  </Fragment>')
            [void]$wxsContent.AppendLine('</Wix>')
            [IO.File]::WriteAllText($filesWxsPath, $wxsContent.ToString(), [System.Text.Encoding]::UTF8)

            Write-Host "Compiling WiX components with candle.exe..."
            & $candleExe -nologo -arch x64 `
                "-dVersion=$Version" `
                "-dSourceDir=$package" `
                "-dIconPath=$iconPath" `
                "-dLicenseRtf=$licenseRtf" `
                "-out" "$work\" `
                $wxsTemplate $filesWxsPath
            if ($LASTEXITCODE -ne 0) { throw "WiX candle compilation failed." }

            Write-Host "Linking MSI installer with light.exe..."
            $wixObj1 = Join-Path $work "netray.wixobj"
            $wixObj2 = Join-Path $work "files.wixobj"

            # Run light with WixUIExtension for modern installer dialog
            & $lightExe -nologo -ext WixUIExtension -spdb -sval $wixObj1 $wixObj2 -out $msiPath
            if ($LASTEXITCODE -ne 0) {
                Write-Host "Retrying light.exe without UI extension..."
                & $lightExe -nologo -spdb -sval $wixObj1 $wixObj2 -out $msiPath
                if ($LASTEXITCODE -ne 0) { throw "WiX light linking failed." }
            }
            Write-Host "Windows MSI Installer package created: $msiPath"
        }
    }

    # 6. Build Windows Store (MSIX) Package
    if (-not $SkipMsix) {
        Write-Host "--- Building Microsoft Store (MSIX) Package ---"
        $makeAppx = Find-SdkTool "makeappx.exe"
        if (-not $makeAppx) {
            if ($env:GITHUB_ACTIONS -eq "true") {
                throw "Windows SDK tool makeappx.exe was not found in CI environment."
            } else {
                Write-Warning "makeappx.exe not found. Skipping MSIX build. Install Windows 10/11 SDK."
            }
        } else {
            $msixStaging = Join-Path $work "msix_staging"
            New-Item -ItemType Directory -Path $msixStaging -Force | Out-Null

            # Copy application payload into MSIX staging root
            Copy-Item (Join-Path $package "*") -Destination $msixStaging -Recurse -Force

            # Copy Assets folder (Store logos, tiles, splash)
            $assetsDir = Join-Path $packagingDir "assets"
            if (-not (Test-Path $assetsDir)) { throw "MSIX assets directory not found: $assetsDir" }
            Copy-Item $assetsDir -Destination (Join-Path $msixStaging "Assets") -Recurse -Force

            # Format 4-part quad version required by AppxManifest (e.g. 1.0.4 -> 1.0.4.0)
            $versionParts = $Version.Split('.')
            while ($versionParts.Count -lt 4) { $versionParts += "0" }
            $msixVersion = ($versionParts[0..3]) -join '.'

            # Generate AppxManifest.xml with dynamic identity & version
            $manifestTemplate = Join-Path $packagingDir "AppxManifest.xml"
            if (-not (Test-Path $manifestTemplate)) { throw "AppxManifest template not found: $manifestTemplate" }
            $manifestXml = Get-Content $manifestTemplate -Raw
            $manifestXml = $manifestXml.Replace('{{IDENTITY_NAME}}', $finalStoreIdentity)
            $manifestXml = $manifestXml.Replace('{{PUBLISHER}}', $finalStorePub)
            $manifestXml = $manifestXml.Replace('{{PUBLISHER_DISPLAY_NAME}}', $finalStorePubDisplay)
            $manifestXml = $manifestXml.Replace('{{VERSION}}', $msixVersion)
            [IO.File]::WriteAllText((Join-Path $msixStaging "AppxManifest.xml"), $manifestXml, [System.Text.Encoding]::UTF8)

            $msixPath = Join-Path $distDir "netray-windows-$Version.msix"
            Write-Host "Packaging MSIX container with makeappx.exe..."
            & $makeAppx pack /d $msixStaging /p $msixPath /o
            if ($LASTEXITCODE -ne 0) { throw "MakeAppx failed to package MSIX." }

            # Code-signing step for local testing/sideloading
            $signTool = Find-SdkTool "signtool.exe"
            if ($signTool) {
                if ($env:MSIX_CERT_PATH -and (Test-Path $env:MSIX_CERT_PATH)) {
                    Write-Host "Signing MSIX with provided certificate: $env:MSIX_CERT_PATH"
                    $signArgs = @("sign", "/fd", "SHA256", "/f", $env:MSIX_CERT_PATH)
                    if ($env:MSIX_CERT_PASSWORD) { $signArgs += @("/p", $env:MSIX_CERT_PASSWORD) }
                    $signArgs += $msixPath
                    & $signTool @signArgs
                } else {
                    try {
                        $certSubject = $finalStorePub
                        if ($certSubject -notmatch '^CN=') { $certSubject = "CN=$certSubject" }
                        Write-Host "Creating self-signed test certificate for '$certSubject'..."
                        $cert = New-SelfSignedCertificate `
                            -Type Custom `
                            -Subject $certSubject `
                            -KeyUsage DigitalSignature `
                            -FriendlyName "Netray Package Signing Certificate" `
                            -CertStoreLocation "Cert:\CurrentUser\My" `
                            -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3")

                        Write-Host "Signing MSIX package for local testing..."
                        & $signTool sign /fd SHA256 /sha1 $cert.Thumbprint /v $msixPath

                        # Export public certificate for easy local sideloading / testing
                        $cerPath = Join-Path $distDir "netray-windows-$Version.cer"
                        Export-Certificate -Cert $cert -FilePath $cerPath | Out-Null
                        Write-Host "Exported test certificate for local sideloading: $cerPath"
                    } catch {
                        Write-Warning "Could not sign MSIX for local sideloading: $_"
                        Write-Host "Note: Microsoft Partner Center signs the package automatically during Store ingestion."
                    }
                }
            } else {
                Write-Host "signtool.exe not found; package left unsigned. Microsoft Partner Center signs during Store ingestion."
            }
            Write-Host "Microsoft Store (MSIX) package created: $msixPath"
        }
    }

    Write-Host "All Windows packaging steps completed successfully!"
} finally {
    Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
}
