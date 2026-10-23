# Changelog - Netray Windows

All notable changes to the Netray Windows edition will be documented in this file.

## [1.0.0] - 2026-10-23

### Added
- **Native Windows Platform Support**:
  - Registry-based system proxy configuration (`reg.exe` and `wininet.dll` notification).
  - WinTun driver inbound configuration for full transparent tunneling.
  - Automatic `xray.exe` and `v2ray.exe` executable discovery.
  - Windows taskkill process tree supervisor for clean termination.
  - APPDATA profile persistence with zero-copy serialization.
- **Iced Windows GUI**:
  - Deep obsidian dark theme with electric blue accent (`#2563EB`).
  - Zero-allocation vector SVG icons via Lucide.
  - Comprehensive screen views: Profiles list, VLess editor, real-time log viewer, and SSH config.
- **Tooling & CI/CD**:
  - PowerShell elevation launcher `run.ps1` with auto-UAC check.
  - Batch scripts `run.bat` and `build.bat`.
  - Automatic WinTun driver download script `setup-wintun.ps1`.
  - GitHub Actions CI workflow for automated Windows release builds.
