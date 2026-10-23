#[path = "../src/storage.rs"]
mod storage;

use storage::{AppConfig, Profile, SshConfig, V2rayConfig};

#[test]
fn test_windows_storage_roundtrip() {
    let temp_dir = std::env::temp_dir().join(format!("netray_win_test_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let _ = std::fs::create_dir_all(&temp_dir);
    let config_path = temp_dir.join("config.json");

    let mut config = AppConfig::default();
    config.profiles.push(Profile {
        name: "Windows Direct SOCKS".to_string(),
        remote_host: "104.16.0.1".to_string(),
        remote_port: 443,
        payload: "".to_string(),
        sni: "cloudflare.com".to_string(),
        enable_wintun: true,
        enable_sysproxy: true,
        ssh: SshConfig::default(),
        v2ray: V2rayConfig::default(),
    });

    let json = serde_json::to_string_pretty(&config).expect("Serialize config");
    std::fs::write(&config_path, json).expect("Write test config");

    let loaded_content = std::fs::read_to_string(&config_path).expect("Read test config");
    let loaded: AppConfig = serde_json::from_str(&loaded_content).expect("Deserialize config");

    assert_eq!(loaded.profiles.len(), 1);
    assert_eq!(loaded.profiles[0].name, "Windows Direct SOCKS");
    assert!(loaded.profiles[0].enable_wintun);
    assert!(loaded.profiles[0].enable_sysproxy);

    let _ = std::fs::remove_dir_all(&temp_dir);
}
