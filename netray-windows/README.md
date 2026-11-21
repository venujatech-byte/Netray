# Netray Windows

Ultra-lightweight, high-performance V2Ray, VLess, and SSH tunneling client for Windows, built with Rust and Iced.

## Key Features

- **Native Windows Integration**:
  - System proxy manipulation via Windows Registry (`HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings`).
  - Seamless notification using `wininet.dll` (`InternetSetOptionW`).
  - Native WinTun adapter inbound support (`wintun.dll`) for complete system-wide traffic tunneling.
  - Split-tunneling whitelist: easily bypass specific apps (e.g. `chrome.exe`, `discord.exe`) or domains/URLs/IPs from the tunnel in both WinTun and Proxy modes.
  - Uses a bundled `xray.exe` beside Netray, or finds an existing core on PATH or in common install folders.
  - Windows process supervision and clean termination (`taskkill /F /T /IM xray.exe`).
- **Modern Iced GUI**:
  - Electric Blue accent palette tailored for Windows 10 and Windows 11 dark mode.
  - Zero-allocation vector SVG Lucide icons.
  - Tabbed cockpit interface: Profiles, VLess Editor, Logs, and SSH configuration.
- **Robust Configuration**:
  - Automatically loads and saves profiles to `%APPDATA%\Netray\config.json`.
  - Fallback to `./netray_data/config.json` for portable mode.

## Quick Start

### Prerequisites

1. Install [Rust](https://rustup.rs/) (1.75+ recommended).
2. For development runs, install Xray or put `xray.exe` on PATH. The release package includes Xray.
3. (Optional for WinTun Mode) Download `wintun.dll` using `scripts\setup-wintun.ps1`.

### Running on Windows

Launch using PowerShell with administrator privileges (recommended for WinTun and Proxy):
```powershell
.\scripts\run.ps1
```

Or run via Cargo:
```bash
cargo run --release
```

### Building Release Packages

```bat
scripts\build.bat
```
Or with PowerShell:
```powershell
.\scripts\package-windows.ps1 -Version 1.0.0
```
This builds three release distribution packages into `dist\`:
1. `netray-windows-<version>.msi`: Native Windows Installer with wizard, Start Menu/Desktop shortcuts, and upgrade/uninstall management.
2. `netray-windows-<version>.msix`: Modern packaged application ready for Microsoft Store submission and Windows 10/11 sideloading.
3. `netray-windows-<version>.zip`: Portable package bundled with Xray core and WinTun driver.
4. `netray-windows-<version>.cer`: Test certificate for local sideloading of the MSIX package.

See [docs/WINDOWS_STORE_PUBLISHING.md](../docs/WINDOWS_STORE_PUBLISHING.md) for full Microsoft Store submission instructions.
