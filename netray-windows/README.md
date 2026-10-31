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

### Building Release Package

```bat
scripts\build.bat
```
This builds `dist\netray-windows-0.1.0.zip`, a portable package containing Netray and the official Xray-core release. The build downloads the pinned Xray release from GitHub and verifies its SHA-256 checksum. GitHub Actions publishes a versioned package on each push to `main` or `master`.
