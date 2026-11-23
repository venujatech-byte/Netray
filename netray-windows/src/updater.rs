//! Windows In-App Auto-Updater
//!
//! Enforces strict install channel isolation:
//! 1. MSI install -> updates only by downloading and executing a newer .msi via msiexec
//! 2. Setup EXE install -> updates only by downloading and running a newer setup .exe
//! 3. Microsoft Store (MSIX) -> self-updater is completely disabled; Store manages updates
//! 4. Unknown (Portable / Dev) -> no auto-update; manual download only
//!
//! Downloaded installers are cryptographically verified against SHA-256 hashes
//! from the release manifest before execution.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const GITHUB_REPO: &str = "venujatech-byte/Netray";
pub const MANIFEST_URL: &str = "https://github.com/venujatech-byte/Netray/releases/latest/download/latest.json";
pub const RELEASES_API_URL: &str = "https://api.github.com/repos/venujatech-byte/Netray/releases/latest";
pub const RELEASES_WEB_URL: &str = "https://github.com/venujatech-byte/Netray/releases/latest";

/// Opens a web URL in the user's default browser
pub fn open_browser(url: &str) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = std::process::Command::new("rundll32");
        cmd.args(["url.dll,FileProtocolHandler", url]);
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        let _ = cmd.spawn();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

/// Installation channel detected for the running application
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstallChannel {
    Msi,
    Exe,
    Store,
    Unknown,
}

impl InstallChannel {
    pub fn as_str(&self) -> &'static str {
        match self {
            InstallChannel::Msi => "msi",
            InstallChannel::Exe => "exe",
            InstallChannel::Store => "store",
            InstallChannel::Unknown => "unknown",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            InstallChannel::Msi => "Windows Installer (MSI)",
            InstallChannel::Exe => "Setup Installer (EXE)",
            InstallChannel::Store => "Microsoft Store",
            InstallChannel::Unknown => "Standalone / Portable",
        }
    }

    pub fn badge_label(&self) -> &'static str {
        match self {
            InstallChannel::Msi => "MSI",
            InstallChannel::Exe => "EXE",
            InstallChannel::Store => "STORE",
            InstallChannel::Unknown => "PORTABLE",
        }
    }

    pub fn allows_self_update(&self) -> bool {
        matches!(self, InstallChannel::Msi | InstallChannel::Exe)
    }
}

/// Official manifest schema published on GitHub Releases (`latest.json`)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateManifest {
    pub version: String,
    #[serde(default)]
    pub release_date: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub platforms: HashMap<String, PlatformAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformAsset {
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseInfo {
    pub version: String,
    pub tag_name: String,
    pub release_name: String,
    pub release_notes: String,
    pub download_url: String,
    pub asset_name: String,
    pub published_at: String,
    pub expected_sha256: Option<String>,
    pub channel: InstallChannel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateState {
    Idle,
    Checking,
    Available(ReleaseInfo),
    UpToDate(String),
    Downloading { version: String, status: String },
    Downloaded { info: ReleaseInfo, staged_binary: PathBuf },
    Applying,
    ReadyToRestart { version: String },
    StoreManaged,
    ManualDownloadOnly { url: String },
    Error(String),
}

pub fn clean_markdown_inline(line: &str) -> String {
    line.replace("**", "").replace("__", "").replace('`', "")
}

/// Parses a semver-like string ("2.0.1" or "v2.0.1") into (major, minor, patch).
pub fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let clean = v.trim().trim_start_matches(|c| c == 'v' || c == 'V');
    let mut parts = clean.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.split('-').next()?.parse().ok()?;
    Some((major, minor, patch))
}

/// Returns true if `remote` is strictly newer than `local`. Refuses downgrades.
pub fn is_newer_version(remote: &str, local: &str) -> bool {
    match (parse_version(remote), parse_version(local)) {
        (Some(r), Some(l)) => r > l,
        _ => false,
    }
}

// ============================================================================
// Channel Detection
// ============================================================================

/// Detects the installation channel at startup in strict priority order:
/// 1. MSIX package identity (Store)
/// 2. Registry InstallChannel marker (MSI or EXE)
/// 3. Unknown (Portable / Dev run)
pub fn detect_install_channel() -> InstallChannel {
    // 1. Check MSIX package identity
    if is_running_as_msix() {
        return InstallChannel::Store;
    }

    // 2. Check Registry InstallChannel marker (HKLM first for perMachine, then HKCU)
    if let Some(ch) = read_registry_channel() {
        return ch;
    }

    // 3. Fallback: Unknown
    InstallChannel::Unknown
}

/// Checks whether the running process has MSIX package identity via `GetCurrentPackageFullName`
#[cfg(target_os = "windows")]
pub fn is_running_as_msix() -> bool {
    type GetCurrentPackageFullNameFn = unsafe extern "system" fn(*mut u32, *mut u16) -> i32;
    unsafe {
        extern "system" {
            fn GetModuleHandleA(lpModuleName: *const u8) -> *mut std::ffi::c_void;
            fn GetProcAddress(hModule: *mut std::ffi::c_void, lpProcName: *const u8) -> *mut std::ffi::c_void;
        }

        let kernel32 = GetModuleHandleA(b"kernel32.dll\0".as_ptr());
        if kernel32.is_null() {
            return false;
        }
        let proc = GetProcAddress(kernel32, b"GetCurrentPackageFullName\0".as_ptr());
        if proc.is_null() {
            return false;
        }
        let get_package_name: GetCurrentPackageFullNameFn = std::mem::transmute(proc);
        let mut len: u32 = 0;
        let res = get_package_name(&mut len, std::ptr::null_mut());
        // 0 = ERROR_SUCCESS, 122 = ERROR_INSUFFICIENT_BUFFER (both mean package identity is present)
        // 15700 = APPMODEL_ERROR_NO_PACKAGE (meaning unpackaged Win32 application)
        res == 0 || res == 122
    }
}

#[cfg(not(target_os = "windows"))]
pub fn is_running_as_msix() -> bool {
    false
}

/// Reads the `InstallChannel` string value from Windows Registry
fn read_registry_channel() -> Option<InstallChannel> {
    #[cfg(target_os = "windows")]
    {
        for key in &["HKLM\\Software\\Netray", "HKCU\\Software\\Netray"] {
            if let Some(ch) = query_reg_channel(key) {
                return Some(ch);
            }
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn query_reg_channel(key: &str) -> Option<InstallChannel> {
    use std::os::windows::process::CommandExt;
    let mut cmd = std::process::Command::new("reg");
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    let output = cmd.args(["query", key, "/v", "InstallChannel"]).output().ok()?;
    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            if line.contains("InstallChannel") {
                let parts: Vec<&str> = line.split("REG_SZ").collect();
                if parts.len() >= 2 {
                    let val = parts[1].trim();
                    if val.eq_ignore_ascii_case("msi") {
                        return Some(InstallChannel::Msi);
                    } else if val.eq_ignore_ascii_case("exe") {
                        return Some(InstallChannel::Exe);
                    }
                }
            }
        }
    }
    None
}

// ============================================================================
// Update Checking & Feed Parsing
// ============================================================================

/// Checks for updates for the detected channel.
/// - Store builds immediately return Ok(None) without any network activity.
/// - Unknown builds return Ok(None) (manual download).
/// - MSI and EXE builds query the manifest and match their exact channel asset.
pub async fn check_for_updates(current_version: &str) -> Result<Option<ReleaseInfo>, String> {
    let channel = detect_install_channel();

    // The Store contract: The app NEVER self-updates; no network call is made
    if channel == InstallChannel::Store {
        return Ok(None);
    }

    if channel == InstallChannel::Unknown {
        return Ok(None);
    }

    let curr = current_version.to_string();
    tokio::task::spawn_blocking(move || {
        // 1. Try reading the dedicated update manifest (latest.json)
        match fetch_manifest_update(&curr, channel) {
            Ok(Some(info)) => return Ok(Some(info)),
            Ok(None) => return Ok(None),
            Err(_) => {
                // Fallback: If latest.json is not yet present on older releases, query GitHub API
            }
        }

        // 2. Fallback to GitHub Releases API with strict channel asset matching
        fetch_github_api_update(&curr, channel)
    })
    .await
    .unwrap_or_else(|e| Err(format!("Update check execution error: {e}")))
}

/// Fetches and parses the dedicated `latest.json` manifest
fn fetch_manifest_update(current_version: &str, channel: InstallChannel) -> Result<Option<ReleaseInfo>, String> {
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new("curl");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let output = cmd
        .args([
            "-sL",
            "-f",
            "-m",
            "10",
            "-H",
            "User-Agent: Netray-Windows",
            MANIFEST_URL,
        ])
        .output()
        .map_err(|e| format!("Could not reach update manifest: {e}"))?;

    if !output.status.success() {
        return Err(format!("Manifest request failed: code {:?}", output.status.code()));
    }

    let body = String::from_utf8_lossy(&output.stdout);
    parse_manifest_json(&body, current_version, channel)
}

/// Parses the manifest JSON and extracts the exact asset for the current channel & architecture
pub fn parse_manifest_json(
    json_str: &str,
    current_version: &str,
    channel: InstallChannel,
) -> Result<Option<ReleaseInfo>, String> {
    let manifest: UpdateManifest = serde_json::from_str(json_str)
        .map_err(|e| format!("Failed to parse update manifest JSON: {e}"))?;

    if !is_newer_version(&manifest.version, current_version) {
        return Ok(None);
    }

    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };

    let platform_key = match channel {
        InstallChannel::Msi => format!("windows-{arch}-msi"),
        InstallChannel::Exe => format!("windows-{arch}-exe"),
        InstallChannel::Store | InstallChannel::Unknown => return Ok(None),
    };

    let asset = manifest.platforms.get(&platform_key).ok_or_else(|| {
        format!("Manifest contains no update package for platform '{platform_key}'")
    })?;

    let asset_name = asset
        .url
        .split('/')
        .last()
        .unwrap_or("update_installer")
        .to_string();

    Ok(Some(ReleaseInfo {
        version: manifest.version.clone(),
        tag_name: format!("v{}", manifest.version),
        release_name: format!("Netray v{}", manifest.version),
        release_notes: manifest.notes,
        download_url: asset.url.clone(),
        asset_name,
        published_at: manifest.release_date,
        expected_sha256: Some(asset.sha256.clone()),
        channel,
    }))
}

/// Fallback updater querying GitHub Releases API with strict channel matching
fn fetch_github_api_update(current_version: &str, channel: InstallChannel) -> Result<Option<ReleaseInfo>, String> {
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new("curl");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let output = cmd
        .args([
            "-sL",
            "-f",
            "-m",
            "10",
            "-H",
            "User-Agent: Netray-Windows",
            RELEASES_API_URL,
        ])
        .output()
        .map_err(|e| format!("GitHub API request failed: {e}"))?;

    if !output.status.success() {
        return Err(format!("GitHub API returned error code {:?}", output.status.code()));
    }

    let body = String::from_utf8_lossy(&output.stdout);
    parse_github_api_release_for_channel(&body, current_version, channel)
}

pub fn parse_github_api_release_for_channel(
    json_str: &str,
    current_version: &str,
    channel: InstallChannel,
) -> Result<Option<ReleaseInfo>, String> {
    let val: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("Invalid GitHub API response: {e}"))?;

    let tag_name = val["tag_name"]
        .as_str()
        .ok_or("Release is missing tag_name")?
        .to_string();
    let release_name = val["name"].as_str().unwrap_or(&tag_name).to_string();
    let release_notes = val["body"].as_str().unwrap_or("").to_string();
    let published_at = val["published_at"].as_str().unwrap_or("").to_string();
    let version = tag_name
        .trim_start_matches(|c| c == 'v' || c == 'V')
        .to_string();

    if !is_newer_version(&version, current_version) {
        return Ok(None);
    }

    let assets = val["assets"]
        .as_array()
        .ok_or("Release has no assets array")?;

    let mut matched_url = None;
    let mut matched_name = None;

    for asset in assets {
        let name = asset["name"].as_str().unwrap_or("");
        let url = asset["browser_download_url"].as_str().unwrap_or("");
        if name.is_empty() || url.is_empty() {
            continue;
        }

        let name_lower = name.to_lowercase();
        match channel {
            InstallChannel::Msi => {
                if name_lower.ends_with(".msi") && name_lower.contains("windows") {
                    matched_url = Some(url.to_string());
                    matched_name = Some(name.to_string());
                    break;
                }
            }
            InstallChannel::Exe => {
                if name_lower.ends_with(".exe") && (name_lower.contains("setup") || name_lower.contains("installer")) {
                    matched_url = Some(url.to_string());
                    matched_name = Some(name.to_string());
                    break;
                }
            }
            InstallChannel::Store | InstallChannel::Unknown => return Ok(None),
        }
    }

    let (download_url, asset_name) = match (matched_url, matched_name) {
        (Some(u), Some(n)) => (u, n),
        _ => return Err(format!("No matching installer found for channel '{}'", channel.display_name())),
    };

    Ok(Some(ReleaseInfo {
        version,
        tag_name,
        release_name,
        release_notes,
        download_url,
        asset_name,
        published_at,
        expected_sha256: None,
        channel,
    }))
}

// ============================================================================
// Download, Integrity Verification & Installation
// ============================================================================

/// Downloads the channel-specific update installer and cryptographically verifies its SHA-256
pub async fn download_and_extract_update(info: ReleaseInfo) -> Result<PathBuf, String> {
    tokio::task::spawn_blocking(move || {
        let update_dir = std::env::temp_dir().join(format!("netray_update_{}", info.version));
        let _ = std::fs::remove_dir_all(&update_dir);
        std::fs::create_dir_all(&update_dir)
            .map_err(|e| format!("Failed to create temp update dir: {e}"))?;

        let installer_path = update_dir.join(&info.asset_name);

        // 1. Download installer via curl
        #[allow(unused_mut)]
        let mut curl_cmd = std::process::Command::new("curl");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            curl_cmd.creation_flags(0x08000000);
        }
        let curl_status = curl_cmd
            .args([
                "-sL",
                "-f",
                "-m",
                "180",
                "-o",
                &installer_path.to_string_lossy(),
                &info.download_url,
            ])
            .status()
            .map_err(|e| format!("Failed to download update installer: {e}"))?;

        if !curl_status.success() || !installer_path.exists() {
            return Err("Failed to download update package from GitHub.".to_string());
        }

        // 2. Cryptographic SHA-256 verification
        if let Some(expected_hash) = &info.expected_sha256 {
            verify_file_sha256(&installer_path, expected_hash)?;
        }

        // 3. Authenticode signature verification (if signed on Windows)
        verify_authenticode_signature_if_signed(&installer_path)?;

        Ok(installer_path)
    })
    .await
    .unwrap_or_else(|e| Err(format!("Download execution error: {e}")))
}

/// Computes the SHA-256 hex digest of a local file
pub fn compute_file_sha256(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)
        .map_err(|e| format!("Cannot open file for checksum calculation: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer)
            .map_err(|e| format!("Failed reading file for checksum: {e}"))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Verifies that the file at `path` matches `expected_hex`.
/// If the checksum does not match, the file is deleted immediately to prevent running tampered payload.
pub fn verify_file_sha256(path: &Path, expected_hex: &str) -> Result<(), String> {
    let actual_hash = compute_file_sha256(path)?;
    if !actual_hash.eq_ignore_ascii_case(expected_hex.trim()) {
        let _ = std::fs::remove_file(path);
        return Err(format!(
            "Integrity check failed: SHA-256 checksum mismatch!\nExpected: {}\nActual:   {}",
            expected_hex.trim(),
            actual_hash
        ));
    }
    Ok(())
}

/// Verifies Authenticode digital signature on Windows if signed
fn verify_authenticode_signature_if_signed(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut ps_cmd = std::process::Command::new("powershell");
        ps_cmd.creation_flags(0x08000000);
        let path_str = path.to_string_lossy().replace('\'', "''");
        let script = format!("(Get-AuthenticodeSignature -FilePath '{}').Status.ToString()", path_str);
        if let Ok(output) = ps_cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]).output() {
            if output.status.success() {
                let status = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if status == "HashMismatch" || status == "BadDigest" {
                    let _ = std::fs::remove_file(path);
                    return Err(format!("Authenticode verification failed: signature is invalid ({status})"));
                }
            }
        }
    }
    let _ = path;
    Ok(())
}

/// Prepares system before installer execution:
/// Cleanly stops core process and restores system proxy so no locks or adapter conflicts occur
pub fn prepare_system_before_installer() {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // Kill xray.exe cores
        let mut kill_cmd = std::process::Command::new("taskkill");
        kill_cmd.creation_flags(0x08000000);
        kill_cmd.args(["/F", "/T", "/IM", "xray.exe"]);
        let _ = kill_cmd.output();

        // Restore Windows Registry system proxy settings to avoid orphan proxy
        let mut reg_cmd = std::process::Command::new("reg");
        reg_cmd.creation_flags(0x08000000);
        reg_cmd.args([
            "add",
            "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
            "/v",
            "ProxyEnable",
            "/t",
            "REG_DWORD",
            "/d",
            "0",
            "/f",
        ]);
        let _ = reg_cmd.output();
    }
}

/// Applies the update according to the detected install channel and exits the app
pub fn apply_update_and_restart(
    installer_path: &Path,
    pending_notes: Option<&crate::storage::PendingReleaseNotes>,
) -> Result<(), String> {
    if let Some(notes) = pending_notes {
        let _ = crate::storage::save_pending_release_notes(notes);
    }

    let channel = detect_install_channel();

    match channel {
        InstallChannel::Msi => apply_msi_update(installer_path),
        InstallChannel::Exe => apply_exe_update(installer_path),
        InstallChannel::Store => {
            Err("Microsoft Store installations cannot be updated through the in-app updater.".to_string())
        }
        InstallChannel::Unknown => {
            Err("Standalone/portable installations do not support automated installer execution.".to_string())
        }
    }
}

/// Launches `msiexec.exe /i <file> /qb /l*v <log>` elevated and exits
fn apply_msi_update(installer_path: &Path) -> Result<(), String> {
    prepare_system_before_installer();

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let log_path = std::env::temp_dir().join("netray_msi_update.log");
        let msi_str = installer_path.to_string_lossy();
        let log_str = log_path.to_string_lossy();

        let ps_script = format!(
            "Start-Process -FilePath 'msiexec.exe' -ArgumentList @('/i', '{}', '/qb', '/l*v', '{}') -Verb RunAs",
            msi_str.replace('\'', "''"),
            log_str.replace('\'', "''")
        );

        let mut cmd = std::process::Command::new("powershell");
        cmd.creation_flags(0x08000000);
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", &ps_script])
            .spawn()
            .map_err(|e| format!("Failed to spawn elevated msiexec installer: {e}"))?;

        // Exit running app so all files in Program Files\Netray are unlocked
        std::process::exit(0);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = installer_path;
        Err("MSI update can only be run on Windows.".to_string())
    }
}

/// Launches setup EXE with silent flags elevated and exits
fn apply_exe_update(installer_path: &Path) -> Result<(), String> {
    prepare_system_before_installer();

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let exe_str = installer_path.to_string_lossy();

        // Support both Inno Setup (/VERYSILENT /SUPPRESSMSGBOXES /NORESTART) and NSIS (/S)
        let ps_script = format!(
            "Start-Process -FilePath '{}' -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/S') -Verb RunAs",
            exe_str.replace('\'', "''")
        );

        let mut cmd = std::process::Command::new("powershell");
        cmd.creation_flags(0x08000000);
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", &ps_script])
            .spawn()
            .map_err(|e| format!("Failed to spawn elevated setup installer: {e}"))?;

        std::process::exit(0);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = installer_path;
        Err("Setup EXE update can only be run on Windows.".to_string())
    }
}

/// Maps Windows Installer (msiexec) exit codes to friendly user messages
pub fn format_msi_exit_code(code: i32) -> &'static str {
    match code {
        0 => "Installation completed successfully.",
        1602 => "Installation was cancelled by the user.",
        1603 => "Fatal error during installation. Check MSI log for details.",
        1618 => "Another installation is already in progress. Complete that installation first.",
        1619 => "Installation package could not be opened.",
        3010 => "Installation completed successfully. System restart is required.",
        _ => "Installation exited with an unhandled code.",
    }
}

/// Cleanup any stale .old binary or temporary installer downloads left behind.
pub fn cleanup_stale_update_files() {
    if let Ok(exe) = std::env::current_exe() {
        let old = exe.with_extension("old");
        if old.exists() {
            let _ = std::fs::remove_file(old);
        }
    }
    let temp_pattern = std::env::temp_dir();
    if let Ok(entries) = std::fs::read_dir(temp_pattern) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with("netray_update_")
                    && (name.ends_with(".msi") || name.ends_with(".exe") || name.ends_with(".tmp"))
                {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_parsing() {
        assert_eq!(parse_version("1.0.4"), Some((1, 0, 4)));
        assert_eq!(parse_version("v2.0.1"), Some((2, 0, 1)));
        assert_eq!(parse_version("2.1.0-beta.1"), Some((2, 1, 0)));
        assert_eq!(parse_version("invalid"), None);
    }

    #[test]
    fn test_version_comparison() {
        assert!(is_newer_version("2.0.1", "2.0.0"));
        assert!(is_newer_version("2.1.0", "2.0.9"));
        assert!(!is_newer_version("2.0.0", "2.0.0"));
        assert!(!is_newer_version("1.9.9", "2.0.0")); // Refuse downgrade
    }

    #[test]
    fn test_store_channel_never_updates() {
        let channel = InstallChannel::Store;
        assert!(!channel.allows_self_update());
        assert_eq!(channel.badge_label(), "STORE");
        assert_eq!(channel.display_name(), "Microsoft Store");
    }

    #[test]
    fn test_msi_channel_only_selects_msi() {
        let manifest_json = r#"{
            "version": "2.0.1",
            "release_date": "2026-11-23T00:00:00Z",
            "notes": "Bug fixes",
            "platforms": {
                "windows-x64-msi": {
                    "url": "https://github.com/venujatech-byte/Netray/releases/download/v2.0.1/netray-windows-2.0.1.msi",
                    "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                    "size": 15000000
                },
                "windows-x64-exe": {
                    "url": "https://github.com/venujatech-byte/Netray/releases/download/v2.0.1/netray-windows-setup-2.0.1.exe",
                    "sha256": "1111111111111111111111111111111111111111111111111111111111111111",
                    "size": 15000000
                }
            }
        }"#;

        let res = parse_manifest_json(manifest_json, "2.0.0", InstallChannel::Msi).unwrap().unwrap();
        assert_eq!(res.version, "2.0.1");
        assert!(res.download_url.ends_with(".msi"));
        assert_eq!(res.expected_sha256, Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into()));
        assert_eq!(res.channel, InstallChannel::Msi);
    }

    #[test]
    fn test_exe_channel_only_selects_exe() {
        let manifest_json = r#"{
            "version": "2.0.1",
            "release_date": "2026-11-23T00:00:00Z",
            "notes": "Bug fixes",
            "platforms": {
                "windows-x64-msi": {
                    "url": "https://github.com/venujatech-byte/Netray/releases/download/v2.0.1/netray-windows-2.0.1.msi",
                    "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                    "size": 15000000
                },
                "windows-x64-exe": {
                    "url": "https://github.com/venujatech-byte/Netray/releases/download/v2.0.1/netray-windows-setup-2.0.1.exe",
                    "sha256": "1111111111111111111111111111111111111111111111111111111111111111",
                    "size": 15000000
                }
            }
        }"#;

        let res = parse_manifest_json(manifest_json, "2.0.0", InstallChannel::Exe).unwrap().unwrap();
        assert_eq!(res.version, "2.0.1");
        assert!(res.download_url.ends_with(".exe"));
        assert_eq!(res.expected_sha256, Some("1111111111111111111111111111111111111111111111111111111111111111".into()));
        assert_eq!(res.channel, InstallChannel::Exe);
    }

    #[test]
    fn test_msi_exit_code_mapping() {
        assert_eq!(format_msi_exit_code(0), "Installation completed successfully.");
        assert_eq!(format_msi_exit_code(1602), "Installation was cancelled by the user.");
        assert_eq!(format_msi_exit_code(1603), "Fatal error during installation. Check MSI log for details.");
        assert_eq!(format_msi_exit_code(3010), "Installation completed successfully. System restart is required.");
    }
}
