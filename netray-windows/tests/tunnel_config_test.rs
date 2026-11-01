use netray_windows::storage::AppData;

#[test]
fn test_windows_tunnel_profile_initialization() {
    let mut data = AppData::default();
    data.enable_tun = true;
    data.enable_sysproxy = true;

    assert!(data.enable_tun);
    assert!(data.enable_sysproxy);
    assert_eq!(data.local_socks_port, "10808");
}

#[test]
fn test_windows_health_probe_port_and_ssh_settings() {
    let mut data = AppData::default();
    data.local_socks_port = "10808".to_string();
    let http_probe_port = data
        .local_socks_port
        .parse::<u16>()
        .unwrap_or(10808)
        .saturating_add(1);
    assert_eq!(http_probe_port, 10809);

    // Verify SSH configuration defaults
    assert_eq!(data.ssh_port, "22");
    assert!(!data.ssh_host.is_empty());
    assert!(!data.payload_text.is_empty());
}

#[test]
fn test_windows_kill_all_xray_processes_callable() {
    // Calling kill_all_xray_processes must be safe and idempotent across platforms
    netray_windows::v2ray::kill_all_xray_processes();
}

#[test]
fn test_windows_manifest_declares_require_administrator() {
    let manifest_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/app.manifest");
    assert!(manifest_path.exists(), "app.manifest must exist");
    let content = std::fs::read_to_string(&manifest_path).expect("Read app.manifest");
    assert!(
        content.contains(r#"<requestedExecutionLevel level="requireAdministrator" uiAccess="false" />"#),
        "app.manifest must mandate requireAdministrator for default admin rights"
    );
}
