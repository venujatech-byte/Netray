#[path = "../src/v2ray.rs"]
mod v2ray;
#[path = "../src/storage.rs"]
mod storage;

use storage::{Profile, V2rayConfig};

#[test]
fn test_windows_v2ray_config_generation() {
    let mut profile = Profile::default();
    profile.remote_host = "test.server.com".to_string();
    profile.remote_port = 8443;
    profile.enable_wintun = true;
    profile.v2ray = V2rayConfig {
        protocol: "vless".to_string(),
        uuid: "11111111-2222-3333-4444-555555555555".to_string(),
        security: "tls".to_string(),
        flow: "xtls-rprx-vision".to_string(),
        network: "tcp".to_string(),
        path: "".to_string(),
        header_type: "none".to_string(),
    };

    let config_json = v2ray::generate_xray_config(&profile, 10808).expect("Generate config");
    assert!(config_json.contains("11111111-2222-3333-4444-555555555555"));
    assert!(config_json.contains("wintun"));
    assert!(config_json.contains("10808"));
    assert!(config_json.contains("test.server.com"));
    assert!(config_json.contains("xtls-rprx-vision"));
}
