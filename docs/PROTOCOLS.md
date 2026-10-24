# Supported Protocol Matrix & Add Options

Netray supports 11 proxy, VPN, and tunneling protocol configurations directly from the **Add Profile** modal (`+` button).

## Protocol Overview

| Protocol | Badge | Engine | URI Scheme / Format | Key Configuration Attributes |
| :--- | :---: | :---: | :--- | :--- |
| **VMess** | `XRAY` | Xray-core | `vmess://<base64_json>` | User ID (UUID), WebSocket path, host, TLS |
| **VLess** | `XRAY` | Xray-core | `vless://uuid@host:port?...` | Flow (xtls-rprx-vision), Reality / TLS, PBK, SID |
| **Socks** | `XRAY` | Xray-core | `socks://user:pass@host:port` | SOCKS5 proxy server address, port, auth |
| **Trojan** | `XRAY` | Xray-core | `trojan://password@host:port?...` | Password, SNI, ALPN, TLS |
| **ShadowsocksR** | `XRAY` | Xray-core | `ssr://<base64>` | Obfs, protocol, method, password |
| **Shadowsocks2022** | `XRAY` | Xray-core | `ss://method:pass@host:port` | Modern 2022 AEAD ciphers (`2022-blake3-*`) |
| **WireGuard** | `XRAY` | Xray-core | `wireguard://privkey@host:port?...` | Secret key, peer public key, internal IP |
| **RAW config (JSON)** | `XRAY` | Xray-core | `{ "outbounds": [...] }` | Full multi-outbound custom Xray configuration |
| **SSH** | `NATIVE` | Pure Rust | `ssh://user:pass@host:port` | SOCKS5 multiplexed over native SSH tunnel |
| **DNSTT + SSH** | `NATIVE` | Pure Rust | `dnstt+ssh://ns:53?...` | DNS tunnel transport (Do53/DoH) + SSH |
| **Hysteria2** | `NEW` | Xray-core | `hy2://pass@host:port?...` | UDP QUIC protocol, SNI, TLS, insecure mode |

## Visual Badge System

The Add Profile modal renders color-coded chip badges matching the user interface:
- **`XRAY`**: Lime-yellow badge (`#e2f798` / `#527405`) for nodes executed via the high-performance Xray-core supervisor.
- **`NATIVE`**: Light green badge (`#daf2a7` / `#496d03`) for protocols processed directly in Rust without external binaries.
- **`NEW`**: Warm amber badge (`#fbd68b` / `#9a5203`) for modern next-gen UDP protocols (Hysteria2).

## Add Profile Modal Interaction

Clicking the `+` button in either Linux or Windows app displays the overlay modal with:
1. One-click profile generation from built-in templates.
2. Immediate persistence to disk (`netray_data/config.json`).
3. Auto-activation of the newly created profile.
4. Seamless integration with TUN mode, System Proxy, and app/URL split tunneling whitelist rules.
