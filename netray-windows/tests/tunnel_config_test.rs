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
