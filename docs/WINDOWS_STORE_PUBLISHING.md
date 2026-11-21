# Publishing Netray to the Microsoft Store & Windows Packaging Guide

Netray for Windows is built with support for multiple distribution formats:
1. **Windows MSI Installer (`.msi`)**: Traditional Windows Installer package with full GUI setup wizard, Start Menu & Desktop shortcuts, and seamless upgrade/uninstall support.
2. **Microsoft Store / MSIX Package (`.msix`)**: Modern packaged Windows application ready for direct submission to Microsoft Partner Center.
3. **Windows Portable Archive (`.zip`)**: Standalone portable folder with bundled Xray core and WinTun driver.
4. **Development Sideload Certificate (`.cer`)**: Exported public certificate for local sideloading and testing of MSIX packages before Store submission.

---

## 1. Publishing to Microsoft Store via Partner Center

### Step 1: Reserve Your Product Name
1. Log in to [Microsoft Partner Center](https://partner.microsoft.com/dashboard).
2. Go to **Apps & games** > **New product** > **MSIX or PWA app** (or **EXE or MSI app**).
3. Reserve your app name (e.g. `Netray`).
4. Once reserved, navigate to **Product management** > **Package identity**. Note the following fields:
   - **Package/Identity/Name**: e.g. `12345YourName.Netray`
   - **Package/Identity/Publisher**: e.g. `CN=98765432-ABCD-1234-EF01-1234567890AB`
   - **Publisher display name**: e.g. `Your Publisher Name`

### Step 2: Configure CI / Packaging Variables
When packaging the MSIX container, Netray dynamically injects these values into `AppxManifest.xml`. You can set them as GitHub Actions Repository Secrets / Environment Variables or pass them directly to the packaging script:

| Environment Variable | Parameter | Default Value | Description |
|---|---|---|---|
| `MS_STORE_IDENTITY_NAME` | `-StoreIdentityName` | `Netray` | Identity Name from Partner Center |
| `MS_STORE_PUBLISHER` | `-StorePublisher` | `CN=Netray` | Publisher ID string from Partner Center |
| `MS_STORE_PUBLISHER_DISPLAY_NAME` | `-StorePublisherDisplayName` | `VTechDev` | Publisher display name |

In GitHub Actions, you can configure these secrets under **Settings > Secrets and variables > Actions**.

### Step 3: Submitting the MSIX Package
1. On each release or CI run, the workflow produces `netray-windows-<VERSION>.msix`.
2. In Partner Center, start a **New submission**.
3. Under **Packages**, upload `netray-windows-<VERSION>.msix`.
4. Partner Center will inspect the package. Note the following declarations:
   - **`runFullTrust`**: Allows full Win32 execution privileges.
   - **`allowElevation`**: Permits UAC privilege elevation, required for configuring the WinTun virtual network adapter and modifying Windows Internet proxy settings (`InternetSetOptionW`).
   - When prompted for submission notes regarding elevated capabilities, explain:
     > *"Netray is a network tunneling client. It requires full trust and elevation to install and initialize the WinTun virtual network adapter and configure Windows system proxy routes."*
5. Fill out store listing details (icons from `packaging/windows/assets` are already formatted to store specifications), privacy policy, and pricing.
6. Submit for certification! Partner Center automatically signs the package with Microsoft's trusted Store certificate during publishing.

---

## 2. Alternative: Publishing via Win32 Installer (MSI)

Microsoft Store also supports submitting traditional Win32 installers directly:
1. In Partner Center, choose **EXE or MSI app**.
2. Under **Package URL**, provide the download link to `netray-windows-<VERSION>.msi` from your GitHub Release.
3. Silent install command: `msiexec /i netray-windows-<VERSION>.msi /qn`
4. Silent uninstall command: `msiexec /x {9B561498-8422-491C-B7D4-4860D4D32A15} /qn`

---

## 3. Local Sideloading & Testing the MSIX Package

Before uploading to Partner Center, you can test the generated `.msix` directly on Windows 10 or 11:

1. Locate the built `netray-windows-<VERSION>.msix` and `netray-windows-<VERSION>.cer` in the `dist` folder.
2. Double-click `netray-windows-<VERSION>.cer` > **Install Certificate**.
3. Choose **Local Machine** > **Next**.
4. Select **Place all certificates in the following store** > **Browse** > choose **Trusted People** > **OK** > **Next** > **Finish**.
5. Double-click `netray-windows-<VERSION>.msix` and click **Install**.

---

## 4. Local Building Instructions

To build all packages on your local Windows development machine:

### Prerequisites
1. **Rust toolchain**: `rustup default stable`
2. **WiX Toolset v3.11** (for MSI):
   ```powershell
   choco install wixtoolset -y
   ```
3. **Windows 10/11 SDK** (for MSIX / `makeappx.exe`):
   Installed with Visual Studio or via [Windows SDK](https://developer.microsoft.com/en-us/windows/downloads/windows-sdk/).

### Build Command
```powershell
cd netray-windows
cargo build --release
.\scripts\package-windows.ps1 -Version 1.0.4
```

To customize Store credentials locally:
```powershell
.\scripts\package-windows.ps1 -Version 1.0.4 -StoreIdentityName "MyCompany.Netray" -StorePublisher "CN=MY-GUID-HERE" -StorePublisherDisplayName "MyCompany"
```
