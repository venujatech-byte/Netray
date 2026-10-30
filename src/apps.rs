//! Application Catalog and Discovery for Whitelist Split Tunneling
//!
//! Provides a list of popular applications, scans installed desktop entries,
//! and helpers to manage whitelist app chips.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

static PROCESS_PORT_CACHE: Mutex<Option<HashMap<u16, (String, Instant)>>> = Mutex::new(None);

/// Parses the socket inode from `/proc/net/tcp` or `/proc/net/tcp6` table for a given local port.
pub fn parse_inode_from_proc_net_tcp(content: &str, port: u16) -> Option<String> {
    let hex_port = format!(":{:04X}", port);
    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() > 9 {
            // Match either local_address (column 1) or rem_address (column 2)
            if parts[1].ends_with(&hex_port) || parts[2].ends_with(&hex_port) {
                let inode = parts[9];
                if inode != "0" {
                    return Some(inode.to_string());
                }
            }
        }
    }
    None
}

/// Resolves the process name of the local application connected to/from `port`.
/// Uses `/proc/net/tcp` inode resolution and caches hits for up to 5 seconds.
pub fn resolve_socket_process_name(port: u16) -> Option<String> {
    if port == 0 {
        return None;
    }

    // 1. Check fast in-memory cache
    if let Ok(mut guard) = PROCESS_PORT_CACHE.lock() {
        let cache = guard.get_or_insert_with(HashMap::new);
        if let Some((name, ts)) = cache.get(&port) {
            if ts.elapsed().as_secs() < 5 {
                return Some(name.clone());
            }
        }
    }

    // 2. Query /proc/net/tcp and /proc/net/tcp6
    let mut inode = None;
    for table_path in ["/proc/net/tcp", "/proc/net/tcp6"] {
        if let Ok(content) = std::fs::read_to_string(table_path) {
            if let Some(i) = parse_inode_from_proc_net_tcp(&content, port) {
                inode = Some(i);
                break;
            }
        }
    }

    let inode = inode?;
    let target_socket = format!("socket:[{}]", inode);

    // 3. Scan /proc for the matching fd
    let mut proc_name = None;
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let pid_dir = entry.path();
            let file_name = entry.file_name();
            let name_str = file_name.to_string_lossy();
            if !name_str.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }

            let fd_dir = pid_dir.join("fd");
            if let Ok(fd_entries) = std::fs::read_dir(fd_dir) {
                for fd_entry in fd_entries.flatten() {
                    if let Ok(target) = std::fs::read_link(fd_entry.path()) {
                        if target.to_string_lossy() == target_socket {
                            if let Ok(comm) = std::fs::read_to_string(pid_dir.join("comm")) {
                                let clean = comm.trim().to_string();
                                if !clean.is_empty() {
                                    proc_name = Some(clean);
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            if proc_name.is_some() {
                break;
            }
        }
    }

    if let Some(ref name) = proc_name {
        if let Ok(mut guard) = PROCESS_PORT_CACHE.lock() {
            if let Some(cache) = guard.as_mut() {
                if cache.len() > 256 {
                    cache.clear();
                }
                cache.insert(port, (name.clone(), Instant::now()));
            }
        }
    }

    proc_name
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPreset {
    pub label: String,
    pub executable: String,
}

impl std::fmt::Display for AppPreset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}

/// Returns a list of curated popular desktop apps for Linux
pub fn get_curated_linux_apps() -> Vec<AppPreset> {
    vec![
        AppPreset {
            label: "Google Chrome (google-chrome)".to_string(),
            executable: "google-chrome".to_string(),
        },
        AppPreset {
            label: "Mozilla Firefox (firefox)".to_string(),
            executable: "firefox".to_string(),
        },
        AppPreset {
            label: "Brave Browser (brave-browser)".to_string(),
            executable: "brave-browser".to_string(),
        },
        AppPreset {
            label: "Discord (discord)".to_string(),
            executable: "discord".to_string(),
        },
        AppPreset {
            label: "Telegram Desktop (telegram-desktop)".to_string(),
            executable: "telegram-desktop".to_string(),
        },
        AppPreset {
            label: "Steam (steam)".to_string(),
            executable: "steam".to_string(),
        },
        AppPreset {
            label: "Spotify (spotify)".to_string(),
            executable: "spotify".to_string(),
        },
        AppPreset {
            label: "Visual Studio Code (code)".to_string(),
            executable: "code".to_string(),
        },
        AppPreset {
            label: "Slack (slack)".to_string(),
            executable: "slack".to_string(),
        },
        AppPreset {
            label: "Obsidian (obsidian)".to_string(),
            executable: "obsidian".to_string(),
        },
        AppPreset {
            label: "cURL (curl)".to_string(),
            executable: "curl".to_string(),
        },
        AppPreset {
            label: "Git (git)".to_string(),
            executable: "git".to_string(),
        },
    ]
}

/// Discovers installed desktop applications by reading standard .desktop files.
/// Filters for valid applications and limits to a curated/popular set so dropdown
/// text shaping and layout remains lightweight during scrolling.
pub fn discover_installed_linux_apps() -> Vec<AppPreset> {
    let mut apps = get_curated_linux_apps();
    let dirs = [
        "/usr/share/applications",
        "/usr/local/share/applications",
    ];

    for dir_path in dirs {
        let path = Path::new(dir_path);
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                let file_path = entry.path();
                if file_path.extension().and_then(|s| s.to_str()) == Some("desktop") {
                    if let Ok(content) = std::fs::read_to_string(&file_path) {
                        if let Some((name, exec)) = parse_desktop_entry(&content) {
                            if !apps.iter().any(|a| a.executable.eq_ignore_ascii_case(&exec)) {
                                apps.push(AppPreset {
                                    label: format!("{name} ({exec})"),
                                    executable: exec,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // Sort alphabetically by label
    apps.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
    // Cap at 40 presets to avoid pick_list shaper overhead on scroll
    apps.truncate(40);
    apps
}

/// Parses a `.desktop` file to extract `Name=` and `Exec=`
fn parse_desktop_entry(content: &str) -> Option<(String, String)> {
    let mut name = None;
    let mut exec = None;
    let mut is_application = false;
    let mut is_hidden = false;
    let mut in_desktop_entry = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[Desktop Entry]" {
            in_desktop_entry = true;
            continue;
        } else if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_desktop_entry = false;
        }

        if in_desktop_entry {
            if trimmed.starts_with("NoDisplay=true") || trimmed.starts_with("Hidden=true") {
                is_hidden = true;
            }
            if trimmed == "Type=Application" {
                is_application = true;
            }
            if name.is_none() && trimmed.starts_with("Name=") {
                let val = trimmed.strip_prefix("Name=").unwrap().trim();
                if !val.is_empty() {
                    name = Some(val.to_string());
                }
            }
            if exec.is_none() && trimmed.starts_with("Exec=") {
                let raw_exec = trimmed.strip_prefix("Exec=").unwrap().trim();
                // Extract first token (the binary name or path)
                let first_token = raw_exec.split_whitespace().next().unwrap_or("");
                let bin_name = if let Some((_, n)) = first_token.rsplit_once('/') {
                    n
                } else {
                    first_token
                };
                let clean_bin = bin_name.trim_matches(&['"', '\''][..]).trim();
                if !clean_bin.is_empty() && !clean_bin.starts_with('%') {
                    exec = Some(clean_bin.to_string());
                }
            }
        }
    }

    if is_hidden || !is_application {
        return None;
    }

    match (name, exec) {
        (Some(n), Some(e)) => Some((n, e)),
        _ => None,
    }
}

/// Adds an executable process name to the comma-separated whitelist string.
/// Avoids duplicates (case-insensitive).
pub fn add_app_to_whitelist(current: &str, new_app: &str) -> String {
    let new_clean = new_app.trim().trim_matches(&['"', '\'', '`'][..]).trim();
    if new_clean.is_empty() {
        return current.to_string();
    }

    let existing = extract_app_chips(current);
    if existing.iter().any(|app| app.eq_ignore_ascii_case(new_clean)) {
        return current.to_string();
    }

    let mut list = existing;
    list.push(new_clean.to_string());
    list.join(", ")
}

/// Removes an executable process name from the comma-separated whitelist string.
pub fn remove_app_from_whitelist(current: &str, app_to_remove: &str) -> String {
    let to_remove = app_to_remove.trim();
    let existing = extract_app_chips(current);
    let filtered: Vec<String> = existing
        .into_iter()
        .filter(|app| !app.eq_ignore_ascii_case(to_remove))
        .collect();
    filtered.join(", ")
}

/// Extracts clean, trimmed process names from the whitelist string for chip display.
pub fn extract_app_chips(raw: &str) -> Vec<String> {
    let mut chips = Vec::new();
    for token in raw.split(|c| c == ',' || c == ';' || c == '\n' || c == '\r' || c == '\t') {
        let trimmed = token.trim().trim_matches(&['"', '\'', '`'][..]).trim();
        if !trimmed.is_empty() {
            // Strip any directory path components
            let file_name = if let Some((_, name)) = trimmed.rsplit_once('/') {
                name
            } else if let Some((_, name)) = trimmed.rsplit_once('\\') {
                name
            } else {
                trimmed
            };
            let file_name = file_name.trim();
            if !file_name.is_empty() && !chips.iter().any(|c: &String| c.eq_ignore_ascii_case(file_name)) {
                chips.push(file_name.to_string());
            }
        }
    }
    chips
}

/// Launches a native file picker dialog (zenity or kdialog) to allow user to pick an executable file from disk.
/// Returns the extracted executable name (e.g. `slack` or `my_app`), or None if canceled.
pub async fn pick_executable_file() -> Option<String> {
    tokio::task::spawn_blocking(|| {
        // Try zenity first
        if let Ok(output) = std::process::Command::new("zenity")
            .args(["--file-selection", "--title=Select Application Executable to Bypass"])
            .output()
        {
            if output.status.success() {
                let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path_str.is_empty() {
                    return extract_binary_name(&path_str);
                }
            }
        }

        // Fallback to kdialog
        if let Ok(output) = std::process::Command::new("kdialog")
            .args(["--getopenfilename", ".", "--title", "Select Application Executable"])
            .output()
        {
            if output.status.success() {
                let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path_str.is_empty() {
                    return extract_binary_name(&path_str);
                }
            }
        }

        None
    })
    .await
    .unwrap_or(None)
}

pub fn extract_binary_name(path_str: &str) -> Option<String> {
    let clean = path_str.trim().trim_matches(&['"', '\'', '`'][..]).trim();
    if clean.is_empty() {
        return None;
    }
    let name = if let Some((_, n)) = clean.rsplit_once('/') {
        n
    } else if let Some((_, n)) = clean.rsplit_once('\\') {
        n
    } else {
        clean
    };
    let name = name.trim();
    if !name.is_empty() {
        Some(name.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_app_to_whitelist() {
        let current = "";
        let res = add_app_to_whitelist(current, "firefox");
        assert_eq!(res, "firefox");

        let res2 = add_app_to_whitelist(&res, "chrome");
        assert_eq!(res2, "firefox, chrome");

        // Duplicate addition should be ignored
        let res3 = add_app_to_whitelist(&res2, "Firefox");
        assert_eq!(res3, "firefox, chrome");
    }

    #[test]
    fn test_remove_app_from_whitelist() {
        let current = "curl, firefox, discord, chrome";
        let res = remove_app_from_whitelist(current, "firefox");
        assert_eq!(res, "curl, discord, chrome");

        let res2 = remove_app_from_whitelist(&res, "CURL");
        assert_eq!(res2, "discord, chrome");
    }

    #[test]
    fn test_extract_app_chips() {
        let raw = "\"curl\"; '/usr/bin/firefox'\t`discord`\nslack, chrome";
        let chips = extract_app_chips(raw);
        assert_eq!(chips, vec!["curl", "firefox", "discord", "slack", "chrome"]);
    }

    #[test]
    fn test_extract_binary_name() {
        assert_eq!(extract_binary_name("/usr/bin/slack"), Some("slack".to_string()));
        assert_eq!(extract_binary_name("C:\\Program Files\\App\\app.exe"), Some("app.exe".to_string()));
        assert_eq!(extract_binary_name("\"firefox\""), Some("firefox".to_string()));
        assert_eq!(extract_binary_name("   "), None);
    }

    #[test]
    fn test_curated_apps_catalog() {
        let apps = get_curated_linux_apps();
        assert!(!apps.is_empty());
        assert!(apps.iter().any(|a| a.executable == "firefox"));
        assert!(apps.iter().any(|a| a.executable == "google-chrome"));
    }

    #[test]
    fn test_parse_inode_from_proc_net_tcp() {
        let sample = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 998877 1 00000000 100 0 0 10 0
   1: 0100007F:A001 0100007F:1F90 01 00000000:00000000 00:00000000 00000000  1000        0 112233 1 00000000 100 0 0 10 0
";
        // 0x1F90 = 8080
        assert_eq!(parse_inode_from_proc_net_tcp(sample, 8080), Some("998877".to_string()));
        // 0xA001 = 40961
        assert_eq!(parse_inode_from_proc_net_tcp(sample, 40961), Some("112233".to_string()));
        // Unmatched port
        assert_eq!(parse_inode_from_proc_net_tcp(sample, 9999), None);
    }

    #[test]
    fn test_resolve_socket_process_name_zero_port() {
        assert_eq!(resolve_socket_process_name(0), None);
    }
}


