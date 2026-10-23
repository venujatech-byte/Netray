# Netray Windows

Ultra-lightweight, high-performance V2Ray, VLess, and SSH tunneling client for Windows, built with Rust and Iced.

## Key Features

- **Native Windows Integration**:
  - System proxy manipulation via Windows Registry (`HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings`).
  - Seamless notification using `wininet.dll` (`InternetSetOptionW`).
  - Native WinTun adapter inbound support (`wintun.dll`) for complete system-wide traffic tunneling.
  - Automatic `xray.exe` and `v2ray.exe` detection across PATH, `%ProgramFiles%\Xray`, `%LOCALAPPDATA%\Xray`, and local directory.
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
2. Download `xray.exe` or `v2ray.exe` and ensure it is placed in your PATH or in the same directory as the executable.
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
The compiled binary will be placed at `target\release\netray-windows.exe`.
