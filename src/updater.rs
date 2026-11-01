//! In-App Auto-Updater & Version Checker for Netray
//!
//! Checks GitHub CI Releases for newer versions of Netray, notifies the user in UI,
//! and provides one-click in-app download, staging, binary replacement, and application restart.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const GITHUB_REPO: &str = "venujatech-byte/Netray";
pub const RELEASES_API_URL: &str = "https://api.github.com/repos/venujatech-byte/Netray/releases/latest";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseInfo {
    pub version: String,
    pub tag_name: String,
    pub release_name: String,
    pub release_notes: String,
    pub download_url: String,
    pub asset_name: String,
    pub published_at: String,
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
    Error(String),
}

/// Parses a semver-like string ("0.1.26" or "v0.1.26") into (major, minor, patch).
pub fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let clean = v.trim().trim_start_matches(|c| c == 'v' || c == 'V');
    let mut parts = clean.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.split('-').next()?.parse().ok()?;
    Some((major, minor, patch))
}

/// Returns true if `remote` is strictly newer than `local`.
pub fn is_newer_version(remote: &str, local: &str) -> bool {
    match (parse_version(remote), parse_version(local)) {
        (Some(r), Some(l)) => r > l,
        _ => false,
    }
}

/// Parses the JSON payload from GitHub releases/latest into a ReleaseInfo matching the current OS.
pub fn parse_github_release_json(json_str: &str, target_os: &str) -> Result<ReleaseInfo, String> {
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

    let assets = val["assets"]
        .as_array()
        .ok_or("Release has no assets array")?;

    let (asset_name, download_url) = find_matching_asset(assets, target_os)?;

    Ok(ReleaseInfo {
        version,
        tag_name,
        release_name,
        release_notes,
        download_url,
        asset_name,
        published_at,
    })
}

fn find_matching_asset(
    assets: &[serde_json::Value],
    target_os: &str,
) -> Result<(String, String), String> {
    for asset in assets {
        let name = asset["name"].as_str().unwrap_or("");
        let url = asset["browser_download_url"].as_str().unwrap_or("");
        if url.is_empty() {
            continue;
        }

        let name_lower = name.to_lowercase();
        if target_os == "windows" {
            // e.g. netray-windows-0.1.26.zip
            if name_lower.contains("windows") && name_lower.ends_with(".zip") {
                return Ok((name.to_string(), url.to_string()));
            }
        } else {
            // Linux: e.g. netray-linux-linux-64-0.1.26.tar.gz
            if name_lower.contains("linux") && name_lower.ends_with(".tar.gz") {
                return Ok((name.to_string(), url.to_string()));
            }
        }
    }

    // Fallback: search for any .zip on Windows or .tar.gz on Linux
    for asset in assets {
        let name = asset["name"].as_str().unwrap_or("");
        let url = asset["browser_download_url"].as_str().unwrap_or("");
        if url.is_empty() {
            continue;
        }
        let name_lower = name.to_lowercase();
        if target_os == "windows" && name_lower.ends_with(".zip") {
            return Ok((name.to_string(), url.to_string()));
        } else if target_os != "windows" && name_lower.ends_with(".tar.gz") {
            return Ok((name.to_string(), url.to_string()));
        }
    }

    Err(format!(
        "No compatible release package found for target OS: {target_os}"
    ))
}

/// Checks GitHub for the latest release and returns ReleaseInfo if a newer version is available.
pub async fn check_for_updates(current_version: &str) -> Result<Option<ReleaseInfo>, String> {
    tokio::task::spawn_blocking({
        let curr = current_version.to_string();
        move || {
            let target_os = if cfg!(target_os = "windows") {
                "windows"
            } else {
                "linux"
            };

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
                    "-m",
                    "10",
                    "-H",
                    "User-Agent: Netray-App",
                    RELEASES_API_URL,
                ])
                .output()
                .map_err(|e| format!("Could not reach GitHub update server: {e}"))?;

            if !output.status.success() {
                return Err(format!(
                    "GitHub API request failed with status {:?}",
                    output.status.code()
                ));
            }

            let body = String::from_utf8_lossy(&output.stdout);
            let release = parse_github_release_json(&body, target_os)?;

            if is_newer_version(&release.version, &curr) {
                Ok(Some(release))
            } else {
                Ok(None)
            }
        }
    })
    .await
    .unwrap_or_else(|e| Err(format!("Task execution error: {e}")))
}

/// Downloads and extracts the update package into a staging directory, returning the staged binary path.
pub async fn download_and_extract_update(info: ReleaseInfo) -> Result<PathBuf, String> {
    tokio::task::spawn_blocking(move || {
        let update_dir = std::env::temp_dir().join(format!("netray_update_{}", info.version));
        let _ = std::fs::remove_dir_all(&update_dir);
        std::fs::create_dir_all(&update_dir)
            .map_err(|e| format!("Failed to create temp update dir: {e}"))?;

        let archive_path = update_dir.join(&info.asset_name);

        // 1. Download via curl
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
                &archive_path.to_string_lossy(),
                &info.download_url,
            ])
            .status()
            .map_err(|e| format!("Failed to download update artifact: {e}"))?;

        if !curl_status.success() || !archive_path.exists() {
            return Err("Failed to download release archive from GitHub.".to_string());
        }

        // 2. Extract
        let extract_dir = update_dir.join("extracted");
        std::fs::create_dir_all(&extract_dir)
            .map_err(|e| format!("Failed to create extract dir: {e}"))?;

        extract_archive(&archive_path, &extract_dir)?;

        // 3. Locate new binary
        let binary_name = if cfg!(target_os = "windows") {
            "netray-windows.exe"
        } else {
            "netray"
        };

        let found_binary = find_file_recursive(&extract_dir, binary_name)
            .or_else(|| {
                if cfg!(target_os = "windows") {
                    find_file_recursive(&extract_dir, "netray.exe")
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                format!("Updated executable '{binary_name}' was not found in release archive.")
            })?;

        // 4. Also copy updated auxiliary assets (xray, geoip.dat, geosite.dat) next to the staged binary
        copy_auxiliary_assets(&extract_dir, &found_binary);

        Ok(found_binary)
    })
    .await
    .unwrap_or_else(|e| Err(format!("Task execution error: {e}")))
}

fn extract_archive(archive_path: &Path, extract_dir: &Path) -> Result<(), String> {
    let archive_str = archive_path.to_string_lossy();
    let extract_str = extract_dir.to_string_lossy();

    #[cfg(target_os = "windows")]
    {
        // Try native tar.exe (available in Windows 10/11)
        #[allow(unused_mut)]
        let mut tar_cmd = std::process::Command::new("tar");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            tar_cmd.creation_flags(0x08000000);
        }
        let tar_res = tar_cmd
            .args(["-xf", &archive_str, "-C", &extract_str])
            .status();

        if matches!(tar_res, Ok(s) if s.success()) {
            return Ok(());
        }

        // Fallback: PowerShell Expand-Archive
        let ps_script = format!(
            "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
            archive_str.replace('\'', "''"),
            extract_str.replace('\'', "''")
        );
        #[allow(unused_mut)]
        let mut ps_cmd = std::process::Command::new("powershell");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            ps_cmd.creation_flags(0x08000000);
        }
        let ps_res = ps_cmd
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps_script])
            .status()
            .map_err(|e| format!("Extraction failed: {e}"))?;

        if !ps_res.success() {
            return Err("PowerShell Expand-Archive failed to extract update package.".to_string());
        }
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    {
        let status = if archive_str.ends_with(".tar.gz") || archive_str.ends_with(".tgz") {
            std::process::Command::new("tar")
                .args(["-xzf", &archive_str, "-C", &extract_str])
                .status()
        } else if archive_str.ends_with(".zip") {
            std::process::Command::new("unzip")
                .args(["-oq", &archive_str, "-d", &extract_str])
                .status()
        } else {
            return Err("Unsupported archive extension for Linux update.".to_string());
        };

        match status {
            Ok(s) if s.success() => Ok(()),
            Ok(s) => Err(format!("Archive extraction exited with code {:?}", s.code())),
            Err(e) => Err(format!("Failed to invoke extraction command: {e}")),
        }
    }
}

fn find_file_recursive(dir: &Path, file_name: &str) -> Option<PathBuf> {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.eq_ignore_ascii_case(file_name) {
                        return Some(path);
                    }
                }
            } else if path.is_dir() {
                if let Some(found) = find_file_recursive(&path, file_name) {
                    return Some(found);
                }
            }
        }
    }
    None
}

fn copy_auxiliary_assets(extract_dir: &Path, staged_binary: &Path) {
    let target_dir = match staged_binary.parent() {
        Some(d) => d,
        None => return,
    };

    let files_to_sync = [
        "xray",
        "xray.exe",
        "wintun.dll",
        "geoip.dat",
        "geosite.dat",
    ];

    for file_name in files_to_sync {
        if let Some(src) = find_file_recursive(extract_dir, file_name) {
            let dest = target_dir.join(file_name);
            if src != dest {
                let _ = std::fs::copy(&src, &dest);
            }
        }
    }
}

/// Replaces the current running application executable with the staged binary and relaunches the app.
pub fn apply_update_and_restart(
    staged_binary: &Path,
    pending_notes: Option<&crate::storage::PendingReleaseNotes>,
) -> Result<(), String> {
    if let Some(notes) = pending_notes {
        let _ = crate::storage::save_pending_release_notes(notes);
    }

    let current_exe = std::env::current_exe()
        .map_err(|e| format!("Cannot determine current executable path: {e}"))?;

    let current_dir = current_exe
        .parent()
        .ok_or("Current executable has no parent directory")?;

    #[cfg(target_os = "windows")]
    {
        // On Windows NTFS, a running .exe cannot be overwritten in-place,
        // but it CAN be renamed!
        let old_exe = current_exe.with_extension("old");
        let _ = std::fs::remove_file(&old_exe);

        std::fs::rename(&current_exe, &old_exe)
            .map_err(|e| format!("Failed to rotate running executable to .old: {e}"))?;

        if let Err(e) = std::fs::copy(staged_binary, &current_exe) {
            // Rollback on failure
            let _ = std::fs::rename(&old_exe, &current_exe);
            return Err(format!("Failed to install new executable: {e}"));
        }

        // Copy auxiliary files (xray.exe, wintun.dll, dat files) if present
        if let Some(staged_dir) = staged_binary.parent() {
            for name in &["xray.exe", "wintun.dll", "geoip.dat", "geosite.dat"] {
                let src = staged_dir.join(name);
                if src.exists() {
                    let _ = std::fs::copy(&src, current_dir.join(name));
                }
            }
        }

        // Relaunch the new executable without console windows
        #[allow(unused_mut)]
        let mut cmd = std::process::Command::new(&current_exe);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }
        cmd.spawn()
            .map_err(|e| format!("Failed to relaunch updated Netray: {e}"))?;

        std::process::exit(0);
    }

    #[cfg(not(target_os = "windows"))]
    {
        let can_write_directly = {
            let probe = current_dir.join(format!(".netray_write_probe_{}", std::process::id()));
            match std::fs::File::create(&probe) {
                Ok(_) => {
                    let _ = std::fs::remove_file(probe);
                    true
                }
                Err(_) => false,
            }
        };

        if can_write_directly {
            // User has direct write permissions (e.g. portable build in home folder)
            let new_dest = current_exe.with_extension("new");
            std::fs::copy(staged_binary, &new_dest)
                .map_err(|e| format!("Failed to copy new binary: {e}"))?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&new_dest, std::fs::Permissions::from_mode(0o755));
            }

            std::fs::rename(&new_dest, &current_exe)
                .map_err(|e| format!("Failed to replace running executable: {e}"))?;

            if let Some(staged_dir) = staged_binary.parent() {
                for name in &["xray", "geoip.dat", "geosite.dat"] {
                    let src = staged_dir.join(name);
                    if src.exists() {
                        let dest = current_dir.join(name);
                        let _ = std::fs::copy(&src, &dest);
                        #[cfg(unix)]
                        if name == &"xray" {
                            use std::os::unix::fs::PermissionsExt;
                            let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
                        }
                    }
                }
            }

            std::process::Command::new(&current_exe)
                .spawn()
                .map_err(|e| format!("Failed to relaunch updated Netray: {e}"))?;

            std::process::exit(0);
        } else {
            // System-level installation (/usr/bin/netray or root directory).
            // Elevate via Polkit (pkexec) or sudo, and relaunch after old process exits.
            let app_pid = std::process::id();
            let staged_dir = staged_binary
                .parent()
                .ok_or("Staged binary has no parent directory")?;

            let helper_script = std::env::temp_dir().join(format!("netray_upgrade_{}.sh", app_pid));
            let root_install_script = std::env::temp_dir().join(format!("netray_root_{}.sh", app_pid));

            let root_script_content = format!(
                r#"#!/bin/sh
set -e
cp -f "{staged_binary}" "{target_exe}"
chmod 755 "{target_exe}"
if [ -f "{staged_dir}/xray" ] && [ -d "{target_dir}" ]; then
    cp -f "{staged_dir}/xray" "{target_dir}/xray" 2>/dev/null || true
    chmod 755 "{target_dir}/xray" 2>/dev/null || true
fi
if [ -f "{staged_dir}/geoip.dat" ] && [ -d "{target_dir}" ]; then
    cp -f "{staged_dir}/geoip.dat" "{target_dir}/geoip.dat" 2>/dev/null || true
fi
if [ -f "{staged_dir}/geosite.dat" ] && [ -d "{target_dir}" ]; then
    cp -f "{staged_dir}/geosite.dat" "{target_dir}/geosite.dat" 2>/dev/null || true
fi
exit 0
"#,
                staged_binary = staged_binary.to_string_lossy(),
                target_exe = current_exe.to_string_lossy(),
                staged_dir = staged_dir.to_string_lossy(),
                target_dir = current_dir.to_string_lossy(),
            );

            std::fs::write(&root_install_script, &root_script_content)
                .map_err(|e| format!("Failed to prepare root install script: {e}"))?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&root_install_script, std::fs::Permissions::from_mode(0o755));
            }

            let helper_script_content = format!(
                r#"#!/bin/sh
PID="{app_pid}"
ROOT_SCRIPT="{root_script}"
TARGET_EXE="{target_exe}"

# Wait for running Netray instance to exit
while kill -0 "$PID" 2>/dev/null; do
    sleep 0.15
done

# Elevate using Polkit graphical prompt or sudo
if command -v pkexec >/dev/null 2>&1; then
    pkexec /bin/sh "$ROOT_SCRIPT"
elif command -v sudo >/dev/null 2>&1; then
    sudo /bin/sh "$ROOT_SCRIPT"
fi

rm -f "$ROOT_SCRIPT"

# Relaunch Netray as the desktop user
sleep 0.4
nohup "$TARGET_EXE" >/dev/null 2>&1 &
rm -f "$0"
exit 0
"#,
                app_pid = app_pid,
                root_script = root_install_script.to_string_lossy(),
                target_exe = current_exe.to_string_lossy(),
            );

            std::fs::write(&helper_script, &helper_script_content)
                .map_err(|e| format!("Failed to write update helper script: {e}"))?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&helper_script, std::fs::Permissions::from_mode(0o755));
            }

            std::process::Command::new("/bin/sh")
                .arg(&helper_script)
                .spawn()
                .map_err(|e| format!("Failed to launch upgrade helper: {e}"))?;

            std::process::exit(0);
        }
    }
}

/// Cleanup any stale .old binary left behind from a previous Windows upgrade.
pub fn cleanup_stale_update_files() {
    if let Ok(exe) = std::env::current_exe() {
        let old = exe.with_extension("old");
        if old.exists() {
            let _ = std::fs::remove_file(old);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_parsing() {
        assert_eq!(parse_version("0.1.26"), Some((0, 1, 26)));
        assert_eq!(parse_version("v0.1.26"), Some((0, 1, 26)));
        assert_eq!(parse_version("V1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("0.1.26-alpha"), Some((0, 1, 26)));
        assert_eq!(parse_version("invalid"), None);
    }

    #[test]
    fn test_version_comparison() {
        assert!(is_newer_version("0.1.26", "0.1.0"));
        assert!(is_newer_version("v0.1.26", "v0.1.25"));
        assert!(is_newer_version("1.0.0", "0.9.99"));
        assert!(!is_newer_version("0.1.26", "0.1.26"));
        assert!(!is_newer_version("v0.1.25", "v0.1.26"));
        assert!(!is_newer_version("0.1.0", "0.1.26"));
    }

    #[test]
    fn test_parse_github_release_json_windows() {
        let sample = r#"{
            "tag_name": "v0.1.26",
            "name": "Netray v0.1.26",
            "body": "Automated Netray release.",
            "published_at": "2026-10-31T14:00:36Z",
            "assets": [
                {
                    "name": "netray-windows-0.1.26.zip",
                    "browser_download_url": "https://github.com/venujatech-byte/Netray/releases/download/v0.1.26/netray-windows-0.1.26.zip"
                },
                {
                    "name": "netray-linux-linux-64-0.1.26.tar.gz",
                    "browser_download_url": "https://github.com/venujatech-byte/Netray/releases/download/v0.1.26/netray-linux-linux-64-0.1.26.tar.gz"
                }
            ]
        }"#;

        let rel = parse_github_release_json(sample, "windows").expect("Should parse windows asset");
        assert_eq!(rel.version, "0.1.26");
        assert_eq!(rel.tag_name, "v0.1.26");
        assert_eq!(rel.asset_name, "netray-windows-0.1.26.zip");
        assert!(rel.download_url.contains("netray-windows-0.1.26.zip"));
    }

    #[test]
    fn test_parse_github_release_json_linux() {
        let sample = r#"{
            "tag_name": "v0.1.26",
            "name": "Netray v0.1.26",
            "body": "Automated Netray release.",
            "published_at": "2026-10-31T14:00:36Z",
            "assets": [
                {
                    "name": "netray-windows-0.1.26.zip",
                    "browser_download_url": "https://github.com/venujatech-byte/Netray/releases/download/v0.1.26/netray-windows-0.1.26.zip"
                },
                {
                    "name": "netray-linux-linux-64-0.1.26.tar.gz",
                    "browser_download_url": "https://github.com/venujatech-byte/Netray/releases/download/v0.1.26/netray-linux-linux-64-0.1.26.tar.gz"
                }
            ]
        }"#;

        let rel = parse_github_release_json(sample, "linux").expect("Should parse linux asset");
        assert_eq!(rel.version, "0.1.26");
        assert_eq!(rel.tag_name, "v0.1.26");
        assert_eq!(rel.asset_name, "netray-linux-linux-64-0.1.26.tar.gz");
        assert!(rel.download_url.contains("netray-linux-linux-64-0.1.26.tar.gz"));
    }
}

