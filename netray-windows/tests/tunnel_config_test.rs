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
