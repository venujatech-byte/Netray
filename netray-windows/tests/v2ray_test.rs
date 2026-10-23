use netray_windows::v2ray;

#[test]
fn test_windows_v2ray_config_generation() {
    let uri = "vless://11111111-2222-3333-4444-555555555555@test.server.com:8443?encryption=none&security=tls&flow=xtls-rprx-vision&type=tcp#TestNode";

    let config_json = v2ray::parse_and_generate_xray_config(uri, 10808, 10809, true).expect("Generate config");
    assert!(config_json.contains("11111111-2222-3333-4444-555555555555"));
    assert!(config_json.contains("wintun"));
    assert!(config_json.contains("10808"));
    assert!(config_json.contains("test.server.com"));
    assert!(config_json.contains("xtls-rprx-vision"));
}
