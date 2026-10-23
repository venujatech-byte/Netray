#[path = "../src/tunnel.rs"]
mod tunnel;
#[path = "../src/v2ray.rs"]
mod v2ray;
#[path = "../src/sysproxy.rs"]
mod sysproxy;
#[path = "../src/storage.rs"]
mod storage;

use storage::Profile;

#[test]
fn test_windows_tunnel_profile_initialization() {
    let mut profile = Profile::default();
    profile.name = "Test Windows Profile".to_string();
    profile.remote_host = "127.0.0.1".to_string();
    profile.remote_port = 1080;
    profile.enable_wintun = false;
    profile.enable_sysproxy = false;

    assert_eq!(profile.name, "Test Windows Profile");
    assert!(!profile.enable_wintun);
    assert!(!profile.enable_sysproxy);
}
