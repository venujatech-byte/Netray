//! Netray Protocol Types and Definitions
//!
//! Enumerates all supported node configuration types matching modern proxy cores.

use base64::Engine;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProfileType {
    VMess,
    VLess,
    Socks,
    Trojan,
    ShadowsocksR,
    Shadowsocks2022,
    WireGuard,
    RawJson,
    Ssh,
    DnsttSsh,
    Hysteria2,
}

impl ProfileType {
    /// Returns the human-readable display title
    pub fn title(&self) -> &'static str {
        match self {
            ProfileType::VMess => "VMess",
            ProfileType::VLess => "VLess",
            ProfileType::Socks => "Socks",
            ProfileType::Trojan => "Trojan",
            ProfileType::ShadowsocksR => "ShadowsocksR",
            ProfileType::Shadowsocks2022 => "Shadowsocks2022",
            ProfileType::WireGuard => "WireGuard",
            ProfileType::RawJson => "RAW config (JSON)",
            ProfileType::Ssh => "SSH",
            ProfileType::DnsttSsh => "DNSTT + SSH",
            ProfileType::Hysteria2 => "Hysteria2",
        }
    }

    /// Visual badge type matching protocol core engine
    pub fn badge(&self) -> BadgeKind {
        match self {
            ProfileType::VMess
            | ProfileType::VLess
            | ProfileType::Socks
            | ProfileType::Trojan
            | ProfileType::ShadowsocksR
            | ProfileType::Shadowsocks2022
            | ProfileType::WireGuard
            | ProfileType::RawJson => BadgeKind::Xray,

            ProfileType::Ssh | ProfileType::DnsttSsh => BadgeKind::Native,

            ProfileType::Hysteria2 => BadgeKind::New,
        }
    }

    /// Generates a valid starting profile template `(remarks, config)` for the protocol
    pub fn generate_template(&self) -> (String, String) {
        match self {
            ProfileType::VMess => {
                let vmess_json = serde_json::json!({
                    "v": "2",
                    "ps": "New VMess Node",
                    "add": "127.0.0.1",
                    "port": 443,
                    "id": "b831381d-6324-4d53-ad4f-8cda48b30811",
                    "aid": 0,
                    "net": "ws",
                    "type": "none",
                    "host": "example.com",
                    "path": "/vmess",
                    "tls": "tls"
                });
                let encoded = base64::prelude::BASE64_STANDARD.encode(vmess_json.to_string().as_bytes());
                ("New VMess Node".to_string(), format!("vmess://{encoded}"))
            }
            ProfileType::VLess => (
                "New VLess Node".to_string(),
                "vless://b831381d-6324-4d53-ad4f-8cda48b30811@127.0.0.1:443?encryption=none&security=tls&sni=example.com&type=tcp#New-VLess-Node".to_string(),
            ),
            ProfileType::Socks => (
                "New Socks Node".to_string(),
                "socks://127.0.0.1:1080#New-Socks-Proxy".to_string(),
            ),
            ProfileType::Trojan => (
                "New Trojan Node".to_string(),
                "trojan://password123@127.0.0.1:443?security=tls&sni=example.com#New-Trojan-Node".to_string(),
            ),
            ProfileType::ShadowsocksR => {
                let ssr_raw = "127.0.0.1:8388:origin:aes-256-cfb:plain:cGFzc3dvcmQxMjM=/?remarks=TmV3LVNTUi1Ob2Rl";
                let encoded = base64::prelude::BASE64_STANDARD.encode(ssr_raw.as_bytes());
                ("New ShadowsocksR Node".to_string(), format!("ssr://{encoded}"))
            }
            ProfileType::Shadowsocks2022 => (
                "New Shadowsocks2022 Node".to_string(),
                "ss://2022-blake3-aes-128-gcm:7sN6q9U1jB3V0vYw8Z1A2Q==@127.0.0.1:8388#New-Shadowsocks2022-Node".to_string(),
            ),
            ProfileType::WireGuard => {
                let wg_json = serde_json::json!({
                    "outbounds": [{
                        "tag": "proxy",
                        "protocol": "wireguard",
                        "settings": {
                            "secretKey": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
                            "address": ["10.0.0.2/32"],
                            "peers": [{
                                "publicKey": "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB=",
                                "endpoint": "127.0.0.1:51820"
                            }]
                        }
                    }]
                });
                ("New WireGuard Node".to_string(), wg_json.to_string())
            }
            ProfileType::RawJson => {
                let raw_json = serde_json::json!({
                    "outbounds": [{
                        "protocol": "freedom",
                        "tag": "proxy"
                    }]
                });
                ("New RAW JSON Profile".to_string(), serde_json::to_string_pretty(&raw_json).unwrap_or_else(|_| raw_json.to_string()))
            }
            ProfileType::Ssh => (
                "New SSH Node".to_string(),
                "ssh://user:password123@127.0.0.1:22#New-SSH-Node".to_string(),
            ),
            _ => (format!("New {}", self.title()), String::new()),
        }
    }

    /// All 11 supported profile types in order
    pub const ALL: [ProfileType; 11] = [
        ProfileType::VMess,
        ProfileType::VLess,
        ProfileType::Socks,
        ProfileType::Trojan,
        ProfileType::ShadowsocksR,
        ProfileType::Shadowsocks2022,
        ProfileType::WireGuard,
        ProfileType::RawJson,
        ProfileType::Ssh,
        ProfileType::DnsttSsh,
        ProfileType::Hysteria2,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeKind {
    Xray,
    Native,
    New,
}

impl BadgeKind {
    pub fn text(&self) -> &'static str {
        match self {
            BadgeKind::Xray => "XRAY",
            BadgeKind::Native => "NATIVE",
            BadgeKind::New => "NEW",
        }
    }

    pub fn bg_color(&self) -> iced::Color {
        match self {
            BadgeKind::Xray => iced::Color::from_rgb(0.88, 0.96, 0.58),     // lime-yellowish #e2f798
            BadgeKind::Native => iced::Color::from_rgb(0.85, 0.95, 0.65),   // soft light green #daf2a7
            BadgeKind::New => iced::Color::from_rgb(0.98, 0.82, 0.52),      // warm amber #fbd68b
        }
    }

    pub fn text_color(&self) -> iced::Color {
        match self {
            BadgeKind::Xray => iced::Color::from_rgb(0.32, 0.45, 0.02),     // dark olive #527405
            BadgeKind::Native => iced::Color::from_rgb(0.28, 0.42, 0.01),   // dark forest #496d03
            BadgeKind::New => iced::Color::from_rgb(0.60, 0.32, 0.02),      // dark amber #9a5203
        }
    }
}
