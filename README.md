# Netray

**Netray** is a modern, ultra-fast VPN and proxy client for Linux built in Rust with the [Iced](https://iced.rs/) GUI toolkit.

## Features

- **Xray & V2Ray Core**: Full support for VLESS, VMess, Trojan, and Shadowsocks protocols.
- **Rootless & Privileged TUN Mode**: Seamless full-system virtual network interface routing with automatic teardown on stop.
- **GNOME System Proxy Integration**: Instant one-click system-wide SOCKS5/HTTP routing.
- **SSH & HTTP Payload Injection**: Custom HTTP payload generator with Bug Host / SNI substitution and Direct SOCKS5 tunneling.
- **Modern UI**: Clean Material-inspired Dark aesthetic with Blue accent (`#2563EB`) and vector Lucide icons.
- **Zero-Allocation Hot Paths**: Static SVG bytecode caching, zero-copy configuration saving, and low memory footprint.

## Building & Running

Ensure you have Rust (2021 edition) and `xray` installed.

```bash
# Check and run
cargo run

# Build optimized release binary
cargo build --release
```

## Privileged TUN Capability (Optional)

To enable TUN interface creation without requiring `pkexec` password prompts on every start:

```bash
sudo setcap cap_net_admin,cap_net_bind_service=+ep /usr/local/bin/xray
```
