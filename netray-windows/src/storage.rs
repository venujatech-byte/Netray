use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V2RayProfile {
    pub id: String,
    pub name: String,
    pub config: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppData {
    pub v2ray_profiles: Vec<V2RayProfile>,
    pub active_profile_id: Option<String>,
    pub v2ray_uri_or_json: String,
    pub local_socks_port: String,
    pub enable_tun: bool,
    pub enable_sysproxy: bool,
    pub tunnel_mode: String,
    pub ssh_host: String,
    pub ssh_port: String,
    pub ssh_user: String,
    pub ssh_pass: String,
    pub proxy_ip: String,
    pub proxy_port: String,
    pub payload_text: String,
    pub bug_host: String,
    pub sni_host: String,
    #[serde(default)]
    pub whitelist_apps: String,
    #[serde(default)]
    pub whitelist_urls: String,
}

impl Default for AppData {
    fn default() -> Self {
        let sample_uri = "vless://b831381d-6324-4d53-ad4f-8cda48b30811@example.com:443?encryption=none&security=tls&sni=example.com&type=ws&path=%2F#Sample-Node".to_string();
        let default_profile = V2RayProfile {
            id: "default-vless".to_string(),
            name: "Sample-Node".to_string(),
            config: sample_uri.clone(),
            created_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        };

        Self {
            v2ray_profiles: vec![default_profile],
            active_profile_id: Some("default-vless".to_string()),
            v2ray_uri_or_json: sample_uri,
            local_socks_port: "10808".to_string(),
            enable_tun: true,
            enable_sysproxy: false,
            tunnel_mode: "V2RayXray".to_string(),
            ssh_host: "sg1.myserver.com".to_string(),
            ssh_port: "22".to_string(),
            ssh_user: "netray".to_string(),
            ssh_pass: "".to_string(),
            proxy_ip: "104.16.51.111".to_string(),
            proxy_port: "80".to_string(),
            payload_text: "GET / HTTP/1.1[crlf]Host: [host][crlf]Upgrade: websocket[crlf][crlf]".to_string(),
            bug_host: "cdn.bug-example.com".to_string(),
            sni_host: "cdn.bug-example.com".to_string(),
            whitelist_apps: "".to_string(),
            whitelist_urls: "".to_string(),
        }
    }
}

pub fn get_config_dir() -> PathBuf {
    // Windows APPDATA, or fallback to current dir netray_data
    if let Ok(appdata) = std::env::var("APPDATA") {
        let p = PathBuf::from(appdata).join("Netray");
        let _ = fs::create_dir_all(&p);
        p
    } else if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        let p = PathBuf::from(home).join(".config").join("netray");
        let _ = fs::create_dir_all(&p);
        p
    } else {
        let p = PathBuf::from("./netray_data");
        let _ = fs::create_dir_all(&p);
        p
    }
}

pub fn get_config_path() -> PathBuf {
    get_config_dir().join("config.json")
}

pub fn load_data() -> AppData {
    let path = get_config_path();
    if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(mut data) = serde_json::from_str::<AppData>(&content) {
                if data.v2ray_profiles.is_empty() {
                    let def = AppData::default();
                    data.v2ray_profiles = def.v2ray_profiles;
                    data.active_profile_id = def.active_profile_id;
                    data.v2ray_uri_or_json = def.v2ray_uri_or_json;
                }
                return data;
            }
        }
    }
    let default_data = AppData::default();
    let _ = save_data(&default_data);
    default_data
}

#[derive(Serialize)]
pub struct AppDataRef<'a> {
    pub v2ray_profiles: &'a [V2RayProfile],
    pub active_profile_id: Option<&'a str>,
    pub v2ray_uri_or_json: &'a str,
    pub local_socks_port: &'a str,
    pub enable_tun: bool,
    pub enable_sysproxy: bool,
    pub tunnel_mode: &'a str,
    pub ssh_host: &'a str,
    pub ssh_port: &'a str,
    pub ssh_user: &'a str,
    pub ssh_pass: &'a str,
    pub proxy_ip: &'a str,
    pub proxy_port: &'a str,
    pub payload_text: &'a str,
    pub bug_host: &'a str,
    pub sni_host: &'a str,
    pub whitelist_apps: &'a str,
    pub whitelist_urls: &'a str,
}

impl AppData {
    pub fn as_ref(&self) -> AppDataRef<'_> {
        AppDataRef {
            v2ray_profiles: &self.v2ray_profiles,
            active_profile_id: self.active_profile_id.as_deref(),
            v2ray_uri_or_json: &self.v2ray_uri_or_json,
            local_socks_port: &self.local_socks_port,
            enable_tun: self.enable_tun,
            enable_sysproxy: self.enable_sysproxy,
            tunnel_mode: &self.tunnel_mode,
            ssh_host: &self.ssh_host,
            ssh_port: &self.ssh_port,
            ssh_user: &self.ssh_user,
            ssh_pass: &self.ssh_pass,
            proxy_ip: &self.proxy_ip,
            proxy_port: &self.proxy_port,
            payload_text: &self.payload_text,
            bug_host: &self.bug_host,
            sni_host: &self.sni_host,
            whitelist_apps: &self.whitelist_apps,
            whitelist_urls: &self.whitelist_urls,
        }
    }
}

pub fn save_state_ref(data: &AppDataRef<'_>) -> Result<PathBuf, String> {
    let path = get_config_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let json_str = serde_json::to_string_pretty(data)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    fs::write(&path, json_str)
        .map_err(|e| format!("Failed to write config to {}: {}", path.display(), e))?;
    Ok(path)
}

pub fn save_data(data: &AppData) -> Result<PathBuf, String> {
    save_state_ref(&data.as_ref())
}

pub fn save_active_xray_json(xray_json: &str) -> Result<PathBuf, String> {
    let dir = get_config_dir();
    let file_path = dir.join("active_xray.json");
    fs::write(&file_path, xray_json)
        .map_err(|e| format!("Failed to write active xray json to {}: {}", file_path.display(), e))?;
    Ok(file_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_and_load_config() {
        let mut app_data = AppData::default();
        app_data.v2ray_uri_or_json = "vless://test-uuid@1.2.3.4:443?type=tcp#TestNode".to_string();
        app_data.v2ray_profiles.push(V2RayProfile {
            id: "node-2".to_string(),
            name: "TestNode".to_string(),
            config: app_data.v2ray_uri_or_json.clone(),
            created_at: "2026-10-23".to_string(),
        });

        let serialized = serde_json::to_string_pretty(&app_data).expect("Must serialize");
        let deserialized: AppData = serde_json::from_str(&serialized).expect("Must deserialize");

        assert_eq!(deserialized.v2ray_profiles.len(), 2);
        assert_eq!(deserialized.v2ray_uri_or_json, "vless://test-uuid@1.2.3.4:443?type=tcp#TestNode");
    }

    #[test]
    fn test_disk_roundtrip() {
        let loaded = load_data();
        assert!(!loaded.v2ray_profiles.is_empty());
        let path = get_config_path();
        assert!(path.exists(), "config.json must exist at {}", path.display());
    }
}
