//! Application Catalog and Discovery for Whitelist Split Tunneling on Windows
//!
//! Provides a list of popular Windows applications (.exe),
//! and helpers to manage whitelist app chips.

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

/// Returns a list of curated popular desktop apps for Windows
pub fn get_curated_windows_apps() -> Vec<AppPreset> {
    vec![
        AppPreset {
            label: "Google Chrome (chrome.exe)".to_string(),
            executable: "chrome.exe".to_string(),
        },
        AppPreset {
            label: "Mozilla Firefox (firefox.exe)".to_string(),
            executable: "firefox.exe".to_string(),
        },
        AppPreset {
            label: "Microsoft Edge (msedge.exe)".to_string(),
            executable: "msedge.exe".to_string(),
        },
        AppPreset {
            label: "Brave Browser (brave.exe)".to_string(),
            executable: "brave.exe".to_string(),
        },
        AppPreset {
            label: "Discord (Discord.exe)".to_string(),
            executable: "Discord.exe".to_string(),
        },
        AppPreset {
            label: "Telegram Desktop (Telegram.exe)".to_string(),
            executable: "Telegram.exe".to_string(),
        },
        AppPreset {
            label: "Steam Client (steam.exe)".to_string(),
            executable: "steam.exe".to_string(),
        },
        AppPreset {
            label: "Spotify Music (Spotify.exe)".to_string(),
            executable: "Spotify.exe".to_string(),
        },
        AppPreset {
            label: "Visual Studio Code (Code.exe)".to_string(),
            executable: "Code.exe".to_string(),
        },
        AppPreset {
            label: "Slack (slack.exe)".to_string(),
            executable: "slack.exe".to_string(),
        },
        AppPreset {
            label: "Obsidian (Obsidian.exe)".to_string(),
            executable: "Obsidian.exe".to_string(),
        },
        AppPreset {
            label: "cURL (curl.exe)".to_string(),
            executable: "curl.exe".to_string(),
        },
        AppPreset {
            label: "Git for Windows (git.exe)".to_string(),
            executable: "git.exe".to_string(),
        },
    ]
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

/// Launches a native Windows OpenFileDialog using PowerShell to allow user to pick an .exe file.
/// Returns the extracted executable name (e.g. `slack.exe` or `game.exe`), or None if canceled.
pub async fn pick_executable_file() -> Option<String> {
    tokio::task::spawn_blocking(|| {
        let ps_script = r#"
            [System.Reflection.Assembly]::LoadWithPartialName('System.windows.forms') | Out-Null
            $dialog = New-Object System.Windows.Forms.OpenFileDialog
            $dialog.Filter = 'Executables (*.exe)|*.exe|All files (*.*)|*.*'
            $dialog.Title = 'Select Application Executable to Bypass'
            if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
                Write-Output $dialog.FileName
            }
        "#;

        if let Ok(output) = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", ps_script])
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
    let name = if let Some((_, n)) = clean.rsplit_once('\\') {
        n
    } else if let Some((_, n)) = clean.rsplit_once('/') {
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
    fn test_add_app_to_whitelist_windows() {
        let current = "";
        let res = add_app_to_whitelist(current, "firefox.exe");
        assert_eq!(res, "firefox.exe");

        let res2 = add_app_to_whitelist(&res, "chrome.exe");
        assert_eq!(res2, "firefox.exe, chrome.exe");

        // Duplicate addition should be ignored
        let res3 = add_app_to_whitelist(&res2, "Firefox.EXE");
        assert_eq!(res3, "firefox.exe, chrome.exe");
    }

    #[test]
    fn test_remove_app_from_whitelist_windows() {
        let current = "curl.exe, firefox.exe, discord.exe, chrome.exe";
        let res = remove_app_from_whitelist(current, "firefox.exe");
        assert_eq!(res, "curl.exe, discord.exe, chrome.exe");

        let res2 = remove_app_from_whitelist(&res, "CURL.EXE");
        assert_eq!(res2, "discord.exe, chrome.exe");
    }

    #[test]
    fn test_extract_app_chips_windows() {
        let raw = "\"curl.exe\"; 'C:\\Program Files\\Firefox\\firefox.exe'\t`discord.exe`\nslack.exe, chrome.exe";
        let chips = extract_app_chips(raw);
        assert_eq!(chips, vec!["curl.exe", "firefox.exe", "discord.exe", "slack.exe", "chrome.exe"]);
    }

    #[test]
    fn test_extract_binary_name_windows() {
        assert_eq!(extract_binary_name("C:\\Program Files\\App\\app.exe"), Some("app.exe".to_string()));
        assert_eq!(extract_binary_name("/usr/bin/slack"), Some("slack".to_string()));
        assert_eq!(extract_binary_name("\"firefox.exe\""), Some("firefox.exe".to_string()));
        assert_eq!(extract_binary_name("   "), None);
    }

    #[test]
    fn test_curated_windows_apps_catalog() {
        let apps = get_curated_windows_apps();
        assert!(!apps.is_empty());
        assert!(apps.iter().any(|a| a.executable == "firefox.exe"));
        assert!(apps.iter().any(|a| a.executable == "chrome.exe"));
    }
}


