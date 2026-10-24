//! Netray Protocol Types and Definitions
//!
//! Enumerates all supported node configuration types matching modern proxy cores.

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
