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

    // Fast memory read if checked within last 60 seconds (avoids spawning processes frequently)
    if last != 0 && now.saturating_sub(last) < 60 {
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
    #[cfg(target_os = "windows")]
    {
        // Check Windows registry for system theme preference
        #[allow(unused_mut)]
        let mut cmd = std::process::Command::new("reg");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }
        if let Ok(output) = cmd
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

    #[cfg(target_os = "linux")]
    {
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

pub const ICON_INACTIVE_DARK: Color = Color::from_rgb(0.38, 0.42, 0.48);
pub const ICON_INACTIVE_LIGHT: Color = Color::from_rgb(0.68, 0.72, 0.78);

pub fn icon_color_for_state(is_dark: bool, is_active: bool) -> Color {
    if is_active {
        if is_dark {
            TEXT_PRIMARY_DARK
        } else {
            TEXT_PRIMARY_LIGHT
        }
    } else if is_dark {
        ICON_INACTIVE_DARK
    } else {
        ICON_INACTIVE_LIGHT
    }
}

pub fn pick_list_style(is_dark: bool, status: iced::widget::pick_list::Status) -> iced::widget::pick_list::Style {
    let (bg, border_color, text_color, handle_color) = if is_dark {
        match status {
            iced::widget::pick_list::Status::Hovered | iced::widget::pick_list::Status::Opened => (
                Color::from_rgb(0.16, 0.18, 0.24),
                Color::from_rgb(0.24, 0.58, 1.0),
                Color::WHITE,
                Color::from_rgb(0.44, 0.72, 1.0),
            ),
            iced::widget::pick_list::Status::Active => (
                Color::from_rgb(0.12, 0.13, 0.17),
                Color::from_rgb(0.22, 0.25, 0.32),
                Color::from_rgb(0.92, 0.94, 0.98),
                Color::from_rgb(0.60, 0.65, 0.74),
            ),
        }
    } else {
        match status {
            iced::widget::pick_list::Status::Hovered | iced::widget::pick_list::Status::Opened => (
                Color::WHITE,
                Color::from_rgb(0.15, 0.48, 0.98),
                Color::from_rgb(0.09, 0.11, 0.15),
                Color::from_rgb(0.15, 0.48, 0.98),
            ),
            iced::widget::pick_list::Status::Active => (
                Color::WHITE,
                Color::from_rgb(0.80, 0.83, 0.88),
                Color::from_rgb(0.09, 0.11, 0.15),
                Color::from_rgb(0.45, 0.50, 0.58),
            ),
        }
    };

    iced::widget::pick_list::Style {
        text_color,
        placeholder_color: Color::from_rgb(0.50, 0.54, 0.62),
        handle_color,
        background: bg.into(),
        border: iced::Border {
            radius: 8.0.into(),
            width: 1.0,
            color: border_color,
        },
    }
}

pub fn pick_list_menu_style(is_dark: bool) -> iced::overlay::menu::Style {
    if is_dark {
        iced::overlay::menu::Style {
            background: Color::from_rgb(0.12, 0.14, 0.18).into(),
            border: iced::Border {
                radius: 10.0.into(),
                width: 1.0,
                color: Color::from_rgb(0.24, 0.28, 0.36),
            },
            text_color: Color::from_rgb(0.92, 0.94, 0.98),
            selected_text_color: Color::WHITE,
            selected_background: Color::from_rgb(0.0, 0.48, 1.0).into(),
        }
    } else {
        iced::overlay::menu::Style {
            background: Color::WHITE.into(),
            border: iced::Border {
                radius: 10.0.into(),
                width: 1.0,
                color: Color::from_rgb(0.82, 0.85, 0.90),
            },
            text_color: Color::from_rgb(0.09, 0.11, 0.15),
            selected_text_color: Color::WHITE,
            selected_background: Color::from_rgb(0.15, 0.48, 0.98).into(),
        }
    }
}

pub fn text_input_style(is_dark: bool, status: iced::widget::text_input::Status) -> iced::widget::text_input::Style {
    let (bg, border_color, text_color) = if is_dark {
        match status {
            iced::widget::text_input::Status::Focused => (
                Color::from_rgb(0.16, 0.18, 0.24),
                Color::from_rgb(0.24, 0.58, 1.0),
                Color::WHITE,
            ),
            iced::widget::text_input::Status::Hovered => (
                Color::from_rgb(0.14, 0.16, 0.21),
                Color::from_rgb(0.32, 0.36, 0.46),
                Color::WHITE,
            ),
            iced::widget::text_input::Status::Active => (
                Color::from_rgb(0.11, 0.12, 0.16),
                Color::from_rgb(0.22, 0.25, 0.32),
                Color::from_rgb(0.92, 0.94, 0.98),
            ),
            iced::widget::text_input::Status::Disabled => (
                Color::from_rgb(0.09, 0.10, 0.13),
                Color::from_rgb(0.18, 0.20, 0.25),
                Color::from_rgb(0.40, 0.44, 0.50),
            ),
        }
    } else {
        match status {
            iced::widget::text_input::Status::Focused => (
                Color::from_rgb(0.96, 0.98, 1.0),
                Color::from_rgb(0.15, 0.48, 0.98),
                Color::from_rgb(0.09, 0.11, 0.15),
            ),
            iced::widget::text_input::Status::Hovered => (
                Color::WHITE,
                Color::from_rgb(0.65, 0.70, 0.80),
                Color::from_rgb(0.09, 0.11, 0.15),
            ),
            iced::widget::text_input::Status::Active => (
                Color::WHITE,
                Color::from_rgb(0.80, 0.83, 0.88),
                Color::from_rgb(0.09, 0.11, 0.15),
            ),
            iced::widget::text_input::Status::Disabled => (
                Color::from_rgb(0.94, 0.95, 0.97),
                Color::from_rgb(0.85, 0.87, 0.91),
                Color::from_rgb(0.55, 0.58, 0.64),
            ),
        }
    };

    iced::widget::text_input::Style {
        background: bg.into(),
        border: iced::Border {
            radius: 8.0.into(),
            width: 1.0,
            color: border_color,
        },
        icon: Color::from_rgb(0.50, 0.54, 0.62),
        placeholder: Color::from_rgb(0.45, 0.49, 0.56),
        value: text_color,
        selection: Color::from_rgb(0.15, 0.48, 0.98),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_windows_resolve_effective_theme() {
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
    fn test_windows_effective_theme_to_iced() {
        assert_eq!(EffectiveTheme::Dark.to_iced_theme(), Theme::Dark);
        assert_eq!(EffectiveTheme::Light.to_iced_theme(), Theme::Light);
    }

    #[test]
    fn test_windows_system_theme_caching() {
        let first = detect_system_is_dark();
        let second = detect_system_is_dark();
        assert_eq!(first, second);
        invalidate_system_theme_cache();
        let third = detect_system_is_dark();
        assert_eq!(first, third);
    }
}
