use netray_windows::updater::{
    format_msi_exit_code, is_newer_version, parse_manifest_json, parse_version, verify_file_sha256,
    InstallChannel, UpdateState,
};
use std::io::Write;

#[test]
fn test_store_channel_contract() {
    let channel = InstallChannel::Store;
    // Store channel must never allow self updates
    assert!(!channel.allows_self_update());
    assert_eq!(channel.as_str(), "store");
    assert_eq!(channel.badge_label(), "STORE");
    assert_eq!(channel.display_name(), "Microsoft Store");

    // UI state for Store
    let state = UpdateState::StoreManaged;
    assert_eq!(state, UpdateState::StoreManaged);
}

#[test]
fn test_portable_unknown_channel_contract() {
    let channel = InstallChannel::Unknown;
    // Unknown (portable) channel must never self-update
    assert!(!channel.allows_self_update());
    assert_eq!(channel.as_str(), "unknown");
    assert_eq!(channel.badge_label(), "PORTABLE");

    let state = UpdateState::ManualDownloadOnly {
        url: "https://github.com/venujatech-byte/Netray/releases/latest".into(),
    };
    if let UpdateState::ManualDownloadOnly { url } = state {
        assert!(url.contains("releases/latest"));
    } else {
        panic!("Expected ManualDownloadOnly state");
    }
}

#[test]
fn test_channel_asset_isolation_msi_vs_exe() {
    let manifest_json = r#"{
        "version": "2.0.5",
        "release_date": "2026-11-23T12:00:00Z",
        "notes": "Feature update",
        "platforms": {
            "windows-x64-msi": {
                "url": "https://github.com/venujatech-byte/Netray/releases/download/v2.0.5/netray-windows-2.0.5.msi",
                "sha256": "aaaa1111222233334444555566667777888899990000aaaabbbbccccddddeeee",
                "size": 18500000
            },
            "windows-x64-exe": {
                "url": "https://github.com/venujatech-byte/Netray/releases/download/v2.0.5/netray-windows-setup-2.0.5.exe",
                "sha256": "bbbb1111222233334444555566667777888899990000aaaabbbbccccddddeeee",
                "size": 18200000
            }
        }
    }"#;

    // 1. MSI install querying manifest must select only the MSI package
    let msi_res = parse_manifest_json(manifest_json, "2.0.0", InstallChannel::Msi)
        .expect("Parse manifest")
        .expect("Release found");
    assert_eq!(msi_res.version, "2.0.5");
    assert!(msi_res.download_url.ends_with(".msi"));
    assert_eq!(
        msi_res.expected_sha256.as_deref(),
        Some("aaaa1111222233334444555566667777888899990000aaaabbbbccccddddeeee")
    );
    assert_eq!(msi_res.channel, InstallChannel::Msi);

    // 2. Setup EXE install querying manifest must select only the EXE package
    let exe_res = parse_manifest_json(manifest_json, "2.0.0", InstallChannel::Exe)
        .expect("Parse manifest")
        .expect("Release found");
    assert_eq!(exe_res.version, "2.0.5");
    assert!(exe_res.download_url.ends_with(".exe"));
    assert_eq!(
        exe_res.expected_sha256.as_deref(),
        Some("bbbb1111222233334444555566667777888899990000aaaabbbbccccddddeeee")
    );
    assert_eq!(exe_res.channel, InstallChannel::Exe);

    // 3. Store channel must never match any manifest entry
    let store_res = parse_manifest_json(manifest_json, "2.0.0", InstallChannel::Store)
        .expect("Parse manifest");
    assert!(store_res.is_none());

    // 4. Unknown channel must never match any manifest entry
    let unknown_res = parse_manifest_json(manifest_json, "2.0.0", InstallChannel::Unknown)
        .expect("Parse manifest");
    assert!(unknown_res.is_none());
}

#[test]
fn test_sha256_verification_and_tamper_rejection() {
    let temp_dir = std::env::temp_dir().join(format!(
        "netray_hash_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let file_path = temp_dir.join("test_installer.msi");

    // Write sample installer content
    let content = b"Netray MSI Installer Payload Data 1234567890";
    {
        let mut f = std::fs::File::create(&file_path).unwrap();
        f.write_all(content).unwrap();
    }

    // SHA-256 of the above content:
    // sha256("Netray MSI Installer Payload Data 1234567890")
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content);
    let expected_hash = format!("{:x}", hasher.finalize());

    // 1. Valid hash verification
    assert!(verify_file_sha256(&file_path, &expected_hash).is_ok());

    // 2. Tampered hash must be rejected
    let tampered_hash = "0000000000000000000000000000000000000000000000000000000000000000";
    let err = verify_file_sha256(&file_path, tampered_hash);
    assert!(err.is_err());
    assert!(err.unwrap_err().contains("SHA-256 checksum mismatch"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_version_parsing_and_downgrade_prevention() {
    assert_eq!(parse_version("1.0.4"), Some((1, 0, 4)));
    assert_eq!(parse_version("v2.0.10"), Some((2, 0, 10)));
    assert_eq!(parse_version("2.1.0-rc1"), Some((2, 1, 0)));

    // Strictly newer is allowed
    assert!(is_newer_version("1.0.4", "1.0.3"));
    assert!(is_newer_version("2.0.0", "1.9.9"));
    assert!(is_newer_version("2.0.1", "2.0.0"));

    // Equal version is refused
    assert!(!is_newer_version("1.0.4", "1.0.4"));
    assert!(!is_newer_version("2.0.0", "2.0.0"));

    // Downgrades are strictly refused
    assert!(!is_newer_version("1.0.3", "1.0.4"));
    assert!(!is_newer_version("1.9.9", "2.0.0"));
}

#[test]
fn test_msi_exit_code_mapping() {
    assert_eq!(format_msi_exit_code(0), "Installation completed successfully.");
    assert_eq!(format_msi_exit_code(1602), "Installation was cancelled by the user.");
    assert_eq!(
        format_msi_exit_code(1603),
        "Fatal error during installation. Check MSI log for details."
    );
    assert_eq!(
        format_msi_exit_code(3010),
        "Installation completed successfully. System restart is required."
    );
    assert!(format_msi_exit_code(1618).contains("Another installation"));
    assert!(format_msi_exit_code(9999).contains("unhandled code"));
}

