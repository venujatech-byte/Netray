# Netray

**Netray** is a modern, ultra-fast VPN, V2Ray/Xray, and SSH tunneling client built in Rust with the [Iced](https://iced.rs/) GUI toolkit.

## Editions & Platforms

Netray is available with first-class native support for both **Linux** and **Windows**:

- **[Netray Linux](file:///home/venujasilva/Documents/Coding/Netray)**:
  - Native Linux TUN routing (`ip tuntap`, `ip route`) with clean automatic teardown.
  - Desktop GNOME system proxy configuration via `gsettings`.
  - Process supervision via SIGTERM / SIGKILL.
  - Native XDG config directories (`~/.config/netray`).

- **[Netray Windows](file:///home/venujasilva/Documents/Coding/Netray/netray-windows/README.md)**:
  - Full transparent system routing via **WinTun** driver (`wintun.dll`).
  - Windows Registry system proxy automation (`Internet Settings`) with WinINet live notification.
  - Windows process supervision via `taskkill /F /T`.
  - Native `%APPDATA%\Netray` persistence and portable mode support.
  - Automated launchers (`scripts/run.ps1`, `scripts/run.bat`).

## Features

- **Xray & V2Ray Core**: Full support for VLESS, VMess, Trojan, and Shadowsocks protocols.
- **Rootless & Privileged TUN Mode**: Seamless full-system virtual network interface routing with automatic teardown on stop.
- **System Proxy Integration**: Instant one-click system-wide SOCKS5/HTTP routing across Linux desktops and Windows.
- **SSH & HTTP Payload Injection**: Custom HTTP payload generator with Bug Host / SNI substitution and Direct SOCKS5 tunneling.
- **Modern UI**: Clean Material-inspired Dark aesthetic with Blue accent (`#2563EB`) and vector Lucide icons.
- **Zero-Allocation Hot Paths**: Static SVG bytecode caching, zero-copy configuration saving, and low memory footprint.

## Building & Running (Linux)

Ensure you have Rust (2021 edition) and `xray` installed.

```bash
# Check and run
cargo run

# Build optimized release binary
cargo build --release
```

## Building & Running (Windows)

Navigate to the `netray-windows/` directory:

```powershell
cd netray-windows
.\scripts\run.ps1
```

Or build manually:
```bat
scripts\build.bat
```

## Privileged TUN Capability on Linux (Optional)

To enable TUN interface creation without requiring `pkexec` password prompts on every start:

```bash
sudo setcap cap_net_admin,cap_net_bind_service=+ep /usr/local/bin/xray
```

