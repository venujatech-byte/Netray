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
