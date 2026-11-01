use netray_windows::storage::{AppData, V2RayProfile};

#[test]
fn test_windows_storage_roundtrip() {
    let temp_dir = std::env::temp_dir().join(format!("netray_win_test_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let _ = std::fs::create_dir_all(&temp_dir);
    let config_path = temp_dir.join("config.json");

    let mut config = AppData::default();
    config.v2ray_profiles.push(V2RayProfile {
        id: "test-node".to_string(),
        name: "Windows Direct Node".to_string(),
        config: "vless://test@104.16.0.1:443".to_string(),
        created_at: "2026-10-23".to_string(),
    });

    let json = serde_json::to_string_pretty(&config).expect("Serialize config");
    std::fs::write(&config_path, json).expect("Write test config");

    let loaded_content = std::fs::read_to_string(&config_path).expect("Read test config");
    let loaded: AppData = serde_json::from_str(&loaded_content).expect("Deserialize config");

    assert!(loaded.v2ray_profiles.len() >= 2);
    assert_eq!(loaded.v2ray_profiles[1].name, "Windows Direct Node");
    assert!(loaded.enable_tun);
    assert!(!loaded.enable_sysproxy);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_windows_pending_release_notes_integration() {
    use netray_windows::storage::{
        PendingReleaseNotes, load_and_clear_pending_release_notes_from, save_pending_release_notes_to,
    };

    let temp_dir = std::env::temp_dir().join(format!(
        "netray_win_notes_{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    let _ = std::fs::create_dir_all(&temp_dir);

    let notes = PendingReleaseNotes {
        version: "1.0.30".to_string(),
        release_name: "Netray v1.0.30".to_string(),
        notes: "### Auto Update\n- Linux privileged updater\n- Windows architecture readiness".to_string(),
    };

    assert!(save_pending_release_notes_to(&temp_dir, &notes).is_ok());

    let fetched = load_and_clear_pending_release_notes_from(&temp_dir);
    assert_eq!(fetched, Some(notes));

    // File should have been deleted upon consumption
    let empty = load_and_clear_pending_release_notes_from(&temp_dir);
    assert!(empty.is_none());

    let _ = std::fs::remove_dir_all(&temp_dir);
}
