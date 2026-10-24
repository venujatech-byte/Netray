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
