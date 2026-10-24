use crate::storage::ThemePreference;
use iced::{Color, Theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectiveTheme {
    Dark,
    Light,
}

impl EffectiveTheme {
    pub fn is_dark(&self) -> bool {
        matches!(self, Self::Dark)
    }

    pub fn to_iced_theme(&self) -> Theme {
        match self {
            Self::Dark => Theme::Dark,
            Self::Light => Theme::Light,
        }
    }
}

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static CACHED_IS_DARK: AtomicBool = AtomicBool::new(true);
static LAST_CHECK_SECS: AtomicU64 = AtomicU64::new(0);

/// Detects whether the host desktop environment is currently in dark mode, cached to prevent lag.
pub fn detect_system_is_dark() -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let last = LAST_CHECK_SECS.load(Ordering::Relaxed);

    // Fast memory read if checked within last 10 seconds (avoids spawning processes on frame redraws)
    if last != 0 && now.saturating_sub(last) < 10 {
        return CACHED_IS_DARK.load(Ordering::Relaxed);
    }

    let is_dark = query_system_is_dark_uncached();
    CACHED_IS_DARK.store(is_dark, Ordering::Relaxed);
    LAST_CHECK_SECS.store(now, Ordering::Relaxed);
    is_dark
}

/// Force invalidate the cached theme (e.g. when user changes settings)
pub fn invalidate_system_theme_cache() {
    LAST_CHECK_SECS.store(0, Ordering::Relaxed);
}

fn query_system_is_dark_uncached() -> bool {
    #[cfg(target_os = "linux")]
    {
        // 1. Try gsettings (GNOME / Cinnamon / Unity / Pop!_OS)
        if let Ok(output) = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "color-scheme"])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();
            if stdout.contains("dark") {
                return true;
            } else if stdout.contains("light") {
                return false;
            }
        }

        // 2. Try xdg-desktop-portal Settings interface
        if let Ok(output) = std::process::Command::new("busctl")
            .args([
                "--user",
                "call",
                "org.freedesktop.portal.Desktop",
                "/org/freedesktop/portal/desktop",
                "org.freedesktop.portal.Settings",
                "Read",
                "ss",
                "org.freedesktop.appearance",
                "color-scheme",
            ])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.contains(" 1") {
                return true; // 1 = prefer-dark
            } else if stdout.contains(" 2") {
                return false; // 2 = prefer-light
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        // Check Windows registry for system theme preference
        if let Ok(output) = std::process::Command::new("reg")
            .args([
                "query",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
                "/v",
                "AppsUseLightTheme",
            ])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.contains("0x1") {
                return false;
            } else if stdout.contains("0x0") {
                return true;
            }
        }
    }

    // Default fallback to true (dark) for cybersecurity / proxy cockpit consistency
    true
}

/// Resolves user theme preference to effective theme (Dark or Light)
pub fn resolve_effective_theme(preference: ThemePreference) -> EffectiveTheme {
    match preference {
        ThemePreference::Dark => EffectiveTheme::Dark,
        ThemePreference::Light => EffectiveTheme::Light,
        ThemePreference::System => {
            if detect_system_is_dark() {
                EffectiveTheme::Dark
            } else {
                EffectiveTheme::Light
            }
        }
    }
}

pub const TEXT_PRIMARY_DARK: Color = Color::WHITE;
pub const TEXT_PRIMARY_LIGHT: Color = Color::from_rgb(0.09, 0.11, 0.15);

pub const BG_WINDOW_DARK: Color = Color::from_rgb(0.10, 0.11, 0.14);
pub const BG_WINDOW_LIGHT: Color = Color::from_rgb(0.95, 0.96, 0.98);

pub const CARD_BG_DARK: Color = Color::from_rgb(0.14, 0.16, 0.20);
pub const CARD_BG_LIGHT: Color = Color::WHITE;

pub const CARD_BORDER_DARK: Color = Color::from_rgb(0.20, 0.22, 0.28);
pub const CARD_BORDER_LIGHT: Color = Color::from_rgb(0.86, 0.88, 0.92);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_effective_theme() {
        assert_eq!(
            resolve_effective_theme(ThemePreference::Dark),
            EffectiveTheme::Dark
        );
        assert_eq!(
            resolve_effective_theme(ThemePreference::Light),
            EffectiveTheme::Light
        );
        let system_resolved = resolve_effective_theme(ThemePreference::System);
        assert!(matches!(
            system_resolved,
            EffectiveTheme::Dark | EffectiveTheme::Light
        ));
    }

    #[test]
    fn test_effective_theme_to_iced() {
        assert_eq!(EffectiveTheme::Dark.to_iced_theme(), Theme::Dark);
        assert_eq!(EffectiveTheme::Light.to_iced_theme(), Theme::Light);
    }

    #[test]
    fn test_system_theme_caching() {
        let first = detect_system_is_dark();
        let second = detect_system_is_dark();
        assert_eq!(first, second);
        invalidate_system_theme_cache();
        let third = detect_system_is_dark();
        assert_eq!(first, third);
    }
}
