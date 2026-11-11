#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[path = "../../src/log_activity.rs"]
mod log_activity;
#[path = "../../src/settings_ui.rs"]
mod settings_ui;
pub mod apps;
pub mod autostart;
pub mod icons;
pub mod notifications;
pub mod protocols;
pub mod qr;
pub mod qr_scanner;
pub mod storage;
pub mod sysproxy;
pub mod theme;
#[path = "../../src/tray_status.rs"]
mod tray_status;
mod tunnel;
pub mod updater;
pub mod v2ray;

use chrono::Local;
use iced::futures::{SinkExt, StreamExt};
use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, text, text_input, toggler,
    Space,
};
use iced::{Alignment, Color, Element, Length, Subscription, Task, Theme};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon as TrayIconImage, MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tunnel::{run_real_tunnel_worker, TunnelConfig};

const ACCENT_BLUE: Color = Color::from_rgb(0.15, 0.48, 0.98); // #2563EB
const ACCENT_BLUE_LIGHT: Color = Color::from_rgb(0.24, 0.58, 1.0); // #3B82F6
const TEXT_MUTED: Color = Color::from_rgb(0.55, 0.60, 0.68);
const SUCCESS_GREEN: Color = Color::from_rgb(0.22, 0.78, 0.49);
const FAILURE_RED: Color = Color::from_rgb(0.95, 0.35, 0.40);
const WARNING_AMBER: Color = Color::from_rgb(0.96, 0.62, 0.22);
const CORAL_ACCENT: Color = Color::from_rgb(0.98, 0.52, 0.48); // #FA857A
const APP_VERSION: &str = match option_env!("NETRAY_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

pub fn format_friendly_error(err: &str) -> String {
    let clean = err.trim().trim_start_matches("Connection check failed:").trim();
    if clean.contains("failed to lookup ip") || clean.contains("DOH//") || clean.contains("features/dns") {
        "DNS lookup timeout · Retrying connection".to_string()
    } else if clean.contains("Connection check failed") || clean.contains("unreachable") || clean.contains("503") || clean.contains("502") {
        "Remote proxy endpoint is unreachable".to_string()
    } else if clean.contains("Core process exited") || clean.contains("Engine failed") {
        "V2Ray core process stopped unexpectedly".to_string()
    } else if clean.contains("handshake") || clean.contains("Handshake") || clean.contains("TLS") {
        "TLS handshake error with server node".to_string()
    } else if clean.contains("connection refused") || clean.contains("reset by peer") {
        "Remote server refused or reset connection".to_string()
    } else if clean.contains("Timeout") || clean.contains("timeout") {
        "Network connection timed out".to_string()
    } else if clean.len() > 65 {
        "Continuous network errors detected".to_string()
    } else if clean.is_empty() {
        "Connection lost or unreachable".to_string()
    } else {
        clean.to_string()
    }
}

fn format_uptime(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        (seconds / 60) % 60,
        seconds % 60
    )
}

static UNIQUE_NODE_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub fn generate_unique_node_id(prefix: &str) -> String {
    let count = UNIQUE_NODE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{}-{}-{}", prefix, nanos, count)
}

fn form_card_style(theme: &Theme) -> container::Style {
    let is_dark = match theme {
        Theme::Light => false,
        _ => true,
    };
    if is_dark {
        container::Style {
            background: Some(theme::CARD_BG_DARK.into()),
            border: iced::Border {
                radius: 8.0.into(),
                width: 1.0,
                color: theme::CARD_BORDER_DARK,
            },
            ..Default::default()
        }
    } else {
        container::Style {
            background: Some(theme::CARD_BG_LIGHT.into()),
            border: iced::Border {
                radius: 8.0.into(),
                width: 1.0,
                color: theme::CARD_BORDER_LIGHT,
            },
            ..Default::default()
        }
    }
}

fn transparent_profile_btn(_: &Theme, _: button::Status) -> button::Style {
    button::Style {
        background: None,
        text_color: Color::WHITE,
        ..Default::default()
    }
}

fn transparent_muted_btn(_: &Theme, _: button::Status) -> button::Style {
    button::Style {
        background: None,
        text_color: TEXT_MUTED,
        ..Default::default()
    }
}

fn text_input_dark(_: &Theme, status: text_input::Status) -> text_input::Style {
    theme::text_input_style(true, status)
}

fn text_input_light(_: &Theme, status: text_input::Status) -> text_input::Style {
    theme::text_input_style(false, status)
}

fn modern_text_input<'a>(
    placeholder: &'a str,
    value: &'a str,
    is_dark: bool,
) -> iced::widget::TextInput<'a, Message> {
    text_input(placeholder, value)
        .padding(iced::Padding {
            top: 8.0,
            bottom: 8.0,
            left: 10.0,
            right: 10.0,
        })
        .size(13)
        .style(if is_dark { text_input_dark } else { text_input_light })
}

fn btn_icon_dark_active(_: &Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => Some(Color::from_rgba(1.0, 1.0, 1.0, 0.10).into()),
        button::Status::Pressed => Some(Color::from_rgba(1.0, 1.0, 1.0, 0.16).into()),
        _ => None,
    };
    button::Style {
        background: bg,
        text_color: theme::TEXT_PRIMARY_DARK,
        border: iced::Border {
            radius: 999.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn btn_icon_dark_inactive(_: &Theme, _: button::Status) -> button::Style {
    button::Style {
        background: None,
        text_color: theme::ICON_INACTIVE_DARK,
        border: iced::Border {
            radius: 999.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn btn_icon_light_active(_: &Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => Some(Color::from_rgba(0.0, 0.0, 0.0, 0.08).into()),
        button::Status::Pressed => Some(Color::from_rgba(0.0, 0.0, 0.0, 0.14).into()),
        _ => None,
    };
    button::Style {
        background: bg,
        text_color: theme::TEXT_PRIMARY_LIGHT,
        border: iced::Border {
            radius: 999.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn btn_icon_light_inactive(_: &Theme, _: button::Status) -> button::Style {
    button::Style {
        background: None,
        text_color: theme::ICON_INACTIVE_LIGHT,
        border: iced::Border {
            radius: 999.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

pub fn rounded_icon_btn_style(is_dark: bool, is_active: bool) -> fn(&Theme, button::Status) -> button::Style {
    match (is_dark, is_active) {
        (true, true) => btn_icon_dark_active,
        (true, false) => btn_icon_dark_inactive,
        (false, true) => btn_icon_light_active,
        (false, false) => btn_icon_light_inactive,
    }
}

fn transparent_text_btn(_: &Theme, _: button::Status) -> button::Style {
    button::Style {
        background: None,
        text_color: TEXT_MUTED,
        ..Default::default()
    }
}

fn ping_btn_pinging(_: &Theme, _: button::Status) -> button::Style {
    button::Style {
        background: Some(ACCENT_BLUE_LIGHT.into()),
        text_color: Color::WHITE,
        border: iced::Border {
            radius: 24.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn ping_btn_active(_: &Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => ACCENT_BLUE_LIGHT,
        _ => ACCENT_BLUE,
    };
    button::Style {
        background: Some(bg.into()),
        text_color: Color::WHITE,
        border: iced::Border {
            radius: 24.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

pub fn ping_btn_style(is_dark: bool, active: bool, pinging: bool) -> fn(&Theme, button::Status) -> button::Style {
    if !active {
        rounded_icon_btn_style(is_dark, false)
    } else if pinging {
        ping_btn_pinging
    } else {
        ping_btn_active
    }
}

fn fab_btn_style(_: &Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => ACCENT_BLUE_LIGHT,
        _ => ACCENT_BLUE,
    };
    button::Style {
        background: Some(bg.into()),
        border: iced::Border {
            radius: 32.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn toast_cancel_btn(_: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: match status {
            button::Status::Hovered => Some(Color::from_rgba(1.0, 1.0, 1.0, 0.12).into()),
            _ => Some(Color::from_rgba(1.0, 1.0, 1.0, 0.06).into()),
        },
        border: iced::Border {
            radius: 6.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn toast_close_btn(_: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: match status {
            button::Status::Hovered => Some(Color::from_rgba(1.0, 1.0, 1.0, 0.1).into()),
            _ => None,
        },
        border: iced::Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn toast_attempt_pill(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba(0.96, 0.62, 0.22, 0.15).into()),
        border: iced::Border {
            radius: 8.0.into(),
            width: 1.0,
            color: Color::from_rgba(0.96, 0.62, 0.22, 0.40),
        },
        ..Default::default()
    }
}

fn toast_container_style(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgb(0.12, 0.13, 0.17).into()),
        border: iced::Border {
            radius: 8.0.into(),
            width: 1.0,
            color: Color::from_rgba(0.96, 0.62, 0.22, 0.35),
        },
        ..Default::default()
    }
}




fn bg_window_style(theme: &Theme) -> container::Style {
    let is_dark = !matches!(theme, Theme::Light);
    container::Style {
        background: Some(if is_dark {
            theme::BG_WINDOW_DARK.into()
        } else {
            theme::BG_WINDOW_LIGHT.into()
        }),
        ..Default::default()
    }
}

fn chip_close_btn_style(_: &Theme, _: button::Status) -> button::Style {
    button::Style {
        background: None,
        ..Default::default()
    }
}

fn whitelist_chip_style(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba(0.24, 0.58, 1.0, 0.18).into()),
        border: iced::Border {
            radius: 12.0.into(),
            width: 1.0,
            color: Color::from_rgba(0.24, 0.58, 1.0, 0.45),
        },
        ..Default::default()
    }
}

fn disabled_update_btn(_: &Theme, _: button::Status) -> button::Style {
    button::Style {
        background: Some(Color::from_rgb(0.28, 0.30, 0.34).into()),
        text_color: Color::from_rgb(0.62, 0.64, 0.68),
        border: iced::Border { radius: 999.0.into(), ..Default::default() },
        ..Default::default()
    }
}

fn primary_action_btn(_: &Theme, _: button::Status) -> button::Style {
    button::Style {
        background: Some(ACCENT_BLUE.into()),
        border: iced::Border {
            radius: 6.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

const TUNNEL_MODES: [TunnelMode; 4] = [
    TunnelMode::V2RayXray,
    TunnelMode::SshPayload,
    TunnelMode::SshSslSni,
    TunnelMode::SlowDns,
];

pub fn main() -> iced::Result {
    let window = iced::window::Settings {
        size: (460.0, 640.0).into(),
        icon: create_window_icon(),
        exit_on_close_request: false,
        ..Default::default()
    };
    iced::application("Netray", NetrayApp::update, NetrayApp::view)
        .default_font(iced::Font::DEFAULT)
        .subscription(NetrayApp::subscription)
        .theme(|app| app.theme())
        .window(window)
        .run_with(NetrayApp::new)
}

fn load_logo_pixels() -> Result<(Vec<u8>, u32, u32), Box<dyn std::error::Error>> {
    let decoder = png::Decoder::new(std::io::Cursor::new(include_bytes!(
        "../../packaging/netray-tray.png"
    )));
    let mut reader = decoder.read_info()?;
    let mut pixels = vec![
        0;
        reader
            .output_buffer_size()
            .ok_or("tray icon image is too large")?
    ];
    let info = reader.next_frame(&mut pixels)?;
    Ok((
        pixels[..info.buffer_size()].to_vec(),
        info.width,
        info.height,
    ))
}

fn create_window_icon() -> Option<iced::window::Icon> {
    let (pixels, width, height) = load_logo_pixels().ok()?;
    iced::window::icon::from_rgba(pixels, width, height).ok()
}

fn create_tray_icon() -> Result<(TrayIcon, MenuItem), Box<dyn std::error::Error>> {
    let (pixels, width, height) = load_logo_pixels()?;
    let icon = TrayIconImage::from_rgba(pixels, width, height)?;
    let show = MenuItem::with_id("show", "Show Netray", true, None);
    let hide = MenuItem::with_id("hide", "Hide to Tray", true, None);
    let connect = MenuItem::with_id("connect", "Connect", true, None);
    let exit = MenuItem::with_id("exit", "Exit", true, None);
    let menu = Menu::with_items(&[
        &show,
        &hide,
        &PredefinedMenuItem::separator(),
        &connect,
        &PredefinedMenuItem::separator(),
        &exit,
    ])?;
    let tray = TrayIconBuilder::new()
        .with_icon(icon)
        .with_menu(Box::new(menu))
        .with_tooltip("Netray")
        .build()?;
    Ok((tray, connect))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Profiles,
    EditNode,
    Logs,
    SshSettings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileSort {
    Name,
    CreatedDate,
    Type,
}

impl ProfileSort {
    const ALL: [Self; 3] = [Self::Name, Self::CreatedDate, Self::Type];
}

impl std::fmt::Display for ProfileSort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Name => "Name",
            Self::CreatedDate => "Created date",
            Self::Type => "Type",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TunnelMode {
    SshPayload,
    SshSslSni,
    V2RayXray,
    SlowDns,
}

impl std::fmt::Display for TunnelMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TunnelMode::SshPayload => write!(f, "SSH + HTTP Payload"),
            TunnelMode::SshSslSni => write!(f, "SSH + SSL/TLS (SNI)"),
            TunnelMode::V2RayXray => write!(f, "V2Ray / Xray Core (WinTun)"),
            TunnelMode::SlowDns => write!(f, "DNSTT (SlowDNS)"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Success,
    Warn,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogsFilterPreset {
    All,
    Tunnel,
    Bypass,
    Apps,
    Errors,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub timestamp: String,
    pub source: String,
    pub message: String,
    pub level: LogLevel,
    pub target_host: Option<String>,
    pub target_port: Option<u16>,
    pub app_name: Option<String>,
    pub route: Option<String>,
}

impl LogEntry {
    pub fn new(source: impl Into<String>, message: impl Into<String>, level: LogLevel) -> Self {
        Self {
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
            source: source.into(),
            message: message.into(),
            level,
            target_host: None,
            target_port: None,
            app_name: None,
            route: None,
        }
    }
}

pub async fn send_log(
    tx: &mpsc::Sender<LogEntry>,
    source: impl AsRef<str>,
    msg: impl AsRef<str>,
    level: LogLevel,
) {
    let source = source.as_ref();
    let msg = msg.as_ref();
    if log_activity::should_forward(log_activity::enabled(), source, msg, level == LogLevel::Error) {
        let _ = tx.send(LogEntry::new(source, msg, level)).await;
    }
}

pub async fn send_connection_log(
    tx: &mpsc::Sender<LogEntry>,
    source: impl Into<String>,
    msg: impl Into<String>,
    level: LogLevel,
    target_host: impl Into<String>,
    target_port: u16,
    app_name: Option<String>,
    route: Option<String>,
) {
    if !log_activity::enabled() { return; }
    let _ = tx
        .send(LogEntry {
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
            source: source.into(),
            message: msg.into(),
            level,
            target_host: Some(target_host.into()),
            target_port: Some(target_port),
            app_name,
            route,
        })
        .await;
}













#[derive(Debug, Clone)]
pub struct ShareModalState {
    pub profile_id: String,
    pub profile_name: String,
    pub link: String,
    pub qr_svg: Vec<u8>,
    pub is_protected: bool,
    pub copied: bool,
}


pub const MAX_RECONNECT_ATTEMPTS: u32 = 5;
pub const RECONNECT_COUNTDOWN_SECONDS: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorNotification {
    pub title: String,
    pub message: String,
    pub attempt: u32,
    pub max_attempts: u32,
    pub countdown_secs: Option<u32>,
    pub is_reconnecting: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QrScanSource {
    DesktopScreenshot,
    Webcam,
    PhotoUpload,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QrScanStatus {
    Idle,
    Scanning(QrScanSource),
    Success(String),
    Error(String),
}

#[derive(Debug, Clone)]
pub struct QrScanModalState {
    pub is_open: bool,
    pub status: QrScanStatus,
}

impl Default for QrScanModalState {
    fn default() -> Self {
        Self {
            is_open: false,
            status: QrScanStatus::Idle,
        }
    }
}

pub struct NetrayApp {
    pub view_mode: ViewMode,
    show_overflow_menu: bool,
    profile_sort: ProfileSort,
    usage_records: Vec<storage::DailyUsage>,
    usage_date_filter: String,
    usage_stop_pending: bool,
    skip_final_usage_once: bool,
    usage_query_pending: bool,
    pub is_running: bool,
    connection_error: bool,
    pub connected_at: Option<Instant>,
    pub ping_ms: Option<u128>,
    pub tunnel_mode: TunnelMode,

    pub payload_text: String,
    pub bug_host: String,
    pub sni_host: String,
    pub gen_keep_alive: bool,
    pub gen_online_host: bool,
    pub gen_split: bool,

    pub ssh_host: String,
    pub ssh_port: String,
    pub ssh_user: String,
    pub ssh_pass: String,
    pub proxy_ip: String,
    pub proxy_port: String,
    pub local_socks_port: String,

    pub v2ray_uri_or_json: String,
    pub v2ray_profiles: Vec<storage::V2RayProfile>,
    pub active_profile_id: Option<String>,
    pub node_edit_state: v2ray::NodeEditState,
    pub ping_in_progress: Option<String>,
    pub ping_results: HashMap<String, String>,
    pub show_add_menu: bool,

    pub run_at_startup: bool,
    pub connect_at_startup: bool,
    pub auto_reconnect: bool,
    pub notify_connection_drops: bool,
    pub allow_insecure_tls: bool,
    pub ipv6_enabled: bool,
    pub bypass_lan: bool,
    pub dns_servers: String,
    pub user_stopped: bool,
    pub reconnect_attempts: u32,
    pub error_notification: Option<ErrorNotification>,
    pub startup_connect_prompt: bool,
    pub qr_scan_modal: QrScanModalState,
    pub update_state: updater::UpdateState,
    show_release_notes: bool,
    pub auto_check_updates: bool,
    pub logs_enabled: bool,

    pub enable_tun: bool,
    pub enable_sysproxy: bool,
    pub whitelist_apps: String,
    pub whitelist_urls: String,
    pub available_apps: Vec<apps::AppPreset>,
    pub selected_app_preset: Option<apps::AppPreset>,
    pub public_ip: Option<String>,
    pub is_checking_ip: bool,
    pub instant_import: bool,
    pub clipboard_import_candidate: Option<String>,
    pub share_modal: Option<ShareModalState>,
    pub theme_preference: storage::ThemePreference,
    pub profile_display_cache: HashMap<String, (String, String)>,
    pub cached_app_chips: Vec<String>,
    pub status_badge_text: String,
    pub profile_count_label: String,

    pub logs: Vec<LogEntry>,
    pub auto_scroll_logs: bool,
    pub logs_filter: String,
    pub logs_preset: LogsFilterPreset,
    pub log_sender: Option<mpsc::Sender<LogEntry>>,
    pub stop_signal: Option<tokio::sync::oneshot::Sender<()>>,
    close_confirmation: Option<iced::window::Id>,
    delete_confirmation: Option<String>,
    window_id: Option<iced::window::Id>,
    hidden_to_tray: bool,
    exit_after_stop: bool,
    tray_icon: Option<TrayIcon>,
    tray_connect_item: Option<MenuItem>,
    worker_active: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    SetViewMode(ViewMode),
    ToggleOverflowMenu,
    ProfileSortChanged(ProfileSort),
    UsageTick,
    UsageDateChanged(String),
    UsageUpdated(Result<(u64, u64), String>),
    TunnelModeChanged(TunnelMode),
    ToggleTunnel,
    MinimizeToTray,

    SelectProfile(String),
    SelectProfileIndex(usize),
    PingProfile(String),
    PingProfileIndex(usize),
    PingFinished(String, Result<u128, String>),
    EditProfile(String),
    EditProfileIndex(usize),
    DeleteProfile(String),
    ConfirmDeleteProfile,
    CancelDeleteProfile,
    DeleteProfileIndex(usize),
    ShareProfile(String),
    ShareProfileIndex(usize),
    CloseShareModal,
    CopyShareLink,
    ToggleShareProtect,
    AddNewProfile,
    OpenAddMenu,
    CloseAddMenu,
    CreateProfileWithType(protocols::ProfileType),

    EditRemarksChanged(String),
    EditAddressChanged(String),
    EditPortChanged(String),
    EditEncryptMethodChanged(String),
    EditFlowChanged(String),
    EditUserIdChanged(String),
    EditTransportChanged(String),
    EditHeaderTypeChanged(String),
    EditTlsTypeChanged(String),
    EditSniChanged(String),
    EditFingerprintChanged(String),
    EditAlpnChanged(String),
    EditPathChanged(String),
    EditPublicKeyChanged(String),
    EditShortIdChanged(String),
    SaveEditedNode,
    CancelEditNode,

    PayloadChanged(String),
    BugHostChanged(String),
    SniChanged(String),
    ToggleKeepAlive(bool),
    ToggleOnlineHost(bool),
    ToggleSplit(bool),
    GeneratePayload,
    SshHostChanged(String),
    SshPortChanged(String),
    SshUserChanged(String),
    SshPassChanged(String),
    ProxyIpChanged(String),
    ProxyPortChanged(String),

    V2RayConfigChanged(String),
    LocalSocksPortChanged(String),
    RequestFileImport,
    ConfigFileSelected(Option<(String, String)>),
    RequestClipboardImport,
    ClipboardImported(Option<String>),
    ToggleInstantImport(bool),
    ConfirmClipboardImport,
    CancelClipboardImport,
    ClipboardCandidateChanged(String),
    ToggleTunMode(bool),
    ToggleSysProxy(bool),
    WhitelistAppsChanged(String),
    WhitelistUrlsChanged(String),
    PickAppSelected(apps::AppPreset),
    BrowseAppExecutable,
    AppExecutablePicked(Option<String>),
    RemoveWhitelistApp(String),
    CheckPublicIp,
    PublicIpChecked(Result<String, String>),
    ThemePreferenceChanged(storage::ThemePreference),

    // QR Code Scanner Events
    OpenQrScanModal,
    CloseQrScanModal,
    TriggerQrScan(QrScanSource),
    QrScanFinished(Result<String, String>),
    ToggleRunAtStartup(bool),
    ToggleConnectAtStartup(bool),
    DismissStartupConnectPrompt,
    ConfirmStartupConnectPrompt,
    ReconnectCountdownTick,
    ExecuteAutoReconnect,
    CancelAutoReconnect,
    DismissNotification,
    ToggleAutoReconnect(bool),
    ToggleConnectionDropNotifications(bool),
    ToggleLogsEnabled(bool),
    ToggleAllowInsecureTls(bool),
    ToggleIpv6(bool),
    ToggleBypassLan(bool),
    DnsServersChanged(String),

    CheckForUpdates,
    UpdateCheckFinished(Result<Option<updater::ReleaseInfo>, String>),
    StartInAppUpgrade,
    UpdateDownloadFinished(Result<std::path::PathBuf, String>),
    ApplyUpdateAndRestart,
    DismissUpdateToast,
    CloseReleaseNotesModal,
    ToggleAutoCheckUpdates(bool),

    LoggerReady(mpsc::Sender<LogEntry>),
    NewLog(LogEntry),
    LogBatch(Vec<LogEntry>),
    UptimeTick,
    ClearLogs,
    CopyLogs,
    AutoScrollLogs(bool),
    LogsFilterChanged(String),
    LogsPresetChanged(LogsFilterPreset),
    CloseRequested(iced::window::Id),
    WindowOpened(iced::window::Id),
    WindowTick,
    TrayShow,
    TrayExit,
    Minimized(Option<bool>),
    ConfirmExit,
    CancelExit,
}

impl NetrayApp {
    fn new() -> (Self, Task<Message>) {
        Self::from_saved(storage::load_data())
    }

    fn from_saved(saved: storage::AppData) -> (Self, Task<Message>) {
        updater::cleanup_stale_update_files();
        log_activity::set_enabled(saved.logs_enabled);
        let tunnel_mode = match saved.tunnel_mode.as_str() {
            "SshPayload" => TunnelMode::SshPayload,
            "SshSslSni" => TunnelMode::SshSslSni,
            "SlowDns" => TunnelMode::SlowDns,
            _ => TunnelMode::V2RayXray,
        };

        let active_id = saved.active_profile_id.clone();
        let active_uri = saved.v2ray_uri_or_json.clone();

        let profile_display_cache = saved
            .v2ray_profiles
            .iter()
            .map(|p| {
                (
                    p.id.clone(),
                    (
                        v2ray::extract_node_address(&p.config),
                        v2ray::extract_protocol_tag(&p.config),
                    ),
                )
            })
            .collect();
        let cached_app_chips = apps::extract_app_chips(&saved.whitelist_apps);
        let profile_count = saved.v2ray_profiles.len();
        let has_profiles = !saved.v2ray_profiles.is_empty();

        let app = Self {
            view_mode: ViewMode::Profiles,
            show_overflow_menu: false,
            profile_sort: ProfileSort::Name,
            usage_records: storage::load_usage(),
            usage_date_filter: "All dates".to_string(),
            usage_stop_pending: false,
            skip_final_usage_once: false,
            usage_query_pending: false,
            is_running: false,
            connection_error: false,
            connected_at: None,
            ping_ms: None,
            tunnel_mode,

            payload_text: saved.payload_text,
            bug_host: saved.bug_host,
            sni_host: saved.sni_host,
            gen_keep_alive: true,
            gen_online_host: true,
            gen_split: false,

            ssh_host: saved.ssh_host,
            ssh_port: saved.ssh_port,
            ssh_user: saved.ssh_user,
            ssh_pass: saved.ssh_pass,
            proxy_ip: saved.proxy_ip,
            proxy_port: saved.proxy_port,
            local_socks_port: saved.local_socks_port,

            v2ray_uri_or_json: active_uri,
            v2ray_profiles: saved.v2ray_profiles,
            active_profile_id: active_id,
            node_edit_state: v2ray::NodeEditState::default(),
            ping_in_progress: None,
            ping_results: HashMap::new(),
            show_add_menu: false,

            run_at_startup: saved.run_at_startup,
            connect_at_startup: saved.connect_at_startup,
            auto_reconnect: saved.auto_reconnect,
            notify_connection_drops: saved.notify_connection_drops,
            allow_insecure_tls: saved.allow_insecure_tls,
            ipv6_enabled: saved.ipv6_enabled,
            bypass_lan: saved.bypass_lan,
            dns_servers: saved.dns_servers,
            user_stopped: false,
            reconnect_attempts: 0,
            error_notification: None,
            startup_connect_prompt: saved.connect_at_startup && has_profiles,
            qr_scan_modal: QrScanModalState::default(),
            update_state: updater::UpdateState::Idle,
            show_release_notes: false,
            auto_check_updates: saved.auto_check_updates,
            logs_enabled: saved.logs_enabled,

            enable_tun: saved.enable_tun,
            enable_sysproxy: saved.enable_sysproxy,
            whitelist_apps: saved.whitelist_apps,
            whitelist_urls: saved.whitelist_urls,
            available_apps: apps::get_curated_windows_apps(),
            selected_app_preset: None,
            public_ip: None,
            is_checking_ip: false,
            instant_import: saved.instant_import,
            clipboard_import_candidate: None,
            share_modal: None,
            theme_preference: saved.theme_preference,
            profile_display_cache,
            cached_app_chips,
            status_badge_text: "DISCONNECTED".to_string(),
            profile_count_label: format!("{} profiles", profile_count),

            logs: if saved.logs_enabled { vec![LogEntry::new(
                "Netray",
                "Ready to connect on Windows.",
                LogLevel::Info,
            )] } else { Vec::new() },
            auto_scroll_logs: true,
            logs_filter: String::new(),
            logs_preset: LogsFilterPreset::All,
            log_sender: None,
            stop_signal: None,
            close_confirmation: None,
            delete_confirmation: None,
            window_id: None,
            hidden_to_tray: false,
            exit_after_stop: false,
            tray_icon: None,
            tray_connect_item: None,
            worker_active: false,
        };

        let initial_task = if app.auto_check_updates {
            Task::perform(
                updater::check_for_updates(APP_VERSION),
                Message::UpdateCheckFinished,
            )
        } else {
            Task::none()
        };

        (app, initial_task)
    }

    pub fn refresh_status_badge(&mut self) {
        if self.connection_error && !self.user_stopped {
            self.status_badge_text = "CONNECTING · CONNECTION ERROR".into();
            return;
        }
        if self.is_running {
            let uptime = format_uptime(
                self.connected_at.map(|at| at.elapsed()).unwrap_or_default(),
            );
            self.status_badge_text = self.ping_ms.map_or_else(
                || format!("CONNECTED · {uptime}"),
                |ms| format!("CONNECTED · {uptime} · {ms} ms"),
            );
            return;
        }

        let is_reconnecting = self
            .error_notification
            .as_ref()
            .map_or(false, |n| n.countdown_secs.is_some() || n.is_reconnecting);

        self.status_badge_text = if is_reconnecting {
            if let Some(secs) = self.error_notification.as_ref().and_then(|n| n.countdown_secs) {
                format!("CONNECTING · RETRY IN {}s", secs)
            } else {
                "CONNECTING...".to_string()
            }
        } else if self.worker_active {
            "CONNECTING · CHECKING SERVER".to_string()
        } else {
            "DISCONNECTED".to_string()
        };
    }

    pub fn sync_profile_display_cache(&mut self) {
        self.profile_display_cache.retain(|id, _| self.v2ray_profiles.iter().any(|p| &p.id == id));
        for p in &self.v2ray_profiles {
            self.profile_display_cache.entry(p.id.clone()).or_insert_with(|| {
                (
                    v2ray::extract_node_address(&p.config),
                    v2ray::extract_protocol_tag(&p.config),
                )
            });
        }
        self.profile_count_label = format!("{} profiles", self.v2ray_profiles.len());
    }

    pub fn cache_profile_display(&mut self, profile: &storage::V2RayProfile) {
        self.profile_display_cache.insert(
            profile.id.clone(),
            (
                v2ray::extract_node_address(&profile.config),
                v2ray::extract_protocol_tag(&profile.config),
            ),
        );
        self.profile_count_label = format!("{} profiles", self.v2ray_profiles.len());
    }

    pub fn refresh_app_chips(&mut self) {
        self.cached_app_chips = apps::extract_app_chips(&self.whitelist_apps);
    }

    pub fn is_dark(&self) -> bool {
        theme::resolve_effective_theme(self.theme_preference).is_dark()
    }

    pub fn theme(&self) -> Theme {
        theme::resolve_effective_theme(self.theme_preference).to_iced_theme()
    }

    pub fn text_primary(&self) -> Color {
        if self.is_dark() {
            theme::TEXT_PRIMARY_DARK
        } else {
            theme::TEXT_PRIMARY_LIGHT
        }
    }

    pub fn bg_window(&self) -> Color {
        if self.is_dark() {
            theme::BG_WINDOW_DARK
        } else {
            theme::BG_WINDOW_LIGHT
        }
    }

    fn save_current_state(&self) -> Result<std::path::PathBuf, String> {
        let mode_str = format!("{:?}", self.tunnel_mode);
        let app_ref = storage::AppDataRef {
            v2ray_profiles: &self.v2ray_profiles,
            active_profile_id: self.active_profile_id.as_deref(),
            v2ray_uri_or_json: &self.v2ray_uri_or_json,
            local_socks_port: &self.local_socks_port,
            enable_tun: self.enable_tun,
            enable_sysproxy: self.enable_sysproxy,
            tunnel_mode: &mode_str,
            ssh_host: &self.ssh_host,
            ssh_port: &self.ssh_port,
            ssh_user: &self.ssh_user,
            ssh_pass: &self.ssh_pass,
            proxy_ip: &self.proxy_ip,
            proxy_port: &self.proxy_port,
            payload_text: &self.payload_text,
            bug_host: &self.bug_host,
            sni_host: &self.sni_host,
            whitelist_apps: &self.whitelist_apps,
            whitelist_urls: &self.whitelist_urls,
            instant_import: self.instant_import,
            theme_preference: self.theme_preference,
            run_at_startup: self.run_at_startup,
            connect_at_startup: self.connect_at_startup,
            auto_reconnect: self.auto_reconnect,
            notify_connection_drops: self.notify_connection_drops,
            allow_insecure_tls: self.allow_insecure_tls,
            ipv6_enabled: self.ipv6_enabled,
            bypass_lan: self.bypass_lan,
            dns_servers: &self.dns_servers,
            auto_check_updates: self.auto_check_updates,
            logs_enabled: self.logs_enabled,
        };
        storage::save_state_ref(&app_ref)
    }

    fn push_log(&mut self, source: &str, message: impl Into<String>, level: LogLevel) {
        if !self.logs_enabled && !matches!(source, "Supervisor" | "SSH" | "Connection") { return; }
        let message = message.into();
        if !log_activity::should_store(self.logs_enabled, source, &message) { return; }
        self.logs.push(LogEntry::new(source, message, level));
        if self.logs.len() > 150 {
            self.logs.drain(0..30);
        }
    }

    fn config_locked(&self) -> bool {
        self.is_running || self.worker_active
    }

    fn handle_detected_error(&mut self, err_msg: &str) -> Task<Message> {
        if self.user_stopped || self.worker_active {
            return Task::none();
        }
        if let Some(notif) = &self.error_notification {
            if notif.countdown_secs.is_some() || notif.is_reconnecting {
                return Task::none();
            }
        }

        self.reconnect_attempts = self.reconnect_attempts.saturating_add(1);
        let attempt = self.reconnect_attempts;

        let display_err = format_friendly_error(err_msg);

        // Transition status from Connected to Connecting when error occurs
        self.is_running = false;
        self.refresh_status_badge();

        if self.auto_reconnect && attempt <= MAX_RECONNECT_ATTEMPTS {
            self.error_notification = Some(ErrorNotification {
                title: "Reconnecting to Tunnel".to_string(),
                message: display_err,
                attempt,
                max_attempts: MAX_RECONNECT_ATTEMPTS,
                countdown_secs: Some(RECONNECT_COUNTDOWN_SECONDS),
                is_reconnecting: false,
            });

            self.push_log(
                "Supervisor",
                format!(
                    "Automatic reconnect scheduled in {} seconds (Attempt {} of {})...",
                    RECONNECT_COUNTDOWN_SECONDS, attempt, MAX_RECONNECT_ATTEMPTS
                ),
                LogLevel::Warn,
            );
        } else {
            let reason = if !self.auto_reconnect {
                "Auto-reconnect disabled in settings."
            } else {
                "Maximum retry attempts reached. Please check node configuration."
            };
            self.refresh_status_badge();
            self.error_notification = Some(ErrorNotification {
                title: "Tunnel Error".to_string(),
                message: format!("{} ({})", display_err, reason),
                attempt,
                max_attempts: MAX_RECONNECT_ATTEMPTS,
                countdown_secs: None,
                is_reconnecting: false,
            });
            self.push_log("Supervisor", reason, LogLevel::Error);
        }

        self.refresh_status_badge();
        Task::none()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        let scroll = matches!(
            message,
            Message::NewLog(_) | Message::LogBatch(_)
                | Message::PingFinished(..)
                | Message::SetViewMode(ViewMode::Logs)
                | Message::AutoScrollLogs(true)
        );
        let previous_connection = (self.is_running, self.worker_active);
        let previous_error = self.connection_error;
        let previous_tray = tray_status::Status::for_connection(self.is_running, self.connection_error, self.user_stopped);
        let tray_opened = matches!(message, Message::WindowOpened(_));
        let task = self.update_message(message);
        log_activity::set_detailed(!self.hidden_to_tray && self.view_mode == ViewMode::Logs);
        if self.connection_error && !previous_error && !self.user_stopped {
            if self.notify_connection_drops {
                notifications::send_desktop_notification(
                    "Netray connection issue",
                    "A tunnel connection failed. Check the Logs page for details.",
                    notifications::NotificationUrgency::Normal,
                );
            }
            self.push_log("Connection", "Connection check or core request failed; keeping the tunnel running. Use Ping to verify recovery.", LogLevel::Error);
        }
        let current_tray = tray_status::Status::for_connection(self.is_running, self.connection_error, self.user_stopped);
        if tray_opened || previous_tray != current_tray {
            self.refresh_status_badge();
            if let Some(tray) = &self.tray_icon {
                let result = current_tray.icon().and_then(|icon| tray.set_icon(Some(icon)).map_err(|e| e.to_string()));
                if let Err(error) = result {
                    self.push_log("Tray", format!("Could not update status icon: {error}"), LogLevel::Warn);
                }
            }
        }

        if tray_opened || previous_connection != (self.is_running, self.worker_active) {
            if let Some(item) = &self.tray_connect_item {
                item.set_text(if self.is_running { "Disconnect" } else { "Connect" });
                item.set_enabled(self.is_running || !self.worker_active);
            }
        }
        if scroll && !self.hidden_to_tray && self.auto_scroll_logs && self.view_mode == ViewMode::Logs {
            task.chain(scrollable::snap_to(
                scrollable::Id::new("logs"),
                scrollable::RelativeOffset::END,
            ))
        } else {
            task
        }
    }

    fn update_message(&mut self, message: Message) -> Task<Message> {
        if self.config_locked()
            && matches!(
                &message,
                Message::SetViewMode(ViewMode::EditNode)
                    | Message::SelectProfile(_)
                    | Message::SelectProfileIndex(_)
                    | Message::EditProfile(_)
                    | Message::EditProfileIndex(_)
                    | Message::ConfirmDeleteProfile
                    | Message::DeleteProfile(_)
                    | Message::DeleteProfileIndex(_)
                    | Message::AddNewProfile
                    | Message::OpenAddMenu
                    | Message::CreateProfileWithType(_)
                    | Message::SaveEditedNode
                    | Message::RequestFileImport
                    | Message::ConfigFileSelected(_)
                    | Message::RequestClipboardImport
                    | Message::ClipboardImported(_)
                    | Message::ToggleInstantImport(_)
                    | Message::ConfirmClipboardImport
                    | Message::CancelClipboardImport
                    | Message::ClipboardCandidateChanged(_)
                    | Message::EditRemarksChanged(_)
                    | Message::EditAddressChanged(_)
                    | Message::EditPortChanged(_)
                    | Message::EditEncryptMethodChanged(_)
                    | Message::EditFlowChanged(_)
                    | Message::EditUserIdChanged(_)
                    | Message::EditTransportChanged(_)
                    | Message::EditHeaderTypeChanged(_)
                    | Message::EditTlsTypeChanged(_)
                    | Message::EditSniChanged(_)
                    | Message::EditFingerprintChanged(_)
                    | Message::EditAlpnChanged(_)
                    | Message::EditPathChanged(_)
                    | Message::EditPublicKeyChanged(_)
                    | Message::EditShortIdChanged(_)
                    | Message::OpenQrScanModal
                    | Message::TriggerQrScan(_)
                    | Message::TunnelModeChanged(_)
                    | Message::PayloadChanged(_)
                    | Message::BugHostChanged(_)
                    | Message::SshHostChanged(_)
                    | Message::SshPortChanged(_)
                    | Message::SshUserChanged(_)
                    | Message::SshPassChanged(_)
                    | Message::ProxyIpChanged(_)
                    | Message::ProxyPortChanged(_)
                    | Message::LocalSocksPortChanged(_)
                    | Message::WhitelistAppsChanged(_)
                    | Message::WhitelistUrlsChanged(_)
                    | Message::ToggleKeepAlive(_)
                    | Message::ToggleOnlineHost(_)
                    | Message::ToggleSplit(_)
                    | Message::GeneratePayload
            )
        {
            return Task::none();
        }
        match message {
            Message::SetViewMode(mode) => {
                self.show_overflow_menu = false;
                if mode == ViewMode::SshSettings {
                    self.usage_records = storage::load_usage();
                }
                self.view_mode = mode;
                Task::none()
            }
            Message::ToggleOverflowMenu => {
                self.show_overflow_menu = !self.show_overflow_menu;
                Task::none()
            }
            Message::ProfileSortChanged(sort) => {
                self.profile_sort = sort;
                self.show_overflow_menu = false;
                Task::none()
            }
            Message::UsageTick => {
                if self.worker_active && self.tunnel_mode == TunnelMode::V2RayXray {
                    if self.usage_query_pending {
                        return Task::none();
                    }
                    self.usage_query_pending = true;
                    Task::perform(v2ray::query_xray_traffic(), Message::UsageUpdated)
                } else {
                    self.usage_records = storage::load_usage();
                    Task::none()
                }
            }
            Message::UsageDateChanged(date) => {
                self.usage_date_filter = date;
                Task::none()
            }
            Message::UsageUpdated(Ok((uploaded, downloaded))) => {
                self.usage_query_pending = false;
                self.usage_records = storage::record_usage(uploaded, downloaded);
                if self.usage_stop_pending {
                    self.usage_stop_pending = false;
                    if self.is_running {
                        self.skip_final_usage_once = true;
                        self.update_message(Message::ToggleTunnel)
                    } else {
                        Task::none()
                    }
                } else {
                    Task::none()
                }
            }
            Message::UsageUpdated(Err(_)) => {
                self.usage_query_pending = false;
                if self.usage_stop_pending {
                    self.usage_stop_pending = false;
                    if self.is_running {
                        self.skip_final_usage_once = true;
                        self.update_message(Message::ToggleTunnel)
                    } else {
                        Task::none()
                    }
                } else {
                    Task::none()
                }
            }
            Message::TunnelModeChanged(mode) => {
                self.tunnel_mode = mode;
                let _ = self.save_current_state();
                Task::none()
            }
            Message::SelectProfileIndex(idx) => {
                if let Some(p) = self.v2ray_profiles.get(idx) {
                    let id = p.id.clone();
                    return self.update_message(Message::SelectProfile(id));
                }
                Task::none()
            }
            Message::PingProfileIndex(idx) => {
                if let Some(p) = self.v2ray_profiles.get(idx) {
                    let id = p.id.clone();
                    return self.update_message(Message::PingProfile(id));
                }
                Task::none()
            }
            Message::ShareProfileIndex(idx) => {
                if let Some(p) = self.v2ray_profiles.get(idx) {
                    let id = p.id.clone();
                    return self.update_message(Message::ShareProfile(id));
                }
                Task::none()
            }
            Message::EditProfileIndex(idx) => {
                if let Some(p) = self.v2ray_profiles.get(idx) {
                    let id = p.id.clone();
                    return self.update_message(Message::EditProfile(id));
                }
                Task::none()
            }
            Message::DeleteProfileIndex(idx) => {
                if let Some(p) = self.v2ray_profiles.get(idx) {
                    let id = p.id.clone();
                    return self.update_message(Message::DeleteProfile(id));
                }
                Task::none()
            }
            Message::SelectProfile(id) => {
                if let Some(p) = self.v2ray_profiles.iter().find(|p| p.id == id) {
                    self.v2ray_uri_or_json = p.config.clone();
                    self.active_profile_id = Some(id);
                    let _ = self.save_current_state();
                }
                Task::none()
            }
            Message::PingProfile(id) => {
                let active = self
                    .v2ray_profiles
                    .iter()
                    .find(|profile| profile.id == id)
                    .is_some_and(|profile| {
                        self.active_profile_id.as_deref() == Some(&id)
                            || self.v2ray_uri_or_json == profile.config
                    });
                if self.ping_in_progress.is_some()
                    || (self.worker_active
                        && !(self.is_running
                            && self.tunnel_mode == TunnelMode::V2RayXray
                            && active))
                {
                    Task::none()
                } else if let Some(config) = self
                    .v2ray_profiles
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| p.config.clone())
                {
                    self.ping_results.remove(&id);
                    self.ping_in_progress = Some(id.clone());
                    let local_proxy =
                        (self.is_running && self.tunnel_mode == TunnelMode::V2RayXray).then(|| {
                            self.local_socks_port
                                .parse::<u16>()
                                .unwrap_or(10808)
                                .saturating_add(1)
                        });
                    let allow_insecure_tls = self.allow_insecure_tls;
                    Task::perform(
                        async move {
                            if let Some(port) = local_proxy {
                                v2ray::ping_local_http_proxy(port).await
                            } else {
                                v2ray::ping_profile(&config, allow_insecure_tls).await
                            }
                        },
                        move |result| Message::PingFinished(id.clone(), result),
                    )
                } else {
                    Task::none()
                }
            }
            Message::PingFinished(id, result) => {
                if self.ping_in_progress.as_deref() == Some(&id) {
                    self.ping_in_progress = None;
                    let ping_latency = result.as_ref().ok().copied();
                    let (display, log, level) = match result {
                        Ok(ms) => (
                            format!("HTTP · {ms} ms"),
                            format!(
                                "{}: proxy response in {ms} ms",
                                self.v2ray_profiles
                                    .iter()
                                    .find(|p| p.id == id)
                                    .map(|p| p.name.as_str())
                                    .unwrap_or("Profile")
                            ),
                            LogLevel::Success,
                        ),
                        Err(error) => (
                            "Failed".to_string(),
                            format!(
                                "Ping failed for {}: {error}",
                                self.v2ray_profiles
                                    .iter()
                                    .find(|p| p.id == id)
                                    .map(|p| p.name.as_str())
                                    .unwrap_or("profile")
                            ),
                            LogLevel::Warn,
                        ),
                    };
                    let active = self
                        .v2ray_profiles
                        .iter()
                        .find(|profile| profile.id == id)
                        .is_some_and(|profile| {
                            self.active_profile_id.as_deref() == Some(&id)
                                || self.v2ray_uri_or_json == profile.config
                        });
                    if active && self.is_running {
                        self.connection_error = ping_latency.is_none();
                        self.ping_ms = ping_latency;
                    }
                    self.ping_results.insert(id, display);
                    self.push_log("Ping", log, level);
                }
                Task::none()
            }
            Message::ShareProfile(id) => {
                if let Some(p) = self.v2ray_profiles.iter().find(|p| p.id == id).cloned() {
                    let link = v2ray::generate_share_link(&p.config, &p.name);
                    match qr::generate_qr_svg(&link) {
                        Ok(svg_str) => {
                            self.share_modal = Some(ShareModalState {
                                profile_id: p.id.clone(),
                                profile_name: p.name.clone(),
                                link,
                                qr_svg: svg_str.into_bytes(),
                                is_protected: false,
                                copied: false,
                            });
                            self.push_log(
                                "Share",
                                format!("Generated QR code and shareable link for '{}'.", p.name),
                                LogLevel::Success,
                            );
                        }
                        Err(err) => {
                            self.push_log(
                                "Share",
                                format!("Failed to generate QR code: {err}"),
                                LogLevel::Warn,
                            );
                        }
                    }
                }
                Task::none()
            }
            Message::CloseShareModal => {
                self.share_modal = None;
                Task::none()
            }
            Message::CopyShareLink => {
                let link = if let Some(modal) = &mut self.share_modal {
                    modal.copied = true;
                    Some(modal.link.clone())
                } else {
                    None
                };
                if let Some(link) = link {
                    self.push_log(
                        "Share",
                        "Copied configuration link to clipboard.",
                        LogLevel::Success,
                    );
                    iced::clipboard::write(link)
                } else {
                    Task::none()
                }
            }
            Message::ToggleShareProtect => {
                if let Some(modal) = &mut self.share_modal {
                    modal.is_protected = !modal.is_protected;
                }
                Task::none()
            }
            Message::EditProfile(id) => {
                if let Some(p) = self.v2ray_profiles.iter().find(|p| p.id == id) {
                    self.node_edit_state =
                        v2ray::NodeEditState::from_profile(&p.id, &p.name, &p.config);
                    self.view_mode = ViewMode::EditNode;
                }
                Task::none()
            }
            Message::AddNewProfile => {
                let id = generate_unique_node_id("node");
                let (name, config) = protocols::ProfileType::VLess.generate_template();
                self.node_edit_state =
                    v2ray::NodeEditState::new_for_setup(&id, "VLess", &name, &config);
                self.view_mode = ViewMode::EditNode;
                Task::none()
            }
            Message::OpenAddMenu => {
                if !self.is_running && !self.worker_active {
                    self.show_add_menu = true;
                }
                Task::none()
            }
            Message::CloseAddMenu => {
                self.show_add_menu = false;
                Task::none()
            }
            Message::CreateProfileWithType(proto) => {
                self.show_add_menu = false;
                let id = generate_unique_node_id("node");
                let (name, config) = proto.generate_template();
                self.node_edit_state =
                    v2ray::NodeEditState::new_for_setup(&id, proto.title(), &name, &config);
                self.push_log(
                    "Netray",
                    format!("Opening setup for new {} profile...", proto.title()),
                    LogLevel::Info,
                );
                self.view_mode = ViewMode::EditNode;
                Task::none()
            }
            Message::EditRemarksChanged(v) => {
                self.node_edit_state.remarks = v;
                Task::none()
            }
            Message::EditAddressChanged(v) => {
                self.node_edit_state.address = v;
                Task::none()
            }
            Message::EditPortChanged(v) => {
                self.node_edit_state.port = v;
                Task::none()
            }
            Message::EditEncryptMethodChanged(v) => {
                self.node_edit_state.encrypt_method = v;
                Task::none()
            }
            Message::EditFlowChanged(v) => {
                self.node_edit_state.flow = v;
                Task::none()
            }
            Message::EditUserIdChanged(v) => {
                self.node_edit_state.user_id = v;
                Task::none()
            }
            Message::EditTransportChanged(v) => {
                self.node_edit_state.transport_protocol = v;
                Task::none()
            }
            Message::EditHeaderTypeChanged(v) => {
                self.node_edit_state.header_type = v;
                Task::none()
            }
            Message::EditTlsTypeChanged(v) => {
                self.node_edit_state.tls_type = v;
                Task::none()
            }
            Message::EditSniChanged(v) => {
                self.node_edit_state.sni = v;
                Task::none()
            }
            Message::EditFingerprintChanged(v) => {
                self.node_edit_state.fingerprint = v;
                Task::none()
            }
            Message::EditAlpnChanged(v) => {
                self.node_edit_state.alpn = v;
                Task::none()
            }
            Message::EditPathChanged(v) => {
                self.node_edit_state.path = v;
                Task::none()
            }
            Message::EditPublicKeyChanged(v) => {
                self.node_edit_state.public_key = v;
                Task::none()
            }
            Message::EditShortIdChanged(v) => {
                self.node_edit_state.short_id = v;
                Task::none()
            }
            Message::CancelEditNode => {
                if self.node_edit_state.is_new {
                    self.push_log(
                        "Netray",
                        "Cancelled new profile setup.",
                        LogLevel::Info,
                    );
                }
                self.node_edit_state = v2ray::NodeEditState::default();
                self.view_mode = ViewMode::Profiles;
                Task::none()
            }
            Message::SaveEditedNode => {
                let is_new = self.node_edit_state.is_new;
                let id = self.node_edit_state.id.clone();
                let config = self.node_edit_state.to_config();
                let name = if self.node_edit_state.remarks.trim().is_empty() {
                    v2ray::extract_node_name(&config)
                } else {
                    self.node_edit_state.remarks.trim().to_string()
                };
                self.ping_results.remove(&id);
                if self.ping_in_progress.as_deref() == Some(&id) {
                    self.ping_in_progress = None;
                }

                if let Some(existing) = self.v2ray_profiles.iter_mut().find(|p| p.id == id) {
                    existing.name = name.clone();
                    existing.config = config.clone();
                } else {
                    let profile = storage::V2RayProfile {
                        id: id.clone(),
                        name: name.clone(),
                        config: config.clone(),
                        created_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                    };
                    self.v2ray_profiles.insert(0, profile);
                }

                self.active_profile_id = Some(id);
                self.v2ray_uri_or_json = config;
                self.sync_profile_display_cache();
                let _ = self.save_current_state();
                if is_new {
                    self.push_log(
                        "Config",
                        format!("Setup complete: added profile '{}'.", name),
                        LogLevel::Success,
                    );
                } else {
                    self.push_log(
                        "Config",
                        format!("Saved node '{}'.", name),
                        LogLevel::Success,
                    );
                }
                self.node_edit_state = v2ray::NodeEditState::default();
                self.view_mode = ViewMode::Profiles;
                Task::none()
            }
            Message::ToggleInstantImport(v) => {
                self.instant_import = v;
                let _ = self.save_current_state();
                Task::none()
            }
            Message::CancelClipboardImport => {
                self.clipboard_import_candidate = None;
                Task::none()
            }
            Message::ClipboardCandidateChanged(v) => {
                self.clipboard_import_candidate = Some(v);
                Task::none()
            }
            Message::ConfirmClipboardImport => {
                if let Some(candidate) = self.clipboard_import_candidate.take() {
                    let trimmed = candidate.trim();
                    if !trimmed.is_empty() {
                        let node_name = v2ray::extract_node_name(trimmed);
                        let id = generate_unique_node_id("node");
                        let profile = storage::V2RayProfile {
                            id: id.clone(),
                            name: node_name.clone(),
                            config: trimmed.to_string(),
                            created_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                        };
                        self.v2ray_profiles.retain(|p| p.config != trimmed);
                        self.v2ray_profiles.insert(0, profile);
                        self.active_profile_id = Some(id);

                        self.v2ray_uri_or_json = trimmed.to_string();
                        self.tunnel_mode = TunnelMode::V2RayXray;

                        self.sync_profile_display_cache();
                        let _ = self.save_current_state();
                        self.push_log(
                            "Clipboard",
                            format!("Imported and activated '{}'.", node_name),
                            LogLevel::Success,
                        );
                    }
                }
                Task::none()
            }
            Message::RequestFileImport => {
                Task::perform(v2ray::pick_and_read_config_file(), Message::ConfigFileSelected)
            }
            Message::ConfigFileSelected(maybe_file) => {
                if let Some((file_name, content)) = maybe_file {
                    let parsed_configs = v2ray::parse_imported_configs(&content, Some(&file_name));
                    if parsed_configs.is_empty() {
                        self.push_log(
                            "Import",
                            format!("No valid V2Ray or Xray configurations found in '{}'.", file_name),
                            LogLevel::Warn,
                        );
                    } else {
                        let count = parsed_configs.len();
                        let mut first_id = None;
                        let mut first_cfg = None;
                        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

                        for (_idx, (node_name, config_str)) in parsed_configs.into_iter().rev().enumerate() {
                            let id = generate_unique_node_id("node");
                            let profile = storage::V2RayProfile {
                                id: id.clone(),
                                name: node_name.clone(),
                                config: config_str.clone(),
                                created_at: now.clone(),
                            };
                            self.v2ray_profiles.retain(|p| p.config != config_str);
                            self.v2ray_profiles.insert(0, profile);
                            first_id = Some(id);
                            first_cfg = Some(config_str);
                        }

                        if let (Some(id), Some(cfg)) = (first_id, first_cfg) {
                            self.active_profile_id = Some(id);
                            self.v2ray_uri_or_json = cfg;
                            self.tunnel_mode = TunnelMode::V2RayXray;
                        }

                        self.sync_profile_display_cache();
                        let _ = self.save_current_state();

                        let msg = format!("Imported {} profile(s) from '{}'.", count, file_name);
                        self.push_log("Import", &msg, LogLevel::Success);
                        notifications::send_desktop_notification(
                            "Netray Import",
                            &msg,
                            notifications::NotificationUrgency::Normal,
                        );
                    }
                }
                Task::none()
            }
            Message::RequestClipboardImport => {
                iced::clipboard::read().map(Message::ClipboardImported)
            }
            Message::ClipboardImported(content) => {
                if let Some(text) = content {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        if self.instant_import {
                            let node_name = v2ray::extract_node_name(trimmed);
                            let id = generate_unique_node_id("node");
                            let profile = storage::V2RayProfile {
                                id: id.clone(),
                                name: node_name.clone(),
                                config: trimmed.to_string(),
                                created_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                            };
                            self.v2ray_profiles.retain(|p| p.config != trimmed);
                            self.v2ray_profiles.insert(0, profile);
                            self.active_profile_id = Some(id);

                            self.v2ray_uri_or_json = trimmed.to_string();
                            self.tunnel_mode = TunnelMode::V2RayXray;

                            self.sync_profile_display_cache();
                            let _ = self.save_current_state();
                            self.push_log(
                                "Clipboard",
                                format!("Imported and activated '{}'.", node_name),
                                LogLevel::Success,
                            );
                        } else {
                            self.clipboard_import_candidate = Some(trimmed.to_string());
                        }
                    } else {
                        self.push_log(
                            "Clipboard",
                            "Clipboard is empty.".to_string(),
                            LogLevel::Warn,
                        );
                    }
                } else {
                    self.push_log(
                        "Clipboard",
                        "Clipboard is empty or inaccessible.".to_string(),
                        LogLevel::Warn,
                    );
                }
                Task::none()
            }
            Message::DeleteProfile(id) => {
                self.delete_confirmation = self.v2ray_profiles.iter()
                    .find(|profile| profile.id == id).map(|profile| profile.id.clone());
                Task::none()
            }
            Message::CancelDeleteProfile => {
                self.delete_confirmation = None;
                Task::none()
            }
            Message::ConfirmDeleteProfile => {
                let Some(id) = self.delete_confirmation.take() else { return Task::none(); };
                self.v2ray_profiles.retain(|p| p.id != id);
                self.profile_display_cache.remove(&id);
                self.ping_results.remove(&id);
                if self.ping_in_progress.as_deref() == Some(&id) {
                    self.ping_in_progress = None;
                }
                if self.active_profile_id.as_deref() == Some(&id) {
                    self.active_profile_id = self.v2ray_profiles.first().map(|p| p.id.clone());
                    self.v2ray_uri_or_json = self.v2ray_profiles.first()
                        .map(|profile| profile.config.clone()).unwrap_or_default();
                }
                self.profile_count_label = format!("{} profiles", self.v2ray_profiles.len());
                let _ = self.save_current_state();
                Task::none()
            }
            Message::ToggleTunMode(v) => {
                if self.is_running || self.worker_active {
                    return Task::none();
                }
                self.enable_tun = v;
                let _ = self.save_current_state();
                Task::none()
            }
            Message::ToggleSysProxy(v) => {
                if self.is_running || self.worker_active {
                    return Task::none();
                }
                self.enable_sysproxy = v;
                let _ = self.save_current_state();
                Task::none()
            }
            Message::WhitelistAppsChanged(val) => {
                self.whitelist_apps = val;
                self.refresh_app_chips();
                let _ = self.save_current_state();
                Task::none()
            }
            Message::WhitelistUrlsChanged(val) => {
                self.whitelist_urls = val;
                let _ = self.save_current_state();
                Task::none()
            }
            Message::PickAppSelected(preset) => {
                self.whitelist_apps = apps::add_app_to_whitelist(&self.whitelist_apps, &preset.executable);
                self.selected_app_preset = None;
                self.refresh_app_chips();
                let _ = self.save_current_state();
                Task::none()
            }
            Message::BrowseAppExecutable => {
                Task::perform(apps::pick_executable_file(), Message::AppExecutablePicked)
            }
            Message::AppExecutablePicked(maybe_bin) => {
                if let Some(bin) = maybe_bin {
                    self.whitelist_apps = apps::add_app_to_whitelist(&self.whitelist_apps, &bin);
                    self.refresh_app_chips();
                    let _ = self.save_current_state();
                }
                Task::none()
            }
            Message::RemoveWhitelistApp(app_name) => {
                self.whitelist_apps = apps::remove_app_from_whitelist(&self.whitelist_apps, &app_name);
                self.refresh_app_chips();
                let _ = self.save_current_state();
                Task::none()
            }
            Message::CheckPublicIp => {
                self.is_checking_ip = true;
                Task::perform(v2ray::check_public_ip(), Message::PublicIpChecked)
            }
            Message::PublicIpChecked(res) => {
                self.is_checking_ip = false;
                match res {
                    Ok(ip) => self.public_ip = Some(ip),
                    Err(e) => self.public_ip = Some(e),
                }
                Task::none()
            }
            Message::PayloadChanged(v) => {
                self.payload_text = v;
                Task::none()
            }
            Message::BugHostChanged(v) => {
                self.bug_host = v;
                Task::none()
            }
            Message::SniChanged(v) => {
                self.sni_host = v;
                Task::none()
            }
            Message::ToggleKeepAlive(v) => {
                self.gen_keep_alive = v;
                Task::none()
            }
            Message::ToggleOnlineHost(v) => {
                self.gen_online_host = v;
                Task::none()
            }
            Message::ToggleSplit(v) => {
                self.gen_split = v;
                Task::none()
            }
            Message::GeneratePayload => {
                let mut tpl = String::new();
                if self.gen_split {
                    tpl.push_str("GET / HTTP/1.1[crlf][split]");
                } else {
                    tpl.push_str("CONNECT [host_port] [protocol][crlf]");
                }
                tpl.push_str(&format!("Host: {}[crlf]", self.bug_host));
                if self.gen_online_host {
                    tpl.push_str("X-Online-Host: [host][crlf]");
                }
                if self.gen_keep_alive {
                    tpl.push_str("Connection: Keep-Alive[crlf]");
                }
                tpl.push_str("User-Agent: Netray-Windows/1.0[crlf][crlf]");
                self.payload_text = tpl;
                Task::none()
            }
            Message::SshHostChanged(v) => {
                self.ssh_host = v;
                Task::none()
            }
            Message::SshPortChanged(v) => {
                self.ssh_port = v;
                Task::none()
            }
            Message::SshUserChanged(v) => {
                self.ssh_user = v;
                Task::none()
            }
            Message::SshPassChanged(v) => {
                self.ssh_pass = v;
                Task::none()
            }
            Message::ProxyIpChanged(v) => {
                self.proxy_ip = v;
                Task::none()
            }
            Message::ProxyPortChanged(v) => {
                self.proxy_port = v;
                Task::none()
            }
            Message::V2RayConfigChanged(v) => {
                self.v2ray_uri_or_json = v;
                Task::none()
            }
            Message::LocalSocksPortChanged(v) => {
                self.local_socks_port = v;
                Task::none()
            }
            Message::LoggerReady(sender) => {
                self.log_sender = Some(sender);
                Task::none()
            }
            Message::LogBatch(entries) => {
                Task::batch(entries.into_iter().map(|entry| self.update_message(Message::NewLog(entry))).collect::<Vec<_>>())
            }
            Message::NewLog(entry) => {
                if self.worker_active && !self.user_stopped
                    && ((entry.level == LogLevel::Error && matches!(entry.source.as_str(), "Xray" | "Error" | "DNS"))
                        || (entry.source == "Supervisor" && entry.message.starts_with("Connection not yet verified:")))
                {
                    self.connection_error = true;
                }
                if self.worker_active && !self.user_stopped && self.connection_error
                    && entry.source == "Xray"
                    && entry.message.contains("proxy/vless/outbound: tunneling request to")
                {
                    self.connection_error = false;
                    self.refresh_status_badge();
                }
                let ready_ping = (entry.source == "Supervisor")
                    .then(|| {
                        entry
                            .message
                            .strip_prefix("Tunnel ready · HTTP 204 through proxy in ")
                    })
                    .flatten()
                    .and_then(|value| value.strip_suffix(" ms."))
                    .and_then(|value| value.parse::<u128>().ok());
                let ssh_ready =
                    entry.source == "SSH" && entry.message == "SSH Authenticated successfully!";
                if (ready_ping.is_some() || ssh_ready) && self.worker_active && !self.user_stopped {
                    self.is_running = true;
                    self.connection_error = false;
                    self.connected_at = Some(Instant::now());
                    self.ping_ms = ready_ping;
                    self.reconnect_attempts = 0;
                    if let Some(notif) = &self.error_notification {
                        if notif.is_reconnecting && self.notify_connection_drops {
                            notifications::send_desktop_notification(
                                "Netray Connected",
                                "Tunnel reconnected successfully.",
                                notifications::NotificationUrgency::Normal,
                            );
                        }
                    }
                    self.error_notification = None;
                    self.refresh_status_badge();
                }
                let stopped = entry.source == "Supervisor" && entry.message.contains("offline");
                let was_active = self.worker_active;
                if stopped && was_active && !self.user_stopped {
                    self.connection_error = true;
                }
                if stopped || (entry.source == "Error" && entry.message.contains("Engine failed")) {
                    self.is_running = false;
                    self.connected_at = None;
                    self.ping_ms = None;
                    self.refresh_status_badge();
                }
                // Only the final offline event means proxy and route cleanup has finished.
                if stopped {
                    self.stop_signal = None;
                    self.worker_active = false;
                    self.refresh_status_badge();
                }

                // Per-request DNS/HTTP errors do not mean the core has stopped.
                let auto_task = if stopped && was_active && !self.user_stopped {
                    if let Some(notif) = &mut self.error_notification {
                        notif.is_reconnecting = false;
                    }
                    self.handle_detected_error(&entry.message)
                } else {
                    Task::none()
                };

                if log_activity::should_store(self.logs_enabled, &entry.source, &entry.message) {
                    self.logs.push(entry);
                    if self.logs.len() > 150 {
                        self.logs.drain(0..30);
                    }
                }
                if stopped && self.exit_after_stop {
                    self.exit_after_stop = false;
                    self.close_confirmation
                        .take()
                        .map(iced::window::close)
                        .unwrap_or_else(Task::none)
                } else {
                    auto_task
                }
            }
            Message::AutoScrollLogs(enabled) => {
                self.auto_scroll_logs = enabled;
                Task::none()
            }
            Message::LogsFilterChanged(query) => {
                self.logs_filter = query;
                Task::none()
            }
            Message::LogsPresetChanged(preset) => {
                self.logs_preset = preset;
                Task::none()
            }
            Message::ClearLogs => {
                self.logs.clear();
                Task::none()
            }
            Message::CopyLogs => {
                let query = self.logs_filter.trim().to_lowercase();
                let filtered_lines: Vec<String> = self
                    .logs
                    .iter()
                    .filter(|entry| {
                        let matches_preset = match self.logs_preset {
                            LogsFilterPreset::All => true,
                            LogsFilterPreset::Tunnel => entry
                                .route
                                .as_deref()
                                .is_some_and(|r| !r.eq_ignore_ascii_case("bypass") && !r.eq_ignore_ascii_case("direct")),
                            LogsFilterPreset::Bypass => entry
                                .route
                                .as_deref()
                                .is_some_and(|r| r.eq_ignore_ascii_case("bypass") || r.eq_ignore_ascii_case("direct")),
                            LogsFilterPreset::Apps => entry.app_name.is_some(),
                            LogsFilterPreset::Errors => matches!(entry.level, LogLevel::Error | LogLevel::Warn),
                        };
                        if !matches_preset {
                            return false;
                        }

                        if query.is_empty() {
                            return true;
                        }
                        entry.message.to_lowercase().contains(&query)
                            || entry.source.to_lowercase().contains(&query)
                            || entry
                                .app_name
                                .as_deref()
                                .is_some_and(|a| a.to_lowercase().contains(&query))
                            || entry
                                .target_host
                                .as_deref()
                                .is_some_and(|h| h.to_lowercase().contains(&query))
                            || entry
                                .route
                                .as_deref()
                                .is_some_and(|r| r.to_lowercase().contains(&query))
                    })
                    .map(|e| {
                        let mut meta = Vec::new();
                        if let Some(ref app) = e.app_name {
                            meta.push(format!("[app:{app}]"));
                        }
                        if let (Some(ref host), Some(port)) = (&e.target_host, e.target_port) {
                            meta.push(format!("[dest:{host}:{port}]"));
                        }
                        if let Some(ref route) = e.route {
                            meta.push(format!("[route:{route}]"));
                        }
                        let meta_str = if meta.is_empty() {
                            String::new()
                        } else {
                            format!(" {} ", meta.join(" "))
                        };
                        format!("[{}][{}]{}{}", e.timestamp, e.source, meta_str, e.message)
                    })
                    .collect();

                let count = filtered_lines.len();
                let output = filtered_lines.join("\r\n");
                self.push_log(
                    "Logs",
                    format!("Copied {count} log entries to clipboard."),
                    LogLevel::Success,
                );
                iced::clipboard::write(output)
            }
            Message::UptimeTick => {
                self.refresh_status_badge();
                Task::none()
            }
            Message::CloseRequested(id) => {
                self.close_confirmation = Some(id);
                Task::none()
            }
            Message::WindowOpened(id) => {
                self.window_id = Some(id);
                if self.tray_icon.is_none() {
                    match create_tray_icon() {
                        Ok((icon, connect)) => {
                            self.tray_icon = Some(icon);
                            self.tray_connect_item = Some(connect);
                        }
                        Err(error) => self.push_log(
                            "Tray",
                            format!("System tray unavailable: {error}"),
                            LogLevel::Warn,
                        ),
                    }
                }
                Task::none()
            }
            Message::TrayExit => {
                self.close_confirmation = self.window_id;
                self.update_message(Message::TrayShow)
            }
            Message::TrayShow => {
                self.hidden_to_tray = false;
                if let Some(id) = self.window_id {
                    iced::window::change_mode(id, iced::window::Mode::Windowed)
                        .chain(iced::window::minimize(id, false))
                        .chain(iced::window::gain_focus(id))
                } else { Task::none() }
            }
            Message::WindowTick => {
                if self.hidden_to_tray {
                    Task::none()
                } else if let Some(id) = self.window_id {
                    // shortcut: Wayland does not report minimized state; add a native shell hook if Wayland tray minimization is needed.
                    iced::window::get_minimized(id).map(Message::Minimized)
                } else {
                    Task::none()
                }
            }
            Message::Minimized(Some(true)) => {
                if self.tray_icon.is_some() {
                    self.hidden_to_tray = true;
                    self.window_id
                        .map(|id| iced::window::change_mode(id, iced::window::Mode::Hidden))
                        .unwrap_or_else(Task::none)
                } else {
                    Task::none()
                }
            }
            Message::Minimized(_) => Task::none(),
            Message::MinimizeToTray => {
                if self.tray_icon.is_some() {
                    self.hidden_to_tray = true;
                    self.window_id
                        .map(|id| iced::window::change_mode(id, iced::window::Mode::Hidden))
                        .unwrap_or_else(Task::none)
                } else {
                    Task::none()
                }
            }
            Message::ConfirmExit => {
                if self.worker_active {
                    self.exit_after_stop = true;
                    if let Some(stop) = self.stop_signal.take() {
                        let _ = stop.send(());
                    }
                    Task::none()
                } else {
                    self.close_confirmation
                        .take()
                        .map(iced::window::close)
                        .unwrap_or_else(Task::none)
                }
            }
            Message::ThemePreferenceChanged(pref) => {
                theme::invalidate_system_theme_cache();
                self.theme_preference = pref;
                self.push_log(
                    "SYSTEM",
                    format!("Theme appearance set to: {}", pref),
                    LogLevel::Info,
                );
                let _ = self.save_current_state();
                Task::none()
            }
            Message::ToggleRunAtStartup(enabled) => {
                self.run_at_startup = enabled;
                if let Err(e) = autostart::set_run_at_startup(enabled) {
                    self.push_log(
                        "Supervisor",
                        format!("Failed to configure autostart: {e}"),
                        LogLevel::Error,
                    );
                } else {
                    let msg = if enabled {
                        "Run at startup enabled."
                    } else {
                        "Run at startup disabled."
                    };
                    self.push_log("Supervisor", msg, LogLevel::Info);
                }
                let _ = self.save_current_state();
                Task::none()
            }
            Message::ToggleConnectAtStartup(enabled) => {
                self.connect_at_startup = enabled;
                let msg = if enabled {
                    "Connect at startup enabled. Netray will prompt to connect on launch."
                } else {
                    "Connect at startup disabled."
                };
                self.push_log("Supervisor", msg, LogLevel::Info);
                let _ = self.save_current_state();
                Task::none()
            }
            Message::DismissStartupConnectPrompt => {
                self.startup_connect_prompt = false;
                self.push_log(
                    "Supervisor",
                    "Startup connection dismissed.",
                    LogLevel::Info,
                );
                Task::none()
            }
            Message::ConfirmStartupConnectPrompt => {
                self.startup_connect_prompt = false;
                self.push_log(
                    "Supervisor",
                    "Target node confirmed. Connecting tunnel...",
                    LogLevel::Info,
                );
                self.update_message(Message::ToggleTunnel)
            }
            Message::CancelExit => {
                self.close_confirmation = None;
                Task::none()
            }
            Message::ToggleAutoReconnect(enabled) => {
                self.auto_reconnect = enabled;
                if !enabled {
                    if let Some(notif) = &mut self.error_notification {
                        notif.countdown_secs = None;
                    }
                    self.refresh_status_badge();
                }
                let msg = if enabled {
                    "Automatic restart after tunnel exit enabled."
                } else {
                    "Automatic restart after tunnel exit disabled."
                };
                self.push_log("Supervisor", msg, LogLevel::Info);
                let _ = self.save_current_state();
                Task::none()
            }
            Message::DnsServersChanged(value) => {
                if self.config_locked() { return Task::none(); }
                self.dns_servers = value;
                let _ = self.save_current_state();
                Task::none()
            }
            Message::ToggleBypassLan(enabled) => {
                if self.config_locked() { return Task::none(); }
                self.bypass_lan = enabled;
                let _ = self.save_current_state();
                Task::none()
            }
            Message::ToggleIpv6(enabled) => {
                if self.config_locked() { return Task::none(); }
                self.ipv6_enabled = enabled;
                let _ = self.save_current_state();
                Task::none()
            }
            Message::ToggleAllowInsecureTls(enabled) => {
                if self.is_running || self.worker_active { return Task::none(); }
                self.allow_insecure_tls = enabled;
                let _ = self.save_current_state();
                Task::none()
            }
            Message::ToggleConnectionDropNotifications(enabled) => {
                self.notify_connection_drops = enabled;
                self.push_log(
                    "Settings",
                    if enabled {
                        "Connection drop desktop notifications enabled."
                    } else {
                        "Connection drop desktop notifications disabled."
                    },
                    LogLevel::Info,
                );
                let _ = self.save_current_state();
                Task::none()
            }
            Message::ToggleLogsEnabled(enabled) => {
                self.logs_enabled = enabled;
                log_activity::set_enabled(enabled);
                if !enabled {
                    self.logs.clear();
                    if self.is_running {
                        self.push_log("Connection", "Tunnel connected.", LogLevel::Success);
                    }
                }
                let _ = self.save_current_state();
                Task::none()
            }
            Message::ReconnectCountdownTick => {
                if let Some(notif) = &mut self.error_notification {
                    if let Some(secs) = notif.countdown_secs {
                        if secs > 1 {
                            notif.countdown_secs = Some(secs - 1);
                            self.refresh_status_badge();
                            return Task::none();
                        }
                        return self.update_message(Message::ExecuteAutoReconnect);
                    }
                }
                Task::none()
            }
            Message::ExecuteAutoReconnect => {
                // Never replace a live worker: Xray retries outbound connections itself.
                if self.worker_active || self.user_stopped {
                    return Task::none();
                }
                if let Some(notif) = &mut self.error_notification {
                    notif.countdown_secs = None;
                    notif.is_reconnecting = true;
                }
                self.update_message(Message::ToggleTunnel)
            }
            Message::CancelAutoReconnect => {
                self.error_notification = None;
                self.reconnect_attempts = 0;
                self.user_stopped = true;
                self.push_log(
                    "Supervisor",
                    "Automatic reconnect cancelled by user.",
                    LogLevel::Warn,
                );
                Task::none()
            }
            Message::DismissNotification => {
                self.error_notification = None;
                Task::none()
            }
            Message::CheckForUpdates => {
                if matches!(self.update_state,
                    updater::UpdateState::Checking | updater::UpdateState::Downloading { .. }
                    | updater::UpdateState::Downloaded { .. } | updater::UpdateState::Applying
                    | updater::UpdateState::ReadyToRestart { .. })
                {
                    return Task::none();
                }
                self.update_state = updater::UpdateState::Checking;
                Task::perform(
                    updater::check_for_updates(APP_VERSION),
                    Message::UpdateCheckFinished,
                )
            }
            Message::UpdateCheckFinished(res) => {
                match res {
                    Ok(Some(info)) => {
                        let ver = info.version.clone();
                        self.show_release_notes = true;
                        self.update_state = updater::UpdateState::Available(info);
                        notifications::send_desktop_notification(
                            "Netray Update Available",
                            &format!("Version v{} is available. Open Netray to upgrade.", ver),
                            notifications::NotificationUrgency::Normal,
                        );
                        self.push_log("Updater", format!("New Netray release v{} detected on GitHub CI.", ver), LogLevel::Info);
                    }
                    Ok(None) => {
                        self.update_state = updater::UpdateState::UpToDate(APP_VERSION.to_string());
                        self.push_log("Updater", format!("Netray is up to date (v{}).", APP_VERSION), LogLevel::Info);
                    }
                    Err(e) => {
                        self.push_log("Updater", format!("Update check failed: {}", e), LogLevel::Warn);
                        self.update_state = updater::UpdateState::Error(e);
                    }
                }
                Task::none()
            }
            Message::StartInAppUpgrade => {
                if let updater::UpdateState::Available(info) = &self.update_state {
                    self.show_release_notes = false;
                    let info_clone = info.clone();
                    let ver = info.version.clone();
                    self.update_state = updater::UpdateState::Downloading {
                        version: ver.clone(),
                        status: "Downloading update package from GitHub...".to_string(),
                    };
                    self.push_log("Updater", format!("Downloading Netray v{} release artifact...", ver), LogLevel::Info);
                    Task::perform(
                        updater::download_and_extract_update(info_clone),
                        Message::UpdateDownloadFinished,
                    )
                } else {
                    Task::none()
                }
            }
            Message::UpdateDownloadFinished(res) => {
                match res {
                    Ok(staged_binary) => {
                        let ver = match &self.update_state {
                            updater::UpdateState::Downloading { version, .. } => version.clone(),
                            _ => "latest".to_string(),
                        };
                        self.push_log("Updater", format!("Update v{} downloaded & staged. Applying...", ver), LogLevel::Success);
                        if let Err(e) = updater::apply_update_and_restart(&staged_binary, None) {
                            self.push_log("Updater", format!("Failed to apply update: {}", e), LogLevel::Error);
                            self.update_state = updater::UpdateState::Error(e);
                        }
                    }
                    Err(e) => {
                        self.push_log("Updater", format!("Update download failed: {}", e), LogLevel::Error);
                        self.update_state = updater::UpdateState::Error(e);
                    }
                }
                Task::none()
            }
            Message::ApplyUpdateAndRestart => {
                if let updater::UpdateState::Downloaded { staged_binary, .. } = &self.update_state {
                    if let Err(e) = updater::apply_update_and_restart(staged_binary, None) {
                        self.update_state = updater::UpdateState::Error(e);
                    }
                }
                Task::none()
            }
            Message::DismissUpdateToast => {
                self.update_state = updater::UpdateState::Idle;
                Task::none()
            }
            Message::CloseReleaseNotesModal => {
                self.show_release_notes = false;
                Task::none()
            }
            Message::ToggleAutoCheckUpdates(v) => {
                self.auto_check_updates = v;
                let _ = self.save_current_state();
                Task::none()
            }
            Message::ToggleTunnel => {
                self.delete_confirmation = None;
                self.connection_error = false;
                if self.is_running && self.tunnel_mode == TunnelMode::V2RayXray {
                    if self.usage_stop_pending {
                        return Task::none();
                    }
                    if self.usage_query_pending {
                        self.usage_stop_pending = true;
                        return Task::none();
                    }
                    if !self.skip_final_usage_once {
                        self.usage_stop_pending = true;
                        self.usage_query_pending = true;
                        return Task::perform(v2ray::query_xray_traffic(), Message::UsageUpdated);
                    }
                }
                self.skip_final_usage_once = false;
                if self.worker_active && !self.is_running {
                    if let Some(stop) = self.stop_signal.take() {
                        let _ = stop.send(());
                    }
                    self.user_stopped = true;
                    self.error_notification = None;
                    self.reconnect_attempts = 0;
                    self.refresh_status_badge();
                    self.push_log(
                        "Supervisor",
                        "Connection cancelled while verifying the tunnel.",
                        LogLevel::Warn,
                    );
                    return Task::none();
                }
                if self.is_running {
                    if let Some(stop_tx) = self.stop_signal.take() {
                        let _ = stop_tx.send(());
                    }
                    self.user_stopped = true;
                    self.error_notification = None;
                    self.reconnect_attempts = 0;
                    self.is_running = false;
                    self.connected_at = None;
                    self.ping_ms = None;
                    self.refresh_status_badge();
                    self.push_log("Supervisor", "Tunnel stopped by user.", LogLevel::Warn);
                    Task::none()
                } else {
                    self.user_stopped = false;
                    self.is_running = false;
                    self.worker_active = true;
                    self.connected_at = None;
                    self.ping_ms = None;
                    self.refresh_status_badge();
                    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
                    self.stop_signal = Some(stop_tx);

                    let socks_port_u16 = self.local_socks_port.parse::<u16>().unwrap_or(10808);
                    let proxy_addr = if !self.proxy_ip.is_empty() && !self.proxy_port.is_empty() {
                        format!("{}:{}", self.proxy_ip, self.proxy_port)
                    } else {
                        "".to_string()
                    };

                    let cfg = TunnelConfig {
                        mode: self.tunnel_mode,
                        proxy_addr,
                        ssh_host: self.ssh_host.clone(),
                        ssh_port: self.ssh_port.parse::<u16>().unwrap_or(22),
                        ssh_user: self.ssh_user.clone(),
                        ssh_pass: self.ssh_pass.clone(),
                        bug_host: self.bug_host.clone(),
                        sni_host: self.sni_host.clone(),
                        payload_template: self.payload_text.clone(),
                        local_socks_port: socks_port_u16,
                        v2ray_config: self.v2ray_uri_or_json.clone(),
                        enable_tun: self.enable_tun,
                        enable_sysproxy: self.enable_sysproxy,
                        whitelist_apps: self.whitelist_apps.clone(),
                        whitelist_urls: self.whitelist_urls.clone(),
                        allow_insecure_tls: self.allow_insecure_tls,
            ipv6_enabled: self.ipv6_enabled,
            bypass_lan: self.bypass_lan,
                            dns_servers: self.dns_servers.clone(),
                    };

                    let log_tx_opt = self.log_sender.clone();
                    self.push_log(
                        "Supervisor",
                        format!("Starting [{}] on Windows...", self.tunnel_mode),
                        LogLevel::Info,
                    );

                    Task::perform(
                        async move {
                            if let Some(tx) = log_tx_opt {
                                run_real_tunnel_worker(tx, stop_rx, cfg).await;
                            }
                        },
                        |_| {
                            Message::NewLog(LogEntry::new(
                                "Supervisor",
                                "Tunnel stopped.",
                                LogLevel::Warn,
                            ))
                        },
                    )
                }
            }
            Message::OpenQrScanModal => {
                self.qr_scan_modal.is_open = true;
                self.qr_scan_modal.status = QrScanStatus::Idle;
                Task::none()
            }
            Message::CloseQrScanModal => {
                self.qr_scan_modal.is_open = false;
                self.qr_scan_modal.status = QrScanStatus::Idle;
                Task::none()
            }
            Message::TriggerQrScan(source) => {
                self.qr_scan_modal.status = QrScanStatus::Scanning(source);
                match source {
                    QrScanSource::DesktopScreenshot => {
                        Task::perform(
                            async {
                                tokio::time::sleep(Duration::from_millis(150)).await;
                                qr_scanner::scan_qr_from_desktop()
                            },
                            Message::QrScanFinished,
                        )
                    }
                    QrScanSource::Webcam => {
                        Task::perform(
                            async {
                                qr_scanner::scan_qr_from_webcam()
                            },
                            Message::QrScanFinished,
                        )
                    }
                    QrScanSource::PhotoUpload => {
                        Task::perform(
                            async {
                                match qr_scanner::scan_qr_from_picked_file() {
                                    Ok(Some(content)) => Ok(content),
                                    Ok(None) => Err("File selection was cancelled".to_string()),
                                    Err(e) => Err(e),
                                }
                            },
                            Message::QrScanFinished,
                        )
                    }
                }
            }
            Message::QrScanFinished(result) => {
                match result {
                    Ok(scanned_content) => {
                        let trimmed = scanned_content.trim();
                        let parsed_configs = v2ray::parse_imported_configs(trimmed, Some("QR Scan"));
                        let configs_to_add: Vec<(String, String)> = if !parsed_configs.is_empty() {
                            parsed_configs
                        } else if trimmed.contains("://") || trimmed.starts_with('{') {
                            let remark = v2ray::extract_node_name(trimmed);
                            let name = if remark.is_empty() {
                                format!("Scanned Node {}", self.v2ray_profiles.len() + 1)
                            } else {
                                remark
                            };
                            vec![(name, trimmed.to_string())]
                        } else {
                            vec![]
                        };

                        if configs_to_add.is_empty() {
                            let err_msg = format!("Scanned QR code is not a valid V2Ray/Xray link or configuration: '{}'", trimmed);
                            self.push_log("Scanner", &err_msg, LogLevel::Warn);
                            self.qr_scan_modal.status = QrScanStatus::Error("Scanned QR is not a valid proxy profile".to_string());
                        } else {
                            let count = configs_to_add.len();
                            let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                            let mut first_id = None;
                            let mut first_cfg = None;
                            let mut imported_names = Vec::new();

                            for (_idx, (node_name, config_str)) in configs_to_add.into_iter().rev().enumerate() {
                                let id = generate_unique_node_id("qr-node");
                                let profile = storage::V2RayProfile {
                                    id: id.clone(),
                                    name: node_name.clone(),
                                    config: config_str.clone(),
                                    created_at: now.clone(),
                                };
                                self.v2ray_profiles.retain(|p| p.config != config_str);
                                self.v2ray_profiles.insert(0, profile);
                                imported_names.push(node_name);
                                first_id = Some(id);
                                first_cfg = Some(config_str);
                            }

                            if let (Some(id), Some(cfg)) = (first_id, first_cfg) {
                                self.active_profile_id = Some(id);
                                self.v2ray_uri_or_json = cfg;
                                self.tunnel_mode = TunnelMode::V2RayXray;
                            }

                            self.sync_profile_display_cache();
                            let _ = self.save_current_state();

                            let summary = if count == 1 {
                                imported_names.first().cloned().unwrap_or_else(|| "Node".to_string())
                            } else {
                                format!("{} nodes", count)
                            };

                            self.qr_scan_modal.status = QrScanStatus::Success(format!("Imported {}", summary));
                            self.push_log(
                                "Scanner",
                                format!("QR Code scan successful! Imported {} profile(s).", count),
                                LogLevel::Info,
                            );
                            notifications::send_desktop_notification(
                                "Netray QR Scanner",
                                &format!("Successfully imported {} from QR code.", summary),
                                notifications::NotificationUrgency::Normal,
                            );
                        }
                    }
                    Err(e) => {
                        if e == "File selection was cancelled" {
                            self.qr_scan_modal.status = QrScanStatus::Idle;
                        } else {
                            self.push_log("Scanner", format!("QR Code scan error: {e}"), LogLevel::Warn);
                            self.qr_scan_modal.status = QrScanStatus::Error(e);
                        }
                    }
                }
                Task::none()
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let logs = Subscription::run(|| {
            iced::stream::channel(100, |mut output| async move {
                let (tx, mut rx) = mpsc::channel::<LogEntry>(256);
                let _ = output.send(Message::LoggerReady(tx)).await;
                while let Some(entry) = rx.recv().await {
                    if entry.source != "Xray" || !matches!(entry.level, LogLevel::Info) {
                        if output.send(Message::NewLog(entry)).await.is_err() { break; }
                        continue;
                    }
                    let delay = if log_activity::detailed() { 200 } else { 1000 };
                    let deadline = tokio::time::Instant::now() + Duration::from_millis(delay);
                    let mut batch = vec![entry];
                    let mut urgent = None;
                    while batch.len() < 128 {
                        match tokio::time::timeout_at(deadline, rx.recv()).await {
                            Ok(Some(entry)) if entry.source == "Xray" && matches!(entry.level, LogLevel::Info) => batch.push(entry),
                            Ok(Some(entry)) => { urgent = Some(entry); break; }
                            _ => break,
                        }
                    }
                    if output.send(Message::LogBatch(batch)).await.is_err() { break; }
                    if let Some(entry) = urgent {
                        if output.send(Message::NewLog(entry)).await.is_err() { break; }
                    }
                }
            })
        });
        let mut subscriptions = vec![
            logs,
            iced::window::close_requests().map(Message::CloseRequested),
            iced::window::open_events().map(Message::WindowOpened),
            Subscription::run(|| {
                iced::stream::channel(16, |mut output| async move {
                    let (tx, mut rx) = iced::futures::channel::mpsc::unbounded();
                    let clicks = tx.clone();
                    TrayIconEvent::set_event_handler(Some(move |event| {
                        if matches!(
                            event,
                            TrayIconEvent::Click {
                                button: MouseButton::Left,
                                ..
                            } | TrayIconEvent::DoubleClick { .. }
                        ) {
                            let _ = clicks.unbounded_send(Message::TrayShow);
                        }
                    }));
                    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                        let message = match event.id().as_ref() {
                            "show" => Message::TrayShow,
                            "hide" => Message::MinimizeToTray,
                            "connect" => Message::ToggleTunnel,
                            "exit" => Message::TrayExit,
                            _ => return,
                        };
                        let _ = tx.unbounded_send(message);
                    }));
                    while let Some(message) = rx.next().await {
                        if output.send(message).await.is_err() {
                            break;
                        }
                    }
                })
            }),

        ];
        if !self.hidden_to_tray {
            subscriptions.push(iced::time::every(Duration::from_secs(1)).map(|_| Message::WindowTick));
        }
        if !self.hidden_to_tray && self.is_running && self.view_mode == ViewMode::Profiles {
            subscriptions
                .push(iced::time::every(Duration::from_secs(1)).map(|_| Message::UptimeTick));
        }
        if self.worker_active {
            subscriptions.push(iced::time::every(Duration::from_secs(60)).map(|_| Message::UsageTick));
        }
        if let Some(notif) = &self.error_notification {
            if notif.countdown_secs.is_some() {
                subscriptions
                    .push(iced::time::every(Duration::from_secs(1)).map(|_| Message::ReconnectCountdownTick));
            }
        }
        Subscription::batch(subscriptions)
    }

    fn view(&self) -> Element<'_, Message> {
        let content = match self.view_mode {
            ViewMode::Profiles => self.view_profiles_screen(),
            ViewMode::EditNode => self.view_edit_screen(),
            ViewMode::Logs => self.view_logs_screen(),
            ViewMode::SshSettings => self.view_ssh_screen(),
        };
        let mut banners = column![].spacing(2);
        if !matches!(self.update_state, updater::UpdateState::Idle | updater::UpdateState::UpToDate(_)) {
            banners = banners.push(self.view_update_toast());
        }
        let main_col = column![banners, content]
            .width(Length::Fill)
            .height(Length::Fill);
        let content = container(main_col)
            .style(bg_window_style)
            .width(Length::Fill)
            .height(Length::Fill);
        let base_content: Element<'_, Message> = if self.show_add_menu {
            iced::widget::stack![
                content,
                self.view_add_profile_modal(),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            content.into()
        };

        let base_content = if self.clipboard_import_candidate.is_some() {
            iced::widget::stack![
                base_content,
                self.view_clipboard_import_modal(),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            base_content
        };

        let base_content = if self.share_modal.is_some() {
            iced::widget::stack![
                base_content,
                self.view_share_modal(),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            base_content
        };

        let base_content = if self.startup_connect_prompt {
            iced::widget::stack![
                base_content,
                self.view_startup_connect_prompt_modal(),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            base_content
        };

        let base_content = if self.qr_scan_modal.is_open {
            iced::widget::stack![
                base_content,
                self.view_qr_scan_modal(),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            base_content
        };

        let base_content = if let Some(id) = &self.delete_confirmation {
            let name = self.v2ray_profiles.iter().find(|profile| &profile.id == id)
                .map(|profile| profile.name.as_str()).unwrap_or("this profile");
            let dialog = container(
                column![
                    text("Delete V2Ray profile?").size(20),
                    text(format!("Delete “{name}”? This cannot be undone.")).size(14),
                    row![
                        button("Cancel").on_press(Message::CancelDeleteProfile),
                        button("Delete").style(button::danger).on_press(Message::ConfirmDeleteProfile),
                    ].spacing(10),
                ].spacing(16).padding(20),
            ).style(form_card_style).width(Length::Fixed(340.0));
            iced::widget::stack![
                base_content,
                iced::widget::opaque(container(dialog)
                    .width(Length::Fill).height(Length::Fill)
                    .center_x(Length::Fill).center_y(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.62).into()),
                        ..Default::default()
                    })),
            ].width(Length::Fill).height(Length::Fill).into()
        } else {
            base_content
        };

        if self.show_release_notes {
            if let updater::UpdateState::Available(info) = &self.update_state {
                let mut notes = column![].spacing(5);
                let content = if info.release_notes.trim().is_empty() {
                    "No release notes were provided."
                } else {
                    &info.release_notes
                };
                for line in content.lines() {
                    let line = line.trim();
                    if line.is_empty() {
                        notes = notes.push(Space::with_height(4));
                    } else if let Some(heading) = line
                        .strip_prefix("### ")
                        .or_else(|| line.strip_prefix("## "))
                        .or_else(|| line.strip_prefix("# "))
                    {
                        notes = notes.push(
                            text(updater::clean_markdown_inline(heading))
                                .size(13)
                                .color(ACCENT_BLUE_LIGHT),
                        );
                    } else if let Some(bullet) = line
                        .strip_prefix("- ")
                        .or_else(|| line.strip_prefix("* "))
                    {
                        notes = notes.push(
                            row![
                                text("•").color(ACCENT_BLUE),
                                Space::with_width(6),
                                text(updater::clean_markdown_inline(bullet)).size(12),
                            ]
                            .align_y(Alignment::Start),
                        );
                    } else {
                        notes = notes.push(text(updater::clean_markdown_inline(line)).size(12));
                    }
                }
                let dialog = container(
                    column![
                        row![
                            text(format!("Netray v{} is available", info.version))
                                .size(17)
                                .color(Color::WHITE)
                                .width(Length::Fill),
                            button("×").on_press(Message::CloseReleaseNotesModal),
                        ]
                        .align_y(Alignment::Center),
                        text("Here’s what changed in this release:")
                            .size(11)
                            .color(TEXT_MUTED),
                        container(scrollable(notes.padding(10)).height(Length::Fixed(220.0)))
                            .style(form_card_style)
                            .width(Length::Fill),
                        row![
                            Space::with_width(Length::Fill),
                            button("Later").on_press(Message::CloseReleaseNotesModal),
                            button("Upgrade now")
                                .style(primary_action_btn)
                                .on_press(Message::StartInAppUpgrade),
                        ]
                        .spacing(8),
                    ]
                    .spacing(12)
                    .padding(18),
                )
                .style(form_card_style)
                .width(Length::Fixed(420.0));
                return iced::widget::stack![
                    base_content,
                    container(dialog)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .center_x(Length::Fill)
                        .center_y(Length::Fill)
                        .style(|_| container::Style {
                            background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.70).into()),
                            ..Default::default()
                        }),
                ]
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
            }
        }
        if self.close_confirmation.is_some() {
            let is_dark = self.is_dark();
            let dialog = container(column![
                text("Stop Netray and exit?").size(20).color(self.text_primary()),
                text("The active tunnel will be stopped before the app closes.")
                    .size(14).color(settings_ui::secondary(is_dark)),
                row![
                    Space::with_width(Length::Fill),
                    button(text("Cancel").size(14).color(self.text_primary())).padding([10, 16])
                        .style(rounded_icon_btn_style(is_dark, true)).on_press(Message::CancelExit),
                    button(text("Stop and exit").size(14).color(Color::WHITE)).padding([10, 16])
                        .style(|_, status| button::Style {
                            background: Some(if matches!(status, button::Status::Hovered) { Color::from_rgb8(179, 59, 77) } else { Color::from_rgb8(164, 45, 62) }.into()),
                            text_color: Color::WHITE,
                            border: iced::Border { radius: 999.0.into(), ..Default::default() },
                            ..Default::default()
                        }).on_press(Message::ConfirmExit),
                ].spacing(8).align_y(Alignment::Center),
            ].spacing(20)).padding(24).width(Length::Fill).max_width(360).style(settings_ui::menu_surface);
            iced::widget::stack![
                base_content,
                container(dialog)
                    .padding(16)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.62).into()),
                        ..Default::default()
                    }),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            base_content
        }
    }

    fn view_clipboard_import_modal(&self) -> Element<'_, Message> {
        let candidate = self.clipboard_import_candidate.as_deref().unwrap_or("");
        let is_dark = self.is_dark();
        let muted = settings_ui::secondary(is_dark);
        let preview = container(
            column![
                text("Configuration")
                    .size(13)
                    .color(settings_ui::accent(is_dark)),
                scrollable(
                    text(candidate)
                        .size(13)
                        .color(self.text_primary())
                        .width(Length::Fill)
                )
                .height(140),
            ]
            .spacing(10),
        )
        .padding(16)
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(
                if is_dark {
                    Color::from_rgb8(25, 31, 41)
                } else {
                    Color::from_rgb8(248, 250, 254)
                }
                .into(),
            ),
            border: iced::Border {
                radius: 16.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });
        let actions = row![
            Space::with_width(Length::Fill),
            button(text("Cancel").size(14).color(self.text_primary()))
                .padding([10, 16])
                .style(rounded_icon_btn_style(is_dark, true))
                .on_press(Message::CancelClipboardImport),
            button(text("Import").size(14))
                .padding([10, 20])
                .style(primary_action_btn)
                .on_press(Message::ConfirmClipboardImport),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let dialog = container(
            column![
                text("Import from clipboard")
                    .size(20)
                    .color(self.text_primary()),
                text("Review the configuration before adding it.")
                    .size(13)
                    .color(muted),
                preview,
                actions,
            ]
            .spacing(16),
        )
        .padding(24)
        .width(Length::Fill)
        .max_width(400)
        .style(settings_ui::menu_surface);
        container(dialog)
            .padding(16)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(|_| container::Style {
                background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.62).into()),
                ..Default::default()
            })
            .into()
    }


    fn view_share_modal(&self) -> Element<'_, Message> {
        let Some(modal) = &self.share_modal else {
            return Space::with_width(0).into();
        };
        let is_dark = self.is_dark();
        let muted = settings_ui::secondary(is_dark);
        let accent = settings_ui::accent(is_dark);
        let header = row![
            column![
                text("Share profile").size(20).color(self.text_primary()),
                text(&modal.profile_name).size(13).color(muted)
            ]
            .spacing(4)
            .width(Length::Fill),
            button(icons::lucide_icon(icons::CLOSE, 18, self.text_primary()))
                .padding(8)
                .style(rounded_icon_btn_style(is_dark, true))
                .on_press(Message::CloseShareModal),
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        let qr_box = container(
            iced::widget::svg(iced::widget::svg::Handle::from_memory(modal.qr_svg.clone()))
                .width(240)
                .height(240),
        )
        .padding(12)
        .center_x(Length::Fill)
        .style(|_| container::Style {
            background: Some(Color::WHITE.into()),
            border: iced::Border {
                radius: 16.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });
        let copy: Element<'_, Message> = iced::widget::tooltip(
            button(icons::lucide_icon(if modal.copied { icons::CHECK } else { icons::COPY }, 18, accent))
                .width(36).height(36).padding(8)
                .style(rounded_icon_btn_style(is_dark, true)).on_press(Message::CopyShareLink),
            text(if modal.copied { "Copied" } else { "Copy link" }).size(12),
            iced::widget::tooltip::Position::Bottom,
        ).into();
        let preview: String = modal.link.chars().take(52).collect();
        let preview = if preview.len() < modal.link.len() { format!("{preview}…") } else { preview };
        let link = container(
            row![
                column![
                    text("Share link").size(12).color(accent),
                    text(preview).size(12).color(self.text_primary()).width(Length::Fill),
                ]
                .spacing(6)
                .width(Length::Fill),
                copy,
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .padding(12)
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(
                if is_dark {
                    Color::from_rgb8(25, 31, 41)
                } else {
                    Color::from_rgb8(248, 250, 254)
                }
                .into(),
            ),
            border: iced::Border {
                radius: 12.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });
        let body = scrollable(column![link, qr_box,
            text("Scan the QR code or copy the link. Use file export for complex configurations.").size(12).color(muted),
        ].spacing(12)).height(Length::Fill);
        let dialog = container(column![header, body].spacing(16))
            .padding(20)
            .width(Length::Fill)
            .max_width(360)
            .height(Length::Fill)
            .max_height(510)
            .style(settings_ui::menu_surface);
        container(dialog)
            .padding(16)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(|_| container::Style {
                background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.65).into()),
                ..Default::default()
            })
            .into()
    }


    fn view_startup_connect_prompt_modal(&self) -> Element<'_, Message> {
        let active_name = self
            .active_profile_id
            .as_ref()
            .and_then(|id| self.v2ray_profiles.iter().find(|p| &p.id == id))
            .map(|p| p.name.as_str())
            .unwrap_or("Active V2Ray Node");

        let dialog = container(
            column![
                row![
                    icons::lucide_icon(icons::ACTIVITY, 22, ACCENT_BLUE_LIGHT),
                    Space::with_width(8),
                    text("Connect to V2Ray").size(18).color(Color::WHITE),
                ]
                .align_y(Alignment::Center),
                Space::with_height(6),
                text("Do you want to connect to v2ray?")
                    .size(15)
                    .color(Color::WHITE),
                Space::with_height(2),
                text(format!("Target Node: {}", active_name))
                    .size(12)
                    .color(TEXT_MUTED),
                Space::with_height(14),
                row![
                    button(text("Cancel").size(13).color(Color::WHITE))
                        .padding(iced::Padding {
                            top: 8.0,
                            bottom: 8.0,
                            left: 16.0,
                            right: 16.0,
                        })
                        .style(|_, status| button::Style {
                            background: match status {
                                button::Status::Hovered | button::Status::Pressed => {
                                    Some(Color::from_rgb(0.25, 0.28, 0.35).into())
                                }
                                _ => Some(Color::from_rgb(0.18, 0.20, 0.25).into()),
                            },
                            border: iced::Border {
                                radius: 8.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        })
                        .on_press(Message::DismissStartupConnectPrompt),
                    Space::with_width(12),
                    button(text("Connect").size(13).color(Color::WHITE))
                        .padding(iced::Padding {
                            top: 8.0,
                            bottom: 8.0,
                            left: 20.0,
                            right: 20.0,
                        })
                        .style(primary_action_btn)
                        .on_press(Message::ConfirmStartupConnectPrompt),
                ]
                .align_y(Alignment::Center),
            ]
            .spacing(4)
            .padding(20)
            .width(360),
        )
        .style(form_card_style);

        container(dialog)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(|_| container::Style {
                background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.70).into()),
                ..Default::default()
            })
            .into()
    }


    #[allow(dead_code)]
    fn view_notification_toast(&self) -> Element<'_, Message> {
        let Some(notif) = &self.error_notification else {
            return Space::with_width(0).into();
        };
        let is_countdown = notif.countdown_secs.is_some();
        let status_desc = if let Some(secs) = notif.countdown_secs {
            format!("Auto-reconnecting in {}s (Attempt {} of {})...", secs, notif.attempt, notif.max_attempts)
        } else if notif.is_reconnecting {
            format!("Reconnecting to tunnel (Attempt {} of {})...", notif.attempt, notif.max_attempts)
        } else {
            format!("Attempt {} of {} failed.", notif.attempt, notif.max_attempts)
        };

        let action_row = if is_countdown {
            row![
                button(text("Cancel").size(11).color(Color::from_rgb(0.9, 0.9, 0.9)))
                    .padding(iced::Padding { top: 4.0, bottom: 4.0, left: 10.0, right: 10.0 })
                    .style(toast_cancel_btn)
                    .on_press(Message::CancelAutoReconnect),
                Space::with_width(6),
                button(text("Reconnect Now").size(11).color(Color::WHITE))
                    .padding(iced::Padding { top: 4.0, bottom: 4.0, left: 10.0, right: 10.0 })
                    .style(primary_action_btn)
                    .on_press(Message::ExecuteAutoReconnect),
            ]
            .align_y(Alignment::Center)
        } else if !notif.is_reconnecting {
            row![
                button(text("Retry Now").size(11).color(Color::WHITE))
                    .padding(iced::Padding { top: 4.0, bottom: 4.0, left: 12.0, right: 12.0 })
                    .style(primary_action_btn)
                    .on_press(Message::ExecuteAutoReconnect),
            ]
            .align_y(Alignment::Center)
        } else {
            row![
                text("Connecting...").size(11).color(ACCENT_BLUE_LIGHT),
            ]
            .align_y(Alignment::Center)
        };

        let toast = container(
            row![
                icons::lucide_icon(icons::REFRESH_CW, 16, WARNING_AMBER),
                Space::with_width(10),
                column![
                    row![
                        text(&notif.title).size(12).color(Color::WHITE),
                        Space::with_width(8),
                        container(text(format!("Retry {}/{}", notif.attempt, notif.max_attempts)).size(10).color(WARNING_AMBER))
                            .padding(iced::Padding { top: 1.0, bottom: 1.0, left: 6.0, right: 6.0 })
                            .style(toast_attempt_pill),
                    ]
                    .align_y(Alignment::Center),
                    row![
                        text(&notif.message).size(11).color(TEXT_MUTED),
                        Space::with_width(8),
                        text(status_desc).size(11).color(WARNING_AMBER),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                ]
                .spacing(2)
                .width(Length::Fill),
                Space::with_width(8),
                action_row,
                Space::with_width(6),
                button(icons::lucide_icon(icons::CLOSE, 14, TEXT_MUTED))
                    .padding(4)
                    .style(toast_close_btn)
                    .on_press(Message::DismissNotification),
            ]
            .align_y(Alignment::Center)
            .padding(iced::Padding { top: 6.0, bottom: 6.0, left: 12.0, right: 10.0 })
        )
        .style(toast_container_style)
        .width(Length::Fill);

        container(toast)
            .padding(iced::Padding {
                top: 4.0,
                bottom: 4.0,
                left: 12.0,
                right: 12.0,
            })
            .width(Length::Fill)
            .into()
    }

    fn view_update_toast(&self) -> Element<'_, Message> {
        match &self.update_state {
            updater::UpdateState::Available(info) => {
                let badge = container(text(format!("v{}", info.version)).size(10).color(SUCCESS_GREEN))
                    .padding(iced::Padding { top: 1.0, bottom: 1.0, left: 6.0, right: 6.0 })
                    .style(toast_attempt_pill);

                let toast = container(
                    row![
                        icons::lucide_icon(icons::DOWNLOAD, 16, SUCCESS_GREEN),
                        Space::with_width(10),
                        column![
                            row![
                                text("New Version Available").size(12).color(Color::WHITE),
                                Space::with_width(8),
                                badge,
                            ]
                            .align_y(Alignment::Center),
                            row![
                                text(format!("Netray v{} released via GitHub CI (Current: v{}).", info.version, APP_VERSION))
                                    .size(11)
                                    .color(TEXT_MUTED),
                            ]
                            .align_y(Alignment::Center),
                        ]
                        .spacing(2)
                        .width(Length::Fill),
                        Space::with_width(8),
                        row![
                            button(text("Upgrade Now").size(11).color(Color::WHITE))
                                .padding(iced::Padding { top: 4.0, bottom: 4.0, left: 12.0, right: 12.0 })
                                .style(primary_action_btn)
                                .on_press(Message::StartInAppUpgrade),
                            Space::with_width(6),
                            button(text("Later").size(11).color(Color::from_rgb(0.85, 0.85, 0.85)))
                                .padding(iced::Padding { top: 4.0, bottom: 4.0, left: 8.0, right: 8.0 })
                                .style(toast_cancel_btn)
                                .on_press(Message::DismissUpdateToast),
                        ]
                        .align_y(Alignment::Center),
                    ]
                    .align_y(Alignment::Center)
                    .padding(iced::Padding { top: 6.0, bottom: 6.0, left: 12.0, right: 10.0 })
                )
                .style(toast_container_style)
                .width(Length::Fill);

                container(toast)
                    .padding(iced::Padding { top: 4.0, bottom: 4.0, left: 12.0, right: 12.0 })
                    .width(Length::Fill)
                    .into()
            }
            updater::UpdateState::Downloading { version, status } => {
                let toast = container(
                    row![
                        icons::lucide_icon(icons::REFRESH_CW, 16, ACCENT_BLUE_LIGHT),
                        Space::with_width(10),
                        column![
                            text(format!("Upgrading to Netray v{version}...")).size(12).color(Color::WHITE),
                            text(status).size(11).color(ACCENT_BLUE_LIGHT),
                        ]
                        .spacing(2)
                        .width(Length::Fill),
                    ]
                    .align_y(Alignment::Center)
                    .padding(iced::Padding { top: 6.0, bottom: 6.0, left: 12.0, right: 10.0 })
                )
                .style(toast_container_style)
                .width(Length::Fill);

                container(toast)
                    .padding(iced::Padding { top: 4.0, bottom: 4.0, left: 12.0, right: 12.0 })
                    .width(Length::Fill)
                    .into()
            }
            updater::UpdateState::ReadyToRestart { version } => {
                let toast = container(
                    row![
                        icons::lucide_icon(icons::CHECK, 16, SUCCESS_GREEN),
                        Space::with_width(10),
                        column![
                            text(format!("Netray v{version} ready to launch!")).size(12).color(Color::WHITE),
                            text("Restart now to complete the upgrade.").size(11).color(SUCCESS_GREEN),
                        ]
                        .spacing(2)
                        .width(Length::Fill),
                        button(text("Restart Now").size(11).color(Color::WHITE))
                            .padding(iced::Padding { top: 4.0, bottom: 4.0, left: 12.0, right: 12.0 })
                            .style(primary_action_btn)
                            .on_press(Message::ApplyUpdateAndRestart),
                    ]
                    .align_y(Alignment::Center)
                    .padding(iced::Padding { top: 6.0, bottom: 6.0, left: 12.0, right: 10.0 })
                )
                .style(toast_container_style)
                .width(Length::Fill);

                container(toast)
                    .padding(iced::Padding { top: 4.0, bottom: 4.0, left: 12.0, right: 12.0 })
                    .width(Length::Fill)
                    .into()
            }
            updater::UpdateState::Error(err) => {
                let toast = container(
                    row![
                        icons::lucide_icon(icons::INFO, 16, FAILURE_RED),
                        Space::with_width(10),
                        column![
                            text("Update Check Failed").size(12).color(Color::WHITE),
                            text(err).size(11).color(FAILURE_RED),
                        ]
                        .spacing(2)
                        .width(Length::Fill),
                        button(text("Dismiss").size(11).color(Color::from_rgb(0.85, 0.85, 0.85)))
                            .padding(iced::Padding { top: 4.0, bottom: 4.0, left: 8.0, right: 8.0 })
                            .style(toast_cancel_btn)
                            .on_press(Message::DismissUpdateToast),
                    ]
                    .align_y(Alignment::Center)
                    .padding(iced::Padding { top: 6.0, bottom: 6.0, left: 12.0, right: 10.0 })
                )
                .style(toast_container_style)
                .width(Length::Fill);

                container(toast)
                    .padding(iced::Padding { top: 4.0, bottom: 4.0, left: 12.0, right: 12.0 })
                    .width(Length::Fill)
                    .into()
            }
            _ => Space::with_width(0).into(),
        }
    }

    fn view_add_profile_modal(&self) -> Element<'_, Message> {
        let is_dark = self.is_dark();
        let muted = settings_ui::secondary(is_dark);
        let accent = settings_ui::accent(is_dark);
        let mut list = column![].spacing(4);
        for proto in protocols::ProfileType::ALL {
            let badge = container(text(proto.badge().text()).size(11).color(accent))
                .padding([4, 8])
                .style(move |_| container::Style {
                    background: Some(
                        if is_dark {
                            Color::from_rgb8(44, 66, 98)
                        } else {
                            Color::from_rgb8(210, 227, 253)
                        }
                        .into(),
                    ),
                    border: iced::Border {
                        radius: 8.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                });
            list = list.push(
                button(
                    row![
                        text(proto.title()).size(14).color(self.text_primary()),
                        Space::with_width(Length::Fill),
                        badge,
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                )
                .padding([10, 12])
                .width(Length::Fill)
                .style(rounded_icon_btn_style(is_dark, true))
                .on_press(Message::CreateProfileWithType(proto)),
            );
        }
        let header = row![
            column![
                text("Add profile").size(20).color(self.text_primary()),
                text("Choose a connection protocol.").size(13).color(muted)
            ]
            .spacing(6)
            .width(Length::Fill),
            button(icons::lucide_icon(icons::CLOSE, 18, self.text_primary()))
                .padding(8)
                .style(rounded_icon_btn_style(is_dark, true))
                .on_press(Message::CloseAddMenu),
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        let dialog = container(column![header, scrollable(list).height(Length::Fill)].spacing(16))
            .padding(20)
            .width(Length::Fill)
            .max_width(360)
            .height(Length::Fill)
            .max_height(480)
            .style(settings_ui::menu_surface);
        iced::widget::mouse_area(
            container(dialog)
                .padding(16)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.65).into()),
                    ..Default::default()
                }),
        )
        .on_press(Message::CloseAddMenu)
        .into()
    }


    fn view_qr_scan_modal(&self) -> Element<'_, Message> {
        let is_dark = self.is_dark();
        let modal_bg = if is_dark {
            Color::from_rgb(0.12, 0.14, 0.18)
        } else {
            Color::from_rgb(0.96, 0.97, 0.99)
        };
        let card_border = if is_dark {
            Color::from_rgb(0.22, 0.25, 0.32)
        } else {
            Color::from_rgb(0.85, 0.88, 0.92)
        };
        let text_main = if is_dark { Color::WHITE } else { Color::from_rgb(0.1, 0.1, 0.1) };

        // Header
        let header = row![
            icons::lucide_icon(icons::QR_CODE, 20, ACCENT_BLUE_LIGHT),
            column![
                text("QR Code Scanner").size(15).color(text_main),
                text("Scan or upload a V2Ray / Xray configuration QR").size(11).color(TEXT_MUTED),
            ]
            .spacing(2)
            .width(Length::Fill),
            button(icons::lucide_icon(icons::CLOSE, 16, TEXT_MUTED))
                .padding([6, 8])
                .style(|_, status| button::Style {
                    background: match status {
                        button::Status::Hovered => Some(Color::from_rgba(1.0, 1.0, 1.0, 0.08).into()),
                        _ => None,
                    },
                    border: iced::Border {
                        radius: 8.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .on_press(Message::CloseQrScanModal),
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        // Helper to construct scan option button
        let option_button = |icon: &'static [u8], icon_color: Color, title: &'static str, desc: &'static str, source: QrScanSource| {
            let is_scanning_this = matches!(self.qr_scan_modal.status, QrScanStatus::Scanning(s) if s == source);
            let btn_content = row![
                container(icons::lucide_icon(icon, 20, icon_color))
                    .padding(10)
                    .style(move |_| container::Style {
                        background: Some(Color::from_rgba(icon_color.r, icon_color.g, icon_color.b, 0.15).into()),
                        border: iced::Border {
                            radius: 10.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                column![
                    text(title).size(13).color(text_main),
                    text(desc).size(11).color(TEXT_MUTED),
                ]
                .spacing(2)
                .width(Length::Fill),
                if is_scanning_this {
                    text("Scanning...").size(11).color(ACCENT_BLUE_LIGHT)
                } else {
                    text("Scan").size(11).color(TEXT_MUTED)
                }
            ]
            .spacing(12)
            .align_y(Alignment::Center);

            button(btn_content)
                .width(Length::Fill)
                .padding(10)
                .style(move |_, status| button::Style {
                    background: match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(Color::from_rgba(0.24, 0.58, 1.0, 0.12).into())
                        }
                        _ => Some(if is_dark {
                            Color::from_rgb(0.15, 0.17, 0.22).into()
                        } else {
                            Color::from_rgb(0.92, 0.94, 0.98).into()
                        }),
                    },
                    border: iced::Border {
                        radius: 12.0.into(),
                        width: 1.0,
                        color: if is_scanning_this {
                            ACCENT_BLUE_LIGHT
                        } else {
                            card_border
                        },
                    },
                    ..Default::default()
                })
                .on_press(Message::TriggerQrScan(source))
        };

        let opt_screenshot = option_button(
            icons::MONITOR,
            ACCENT_BLUE_LIGHT,
            "Desktop Screenshot",
            "Scan full screen or active window for QR codes",
            QrScanSource::DesktopScreenshot,
        );

        let opt_webcam = option_button(
            icons::CAMERA,
            CORAL_ACCENT,
            "Camera / PC Webcam",
            "Capture single frame from connected webcam",
            QrScanSource::Webcam,
        );

        let opt_photo = option_button(
            icons::IMAGE,
            SUCCESS_GREEN,
            "Upload Image File",
            "Select PNG, JPG, or JPEG image from your PC",
            QrScanSource::PhotoUpload,
        );

        // Status banner
        let status_box: Element<'_, Message> = match &self.qr_scan_modal.status {
            QrScanStatus::Idle => {
                container(
                    row![
                        icons::lucide_icon(icons::INFO, 14, TEXT_MUTED),
                        text("Select any method above to detect & import QR node.")
                            .size(11)
                            .color(TEXT_MUTED),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .padding([8, 12])
                .into()
            }
            QrScanStatus::Scanning(source) => {
                let label = match source {
                    QrScanSource::DesktopScreenshot => "Capturing desktop and analyzing display...",
                    QrScanSource::Webcam => "Reading camera frame and scanning for QR...",
                    QrScanSource::PhotoUpload => "Processing selected image...",
                };
                container(
                    row![
                        icons::lucide_icon(icons::ACTIVITY, 14, ACCENT_BLUE_LIGHT),
                        text(label).size(12).color(ACCENT_BLUE_LIGHT),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .padding([10, 14])
                .style(|_| container::Style {
                    background: Some(Color::from_rgba(0.24, 0.58, 1.0, 0.12).into()),
                    border: iced::Border {
                        radius: 10.0.into(),
                        width: 1.0,
                        color: ACCENT_BLUE_LIGHT,
                    },
                    ..Default::default()
                })
                .into()
            }
            QrScanStatus::Success(summary) => {
                container(
                    row![
                        icons::lucide_icon(icons::CHECK, 16, SUCCESS_GREEN),
                        text(summary).size(12).color(SUCCESS_GREEN),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .padding([10, 14])
                .style(|_| container::Style {
                    background: Some(Color::from_rgba(0.22, 0.78, 0.49, 0.12).into()),
                    border: iced::Border {
                        radius: 10.0.into(),
                        width: 1.0,
                        color: SUCCESS_GREEN,
                    },
                    ..Default::default()
                })
                .into()
            }
            QrScanStatus::Error(err) => {
                container(
                    row![
                        icons::lucide_icon(icons::INFO, 16, FAILURE_RED),
                        text(err).size(11).color(FAILURE_RED),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .padding([10, 14])
                .style(|_| container::Style {
                    background: Some(Color::from_rgba(0.95, 0.35, 0.40, 0.12).into()),
                    border: iced::Border {
                        radius: 10.0.into(),
                        width: 1.0,
                        color: FAILURE_RED,
                    },
                    ..Default::default()
                })
                .into()
            }
        };

        let dialog = container(
            column![
                header,
                opt_screenshot,
                opt_webcam,
                opt_photo,
                status_box,
            ]
            .spacing(12)
            .padding(18),
        )
        .style(move |_| container::Style {
            background: Some(modal_bg.into()),
            border: iced::Border {
                radius: 18.0.into(),
                width: 1.0,
                color: card_border,
            },
            ..Default::default()
        })
        .width(Length::Fixed(390.0));

        container(dialog)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(|_| container::Style {
                background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.65).into()),
                ..Default::default()
            })
            .into()
    }

    fn view_profiles_screen(&self) -> Element<'_, Message> {
        let is_dark = self.is_dark();
        let config_locked = self.config_locked();
        let muted = settings_ui::secondary(is_dark);
        let accent = settings_ui::accent(is_dark);
        let inactive_color = theme::icon_color_for_state(is_dark, false);
        let top_icon_btn = |icon, size: u16, label: &'static str, on_press: Option<Message>| {
            let active = on_press.is_some();
            iced::widget::tooltip(
                button(container(icons::lucide_icon(icon, size, if active { self.text_primary() } else { inactive_color }))
                    .center_x(Length::Fill).center_y(Length::Fill))
                    .width(36).height(36).padding(0)
                    .style(rounded_icon_btn_style(is_dark, active))
                    .on_press_maybe(on_press),
                text(label).size(12),
                iced::widget::tooltip::Position::Bottom,
            )
        };
        let top_bar = row![
            top_icon_btn(icons::MENU, 20, "Settings", Some(Message::SetViewMode(ViewMode::SshSettings))),
            text("Netray").size(22).color(self.text_primary()),
            Space::with_width(Length::Fill),
            top_icon_btn(icons::PLUS, 16, "Add profile", (!config_locked).then_some(Message::OpenAddMenu)),
            top_icon_btn(icons::SCAN_LINE, 16, "Scan QR code", (!config_locked).then_some(Message::OpenQrScanModal)),
            top_icon_btn(icons::CLIPBOARD_PASTE, 16, "Import from clipboard", (!config_locked).then_some(Message::RequestClipboardImport)),
            top_icon_btn(icons::FILE_TEXT, 16, "Import file", (!config_locked).then_some(Message::RequestFileImport)),
            top_icon_btn(icons::MORE_VERTICAL, 20, "More options", Some(Message::ToggleOverflowMenu)),
        ].spacing(4).align_y(Alignment::Center).padding([12, 16]);

        let overflow_menu = container(column![
            button(text("Logs").size(13).color(self.text_primary())).padding([10, 12]).style(rounded_icon_btn_style(is_dark, true)).width(Length::Fill).on_press(Message::SetViewMode(ViewMode::Logs)),
            button(text("Hide to tray").size(13).color(self.text_primary())).padding([10, 12]).style(rounded_icon_btn_style(is_dark, true)).width(Length::Fill).on_press(Message::MinimizeToTray),
            text("Sort profiles by").size(12).color(muted),
            pick_list(ProfileSort::ALL, Some(self.profile_sort), Message::ProfileSortChanged)
                .padding(8).text_size(13).width(Length::Fill)
                .style(move |_, status| settings_ui::select(is_dark, status))
                .menu_style(move |_| theme::pick_list_menu_style(is_dark)),
        ].spacing(8)).padding(16).style(settings_ui::menu_surface).width(210);

        let is_reconnecting = self.error_notification.as_ref()
            .map_or(false, |n| n.countdown_secs.is_some() || n.is_reconnecting);
        let route_option = |label: &'static str, selected: bool, message: Message| {
            let indicator: Element<'_, Message> = if selected {
                icons::lucide_icon(icons::CHECK, 14, accent).into()
            } else {
                Space::with_width(14).into()
            };
            button(row![
                indicator,
                text(label).size(13).color(if selected { accent } else { muted }),
            ].spacing(6).align_y(Alignment::Center))
            .padding([8, 12])
            .style(move |_, status| {
                let mut style = button::Style::default();
                style.border.radius = 999.0.into();
                if selected || matches!(status, button::Status::Hovered) {
                    style.background = Some(if is_dark { Color::from_rgb8(44, 66, 98) } else { Color::from_rgb8(210, 227, 253) }.into());
                }
                style
            })
            .on_press_maybe((!config_locked).then_some(message))
        };
        let route_bar = row![
            text("Routing").size(12).color(muted),
            Space::with_width(Length::Fill),
            route_option("TUN", self.enable_tun, Message::ToggleTunMode(!self.enable_tun)),
            route_option("Proxy", self.enable_sysproxy, Message::ToggleSysProxy(!self.enable_sysproxy)),
        ].spacing(8).align_y(Alignment::Center).padding([4, 16]);

        let mut ordered_profiles: Vec<_> = self.v2ray_profiles.iter().enumerate().collect();
        ordered_profiles.sort_by(|(_, a), (_, b)| match self.profile_sort {
            ProfileSort::Name => a.name.cmp(&b.name),
            ProfileSort::CreatedDate => b.created_at.cmp(&a.created_at),
            ProfileSort::Type => self.profile_display_cache.get(&a.id).map(|(_, tag)| tag)
                .cmp(&self.profile_display_cache.get(&b.id).map(|(_, tag)| tag)),
        });
        let cards: Vec<Element<'_, Message>> = ordered_profiles
            .into_iter()
            .map(|(idx, p)| {
                let is_active = self.active_profile_id.as_deref() == Some(&p.id)
                    || self.v2ray_uri_or_json == p.config;
                let (node_addr, proto_tag) = self
                    .profile_display_cache
                    .get(&p.id)
                    .map(|(a, t)| (a.as_str(), t.as_str()))
                    .unwrap_or(("127.0.0.1:443", "(V2RAY)"));

                let pinging = self.ping_in_progress.as_deref() == Some(&p.id);
                let ping_press = (self.ping_in_progress.is_none()
                    && (!self.worker_active
                        || (self.is_running
                            && is_active
                            && self.tunnel_mode == TunnelMode::V2RayXray)))
                    .then_some(Message::PingProfileIndex(idx));
                let ping_active = ping_press.is_some() || pinging;
                let ping_result = if pinging {
                    "Testing…"
                } else {
                    self.ping_results
                        .get(&p.id)
                        .map(String::as_str)
                        .unwrap_or_default()
                };
                let ping_color = if ping_result == "Failed" {
                    FAILURE_RED
                } else {
                    SUCCESS_GREEN
                };

                let card = container(
                    row![
                        icons::lucide_icon(icons::SERVER, 20, if is_active { accent } else { muted }),
                        Space::with_width(12),
                        column![
                            button(
                                column![
                                    row![
                                        text(p.name.as_str()).size(16).color(self.text_primary()),
                                        Space::with_width(Length::Fill),
                                        text(if is_active { "Selected" } else { "" })
                                            .size(11)
                                            .color(accent),
                                    ],
                                    text(node_addr).size(12).color(muted),
                                ]
                                .spacing(4)
                                .width(Length::Fill)
                            )
                            .padding(0)
                            .width(Length::Fill)
                            .style(transparent_profile_btn)
                            .on_press_maybe(
                                (!config_locked).then_some(Message::SelectProfileIndex(idx))
                            ),
                            text(proto_tag).size(12).color(accent),
                            row![
                                text(ping_result).size(12).color(ping_color),
                                Space::with_width(Length::Fill),
                                iced::widget::opaque(button(icons::lucide_icon(icons::ACTIVITY, 18, if ping_active { Color::WHITE } else { inactive_color }))
                                    .padding(10)
                                    .style(ping_btn_style(is_dark, ping_active, pinging))
                                    .on_press_maybe(ping_press)),
                                iced::widget::opaque(button(icons::lucide_icon(icons::SHARE, 16, muted))
                                    .padding(6)
                                    .style(transparent_muted_btn)
                                    .on_press(Message::ShareProfileIndex(idx))),
                                iced::widget::opaque(button(icons::lucide_icon(icons::PENCIL, 16, if config_locked { inactive_color } else { muted }))
                                    .padding(6)
                                    .style(transparent_muted_btn)
                                    .on_press_maybe(
                                        (!config_locked).then_some(Message::EditProfileIndex(idx))
                                    )),
                                iced::widget::opaque(button(icons::lucide_icon(icons::TRASH, 16, if config_locked { inactive_color } else { muted }))
                                    .padding(6)
                                    .style(transparent_muted_btn)
                                    .on_press_maybe(
                                        (!config_locked).then_some(Message::DeleteProfileIndex(idx))
                                    )),
                            ]
                            .spacing(4)
                            .align_y(Alignment::Center),
                        ]
                        .spacing(4)
                        .width(Length::Fill),
                    ]
                    .align_y(Alignment::Start)
                    .padding(12),
                )
                .width(Length::Fill)
                .style(move |theme| {
                    let mut style = settings_ui::surface(theme);
                    if is_active {
                        style.background = Some(if is_dark { Color::from_rgb8(37, 50, 69) } else { Color::from_rgb8(223, 233, 249) }.into());
                    }
                    style
                });
                if config_locked {
                    card.into()
                } else {
                    iced::widget::mouse_area(card)
                        .on_press(Message::SelectProfileIndex(idx))
                        .interaction(iced::mouse::Interaction::Pointer)
                        .into()
                }
            })
            .collect();

        let list_content: Element<'_, Message> = if cards.is_empty() {
            column![
                Space::with_height(40),
                text("Add your first profile").size(18).color(muted),
                Space::with_height(10),
                text("Import a configuration or add a server to get started.").size(13).color(muted),
                button("Import from clipboard").padding([10, 16]).style(primary_action_btn)
                    .on_press_maybe((!config_locked).then(|| Message::RequestClipboardImport)),
            ]
            .align_x(Alignment::Center)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            scrollable(column(cards).spacing(12).padding(iced::Padding {
                top: 0.0,
                bottom: 16.0,
                left: 16.0,
                right: 16.0,
            }))
            .height(Length::Fill)
            .into()
        };

        let fab_icon = if self.is_running || self.worker_active {
            icons::lucide_icon(icons::SQUARE, 26, Color::WHITE)
        } else {
            icons::lucide_icon(icons::PLAY, 26, Color::WHITE)
        };

        let (dot_color, status_text_color) = if self.is_running {
            (SUCCESS_GREEN, TEXT_MUTED)
        } else if is_reconnecting || self.worker_active {
            (WARNING_AMBER, WARNING_AMBER)
        } else {
            (TEXT_MUTED, TEXT_MUTED)
        };

        let fab_row = row![
            button(
                row![
                    text("●").color(dot_color),
                    text(self.status_badge_text.as_str())
                        .size(12)
                        .color(status_text_color),
                ]
                .spacing(6)
                .align_y(Alignment::Center)
            )
            .padding(8)
            .style(transparent_text_btn)
            .on_press(Message::SetViewMode(ViewMode::Logs)),
            Space::with_width(Length::Fill),
            button(fab_icon)
                .padding(16)
                .style(fab_btn_style)
                .on_press(Message::ToggleTunnel),
        ]
        .padding(iced::Padding {
            top: 6.0,
            bottom: 12.0,
            left: 16.0,
            right: 16.0,
        });

        let profile_column = column![top_bar]
            .push(route_bar)
            .push(list_content)
            .push(fab_row)
            .spacing(8)
            .width(Length::Fill)
            .height(Length::Fill);
        if self.show_overflow_menu {
            iced::widget::stack![
                profile_column,
                container(
                    row![Space::with_width(Length::Fill), overflow_menu]
                        .width(Length::Fill)
                        .padding(iced::Padding {
                            top: 60.0,
                            right: 16.0,
                            ..iced::Padding::ZERO
                        })
                )
                .width(Length::Fill)
                .height(Length::Fill),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            profile_column.into()
        }
    }

    fn view_edit_screen(&self) -> Element<'_, Message> {
        let is_dark = self.is_dark();
        let muted = settings_ui::secondary(is_dark);
        let accent = settings_ui::accent(is_dark);

        let proto_name = match self.node_edit_state.protocol.to_lowercase().as_str() {
            "vless" => "VLess",
            "vmess" => "VMess",
            "trojan" => "Trojan",
            "socks" => "Socks",
            "shadowsocks" | "ss" => "Shadowsocks",
            "shadowsocksr" | "ssr" => "ShadowsocksR",
            "hysteria2" | "hy2" => "Hysteria2",
            "wireguard" => "WireGuard",
            "raw" => "RAW Config",
            _ => "VLess",
        };
        let screen_title = if self.node_edit_state.is_new {
            format!("Setup {}", proto_name)
        } else {
            format!("Edit {}", proto_name)
        };

        let top_bar = row![
            button(
                container(icons::lucide_icon(icons::CLOSE, 20, self.text_primary()))
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            )
            .width(36)
            .height(36)
            .padding(0)
            .style(rounded_icon_btn_style(is_dark, true))
            .on_press(Message::CancelEditNode),
            Space::with_width(14),
            text(screen_title).size(22).color(self.text_primary()),
            Space::with_width(Length::Fill),
            button(
                row![
                    icons::lucide_icon(icons::CHECK, 18, Color::WHITE),
                    text("Save").size(14)
                ]
                .spacing(6)
                .align_y(Alignment::Center)
            )
            .padding([10, 16])
            .style(primary_action_btn)
            .on_press(Message::SaveEditedNode),
        ]
        .align_y(Alignment::Center)
        .padding(iced::Padding {
            top: 10.0,
            bottom: 10.0,
            left: 12.0,
            right: 12.0,
        });

        let remarks_card = container(
            column![
                text("Profile name").size(13).color(muted),
                modern_text_input("Remarks", &self.node_edit_state.remarks, is_dark)
                    .padding([10, 12])
                    .size(14)
                    .style(move |_, status| settings_ui::input(is_dark, status))
                    .on_input(Message::EditRemarksChanged),
            ]
            .spacing(8),
        )
        .padding(0)
        .width(Length::Fill);

        let addr_port_card = row![
            container(
                column![
                    text("Address").size(13).color(muted),
                    modern_text_input("example.com", &self.node_edit_state.address, is_dark)
                        .padding([10, 12])
                        .size(14)
                        .style(move |_, status| settings_ui::input(is_dark, status))
                        .on_input(Message::EditAddressChanged),
                ]
                .spacing(8)
            )
            .padding(0)
            .width(Length::FillPortion(3)),
            Space::with_width(10),
            container(
                column![
                    text("Port").size(13).color(muted),
                    modern_text_input("443", &self.node_edit_state.port, is_dark)
                        .padding([10, 12])
                        .size(14)
                        .style(move |_, status| settings_ui::input(is_dark, status))
                        .on_input(Message::EditPortChanged),
                ]
                .spacing(8)
            )
            .padding(0)
            .width(Length::FillPortion(1)),
        ];

        let encrypt_card = container(
            column![
                text("Encryption").size(13).color(muted),
                pick_list(
                    ["none", "zero", "aes-128-gcm", "chacha20-poly1305"],
                    Some(self.node_edit_state.encrypt_method.as_str()),
                    |s| Message::EditEncryptMethodChanged(s.to_string()),
                )
                .padding([10, 12])
                .text_size(14)
                .width(Length::Fill)
                .style(move |_, status| settings_ui::select(is_dark, status))
                .menu_style(move |_| theme::pick_list_menu_style(is_dark)),
            ]
            .spacing(8),
        )
        .padding(0)
        .width(Length::Fill);

        let flow_card = container(
            column![
                text("Flow").size(13).color(muted),
                pick_list(
                    ["none", "xtls-rprx-vision", "xtls-rprx-vision-udp443"],
                    Some(self.node_edit_state.flow.as_str()),
                    |s| Message::EditFlowChanged(s.to_string()),
                )
                .padding([10, 12])
                .text_size(14)
                .width(Length::Fill)
                .style(move |_, status| settings_ui::select(is_dark, status))
                .menu_style(move |_| theme::pick_list_menu_style(is_dark)),
            ]
            .spacing(8),
        )
        .padding(0)
        .width(Length::Fill);

        let uuid_card = container(
            column![
                text("User ID").size(13).color(muted),
                modern_text_input("User ID (UUID)", &self.node_edit_state.user_id, is_dark)
                    .padding([10, 12])
                    .size(14)
                    .style(move |_, status| settings_ui::input(is_dark, status))
                    .on_input(Message::EditUserIdChanged),
            ]
            .spacing(8),
        )
        .padding(0)
        .width(Length::Fill);

        let transport_card = container(
            column![
                text("Transport").size(13).color(muted),
                pick_list(
                    ["tcp", "ws", "grpc", "h2", "kcp"],
                    Some(self.node_edit_state.transport_protocol.as_str()),
                    |s| Message::EditTransportChanged(s.to_string()),
                )
                .padding([10, 12])
                .text_size(14)
                .width(Length::Fill)
                .style(move |_, status| settings_ui::select(is_dark, status))
                .menu_style(move |_| theme::pick_list_menu_style(is_dark)),
            ]
            .spacing(8),
        )
        .padding(0)
        .width(Length::Fill);

        let header_card = container(
            column![
                text("Header type").size(13).color(muted),
                pick_list(
                    ["none", "http"],
                    Some(self.node_edit_state.header_type.as_str()),
                    |s| Message::EditHeaderTypeChanged(s.to_string()),
                )
                .padding([10, 12])
                .text_size(14)
                .width(Length::Fill)
                .style(move |_, status| settings_ui::select(is_dark, status))
                .menu_style(move |_| theme::pick_list_menu_style(is_dark)),
            ]
            .spacing(8),
        )
        .padding(0)
        .width(Length::Fill);

        let tls_card = container(
            column![
                text("TLS type").size(13).color(muted),
                pick_list(
                    ["tls", "reality", "none"],
                    Some(self.node_edit_state.tls_type.as_str()),
                    |s| Message::EditTlsTypeChanged(s.to_string()),
                )
                .padding([10, 12])
                .text_size(14)
                .width(Length::Fill)
                .style(move |_, status| settings_ui::select(is_dark, status))
                .menu_style(move |_| theme::pick_list_menu_style(is_dark)),
            ]
            .spacing(8),
        )
        .padding(0)
        .width(Length::Fill);

        let sni_card = container(
            column![
                text("Server name indication").size(13).color(muted),
                modern_text_input("SNI (e.g. example.com)", &self.node_edit_state.sni, is_dark)
                    .padding([10, 12])
                    .size(14)
                    .style(move |_, status| settings_ui::input(is_dark, status))
                    .on_input(Message::EditSniChanged),
            ]
            .spacing(8),
        )
        .padding(0)
        .width(Length::Fill);

        let fp_card = container(
            column![
                text("Fingerprint").size(13).color(muted),
                pick_list(
                    ["chrome", "firefox", "safari", "ios", "android", "random"],
                    Some(self.node_edit_state.fingerprint.as_str()),
                    |s| Message::EditFingerprintChanged(s.to_string()),
                )
                .padding([10, 12])
                .text_size(14)
                .width(Length::Fill)
                .style(move |_, status| settings_ui::select(is_dark, status))
                .menu_style(move |_| theme::pick_list_menu_style(is_dark)),
            ]
            .spacing(8),
        )
        .padding(0)
        .width(Length::Fill);

        let alpn_card = container(
            column![
                text("ALPN").size(13).color(muted),
                pick_list(
                    ["none", "h2", "http/1.1", "h2,http/1.1"],
                    Some(self.node_edit_state.alpn.as_str()),
                    |s| Message::EditAlpnChanged(s.to_string()),
                )
                .padding([10, 12])
                .text_size(14)
                .width(Length::Fill)
                .style(move |_, status| settings_ui::select(is_dark, status))
                .menu_style(move |_| theme::pick_list_menu_style(is_dark)),
            ]
            .spacing(8),
        )
        .padding(0)
        .width(Length::Fill);

        let path_card = container(
            column![
                text("Path / Service name").size(13).color(muted),
                modern_text_input("/path (e.g. /ws)", &self.node_edit_state.path, is_dark)
                    .padding([10, 12])
                    .size(14)
                    .style(move |_, status| settings_ui::input(is_dark, status))
                    .on_input(Message::EditPathChanged),
            ]
            .spacing(8),
        )
        .padding(0)
        .width(Length::Fill);

        let server_section = container(
            column![
                text("Server").size(16).color(accent),
                remarks_card,
                addr_port_card,
                encrypt_card,
                flow_card,
                uuid_card,
            ]
            .spacing(16),
        )
        .padding(20)
        .width(Length::Fill)
        .style(settings_ui::surface);
        let transport_section = container(
            column![
                text("Transport").size(16).color(accent),
                transport_card,
                header_card,
                path_card,
            ]
            .spacing(16),
        )
        .padding(20)
        .width(Length::Fill)
        .style(settings_ui::surface);
        let mut security_fields =
            column![text("TLS & security").size(16).color(accent), tls_card,].spacing(16);

        if self.node_edit_state.tls_type == "reality" {
            let reality_row = row![
                container(
                    column![
                        text("Reality Public Key").size(13).color(muted),
                        modern_text_input("PublicKey", &self.node_edit_state.public_key, is_dark)
                            .padding([10, 12])
                            .size(14)
                            .style(move |_, status| settings_ui::input(is_dark, status))
                            .on_input(Message::EditPublicKeyChanged),
                    ]
                    .spacing(8)
                )
                .padding(0)
                .width(Length::FillPortion(2)),
                Space::with_width(10),
                container(
                    column![
                        text("Short ID").size(13).color(muted),
                        modern_text_input("ShortId", &self.node_edit_state.short_id, is_dark)
                            .padding([10, 12])
                            .size(14)
                            .style(move |_, status| settings_ui::input(is_dark, status))
                            .on_input(Message::EditShortIdChanged),
                    ]
                    .spacing(8)
                )
                .padding(0)
                .width(Length::FillPortion(1)),
            ];
            security_fields = security_fields.push(reality_row);
        }

        security_fields = security_fields.push(sni_card).push(fp_card).push(alpn_card);
        let security_section = container(security_fields)
            .padding(20)
            .width(Length::Fill)
            .style(settings_ui::surface);

        let form_content = scrollable(
            column![server_section, transport_section, security_section]
                .spacing(16)
                .padding(iced::Padding {
                    top: 0.0,
                    bottom: 20.0,
                    left: 16.0,
                    right: 16.0,
                }),
        )
        .id(scrollable::Id::new("editor_scroll"))
        .height(Length::Fill);

        column![top_bar, form_content,]
            .spacing(6)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    // ========================================================================
    // SCREEN 3: REAL-TIME LOGS & DIAGNOSTICS
    // ========================================================================

    fn view_logs_screen(&self) -> Element<'_, Message> {
        let is_dark = self.is_dark();
        let muted = settings_ui::secondary(is_dark);
        let accent = settings_ui::accent(is_dark);
        let error = if is_dark {
            Color::from_rgb8(255, 163, 164)
        } else {
            Color::from_rgb8(170, 40, 52)
        };
        let top_bar = row![
            button(icons::lucide_icon(icons::BACK, 20, self.text_primary()))
                .padding(8)
                .style(rounded_icon_btn_style(is_dark, true))
                .on_press(Message::SetViewMode(ViewMode::Profiles)),
            text("Logs").size(22).color(self.text_primary()),
            Space::with_width(Length::Fill),
            button(
                row![
                    icons::lucide_icon(icons::COPY, 16, accent),
                    text("Copy").size(13).color(accent)
                ]
                .spacing(6)
                .align_y(Alignment::Center)
            )
            .padding([8, 12])
            .style(rounded_icon_btn_style(is_dark, true))
            .on_press(Message::CopyLogs),
            button(text("Clear").size(13).color(error))
                .padding([8, 12])
                .style(rounded_icon_btn_style(is_dark, true))
                .on_press(Message::ClearLogs),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .padding([10, 12]);
        let http_port = self
            .local_socks_port
            .parse::<u16>()
            .unwrap_or(10808)
            .saturating_add(1);
        let (active_conns, proxied_total, bypassed_total) = crate::tunnel::get_tunnel_metrics();
        let status_bar = container(
            column![
                row![
                    text(format!("SOCKS :{}", self.local_socks_port))
                        .size(12)
                        .color(muted),
                    text(format!("HTTP :{http_port}")).size(12).color(muted),
                    Space::with_width(Length::Fill),
                    text(if self.is_running { "Active" } else { "Stopped" })
                        .size(12)
                        .color(accent),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
                row![
                    text(format!("Active {active_conns}")).size(12),
                    text(format!("Proxied {proxied_total}")).size(12),
                    text(format!("Bypassed {bypassed_total}")).size(12),
                ]
                .spacing(20),
            ]
            .spacing(10),
        )
        .padding(14)
        .width(Length::Fill)
        .style(settings_ui::surface);

        let query = self.logs_filter.trim().to_lowercase();
        let filtered_logs: Vec<&LogEntry> = self
            .logs
            .iter()
            .filter(|entry| {
                let matches_preset = match self.logs_preset {
                    LogsFilterPreset::All => true,
                    LogsFilterPreset::Tunnel => entry.route.as_deref().is_some_and(|r| {
                        !r.eq_ignore_ascii_case("bypass") && !r.eq_ignore_ascii_case("direct")
                    }),
                    LogsFilterPreset::Bypass => entry.route.as_deref().is_some_and(|r| {
                        r.eq_ignore_ascii_case("bypass") || r.eq_ignore_ascii_case("direct")
                    }),
                    LogsFilterPreset::Apps => entry.app_name.is_some(),
                    LogsFilterPreset::Errors => {
                        matches!(entry.level, LogLevel::Error | LogLevel::Warn)
                    }
                };
                if !matches_preset {
                    return false;
                }

                if query.is_empty() {
                    return true;
                }
                entry.message.to_lowercase().contains(&query)
                    || entry.source.to_lowercase().contains(&query)
                    || entry
                        .app_name
                        .as_deref()
                        .is_some_and(|a| a.to_lowercase().contains(&query))
                    || entry
                        .target_host
                        .as_deref()
                        .is_some_and(|h| h.to_lowercase().contains(&query))
                    || entry
                        .route
                        .as_deref()
                        .is_some_and(|r| r.to_lowercase().contains(&query))
            })
            .collect();

        let log_rows: Vec<Element<Message>> = filtered_logs
            .iter()
            .rev()
            .take(80)
            .rev()
            .map(|&entry| {
                let color = match entry.level {
                    LogLevel::Info => self.text_primary(),
                    LogLevel::Success => {
                        if is_dark {
                            Color::from_rgb8(145, 221, 176)
                        } else {
                            Color::from_rgb8(28, 111, 64)
                        }
                    }
                    LogLevel::Warn => {
                        if is_dark {
                            Color::from_rgb8(242, 194, 104)
                        } else {
                            Color::from_rgb8(135, 73, 0)
                        }
                    }
                    LogLevel::Error => error,
                };
                let mut details = Vec::new();
                if let Some(app) = &entry.app_name {
                    details.push(format!("App: {app}"));
                }
                if let (Some(host), Some(port)) = (&entry.target_host, entry.target_port) {
                    details.push(format!("{host}:{port}"));
                }
                if let Some(route) = &entry.route {
                    details.push(route.clone());
                }
                let mut content = column![
                    row![
                        text(&entry.timestamp).size(12).color(muted),
                        text(&entry.source).size(12).color(accent)
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                    text(&entry.message)
                        .size(13)
                        .color(color)
                        .width(Length::Fill),
                ]
                .spacing(4)
                .width(Length::Fill);
                if !details.is_empty() {
                    content = content.push(
                        text(details.join(" · "))
                            .size(12)
                            .color(muted)
                            .width(Length::Fill),
                    );
                }
                container(content)
                    .padding([8, 0])
                    .width(Length::Fill)
                    .into()
            })
            .collect();
        let log_body: Element<'_, Message> = if filtered_logs.is_empty() {
            container(
                column![
                    text(if self.logs.is_empty() {
                        "No logs yet"
                    } else {
                        "No matching logs"
                    })
                    .size(16)
                    .color(self.text_primary()),
                    text(if self.logs.is_empty() {
                        "Connection events will appear here."
                    } else {
                        "Try another filter or search."
                    })
                    .size(13)
                    .color(muted),
                ]
                .spacing(8)
                .align_x(Alignment::Center),
            )
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
        } else {
            scrollable(column(log_rows).spacing(4))
                .id(scrollable::Id::new("logs"))
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        };
        let terminal = container(log_body)
            .padding(16)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(settings_ui::surface);
        let preset = |value: LogsFilterPreset, label: &'static str| -> Element<'_, Message> {
            let selected = self.logs_preset == value;
            button(
                text(label)
                    .size(12)
                    .color(if selected { accent } else { muted })
                    .align_x(Alignment::Center),
            )
            .width(Length::Fill)
            .padding([8, 6])
            .style(move |_, status| button::Style {
                background: Some(
                    if selected {
                        if is_dark {
                            Color::from_rgb8(44, 66, 98)
                        } else {
                            Color::from_rgb8(210, 227, 253)
                        }
                    } else if matches!(status, button::Status::Hovered) {
                        if is_dark {
                            Color::from_rgb8(35, 42, 54)
                        } else {
                            Color::from_rgb8(233, 238, 246)
                        }
                    } else {
                        Color::TRANSPARENT
                    }
                    .into(),
                ),
                border: iced::Border {
                    radius: 999.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .on_press(Message::LogsPresetChanged(value))
            .into()
        };
        let presets = row![
            preset(LogsFilterPreset::All, "All"),
            preset(LogsFilterPreset::Tunnel, "Tunnel"),
            preset(LogsFilterPreset::Bypass, "Bypass"),
            preset(LogsFilterPreset::Apps, "Apps"),
            preset(LogsFilterPreset::Errors, "Errors"),
        ]
        .spacing(4);
        let mut search =
            row![
                modern_text_input("Search app, host, or message", &self.logs_filter, is_dark)
                    .padding([10, 12])
                    .size(14)
                    .style(move |_, status| settings_ui::input(is_dark, status))
                    .on_input(Message::LogsFilterChanged)
                    .width(Length::Fill)
            ]
            .spacing(8)
            .align_y(Alignment::Center);
        if !self.logs_filter.is_empty() {
            search = search.push(
                button(text("Clear").size(12).color(muted))
                    .padding(8)
                    .style(rounded_icon_btn_style(is_dark, true))
                    .on_press(Message::LogsFilterChanged(String::new())),
            );
        }
        let controls = column![
            presets,
            search,
            row![
                toggler(self.auto_scroll_logs)
                    .label("Auto-scroll")
                    .size(18)
                    .text_size(13)
                    .on_toggle(Message::AutoScrollLogs),
                Space::with_width(Length::Fill),
                text(format!(
                    "{} / {} entries",
                    filtered_logs.len(),
                    self.logs.len()
                ))
                .size(12)
                .color(muted),
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(10);
        column![
            top_bar,
            container(column![status_bar, controls, terminal].spacing(12))
                .padding(iced::Padding {
                    top: 0.0,
                    bottom: 16.0,
                    left: 16.0,
                    right: 16.0
                })
                .width(Length::Fill)
                .height(Length::Fill),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn view_ssh_screen(&self) -> Element<'_, Message> {
        let is_dark_active = self.is_dark();
        let locked = self.config_locked();
        let muted = settings_ui::secondary(is_dark_active);
        let accent = settings_ui::accent(is_dark_active);
        let primary = self.text_primary();
        let warning = if is_dark_active { WARNING_AMBER } else { Color::from_rgb8(135, 73, 0) };
        let top_bar = row![
            button(icons::lucide_icon(icons::BACK, 22, self.text_primary()))
                .padding(6)
                .style(|_, _| button::Style {
                    background: None,
                    ..Default::default()
                })
                .on_press(Message::SetViewMode(ViewMode::Profiles)),
            Space::with_width(12),
            text("Settings").size(24).color(self.text_primary()),
        ]
        .align_y(Alignment::Center)
        .padding(iced::Padding {
            top: 10.0,
            bottom: 10.0,
            left: 12.0,
            right: 12.0,
        });

        let theme_button = |label: &'static str, icon_bytes: &'static [u8], pref: storage::ThemePreference| {
            let selected = self.theme_preference == pref;
            let ink = if selected { accent } else { primary };
            button(
                column![icons::lucide_icon(icon_bytes, 20, ink), text(label).size(12).color(ink)]
                    .spacing(8).align_x(Alignment::Center).width(Length::Fill)
            )
            .width(Length::Fill)
            .padding([12, 8])
            .style(move |_, status| button::Style {
                background: Some(if selected {
                    if is_dark_active { Color::from_rgb8(44, 66, 98) } else { Color::from_rgb8(210, 227, 253) }
                } else if matches!(status, button::Status::Hovered) {
                    if is_dark_active { Color::from_rgb8(48, 56, 70) } else { Color::from_rgb8(222, 229, 240) }
                } else {
                    Color::TRANSPARENT
                }.into()),
                border: iced::Border { radius: 16.0.into(), ..Default::default() },
                ..Default::default()
            })
            .on_press(Message::ThemePreferenceChanged(pref))
        };

        let active_theme_label = match self.theme_preference {
            storage::ThemePreference::Light => "Light mode active",
            storage::ThemePreference::Dark => "Dark mode active",
            storage::ThemePreference::System => {
                if is_dark_active {
                    "System theme active (Dark detected)"
                } else {
                    "System theme active (Light detected)"
                }
            }
        };

        let theme_card = container(
            column![
                row![
                    icons::lucide_icon(icons::SUN, 16, accent),
                    Space::with_width(6),
                    text("Appearance").size(16).color(accent),
                ]
                .align_y(Alignment::Center),
                text("Choose how Netray looks on this device.")
                    .size(12)
                    .color(muted),
                Space::with_height(6),
                row![
                    theme_button("Light", icons::SUN, storage::ThemePreference::Light),
                    theme_button("Dark", icons::MOON, storage::ThemePreference::Dark),
                    theme_button("System", icons::MONITOR, storage::ThemePreference::System),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
                Space::with_height(2),
                text(active_theme_label)
                    .size(11)
                    .color(accent),
            ]
            .spacing(4)
            .width(Length::Fill)
        )
        .padding(20)
        .style(settings_ui::surface)
        .width(Length::Fill);

        let startup_card = container(column![
            text("Startup & connection").size(16).color(accent),
            settings_ui::switch_row("Run at startup", "Launch Netray when you sign in.", self.run_at_startup, true, Message::ToggleRunAtStartup, is_dark_active),
            settings_ui::switch_row("Connect at startup", "Ask to connect when Netray opens.", self.connect_at_startup, true, Message::ToggleConnectAtStartup, is_dark_active),
            settings_ui::switch_row("Automatic reconnect", "Restart the tunnel if its process exits.", self.auto_reconnect, true, Message::ToggleAutoReconnect, is_dark_active),
        ].spacing(4)).padding(20).style(settings_ui::surface).width(Length::Fill);

        let network_card = container(column![
            text("Network & security").size(16).color(accent),
            text("DNS servers").size(14),
            text("Comma-separated IP addresses. Changes apply on the next connection.").size(12).color(muted),
            modern_text_input("1.1.1.1, 8.8.8.8", &self.dns_servers, is_dark_active)
                .style(move |_, status| settings_ui::input(is_dark_active, status))
                .on_input_maybe((!locked).then_some(Message::DnsServersChanged)),
            settings_ui::switch_row("Bypass local network", "Connect directly to devices on your local network.", self.bypass_lan, !locked, Message::ToggleBypassLan, is_dark_active),
            settings_ui::switch_row("Enable IPv6", "When off, IPv6 traffic is blocked.", self.ipv6_enabled, !locked, Message::ToggleIpv6, is_dark_active),
            settings_ui::switch_row("Allow insecure TLS", "Skip server certificate verification. This permits interception.", self.allow_insecure_tls, !self.is_running && !self.worker_active, Message::ToggleAllowInsecureTls, is_dark_active),
        ].spacing(8)).padding(20).style(settings_ui::surface).width(Length::Fill);

        let activity_card = container(column![
            text("Notifications & logs").size(16).color(accent),
            settings_ui::switch_row("Connection alerts", "Notify when the tunnel drops or reconnects.", self.notify_connection_drops, true, Message::ToggleConnectionDropNotifications, is_dark_active),
            settings_ui::switch_row("Detailed logs", "When off, show only connection events. Core logging changes on reconnect.", self.logs_enabled, true, Message::ToggleLogsEnabled, is_dark_active),
        ].spacing(4)).padding(20).style(settings_ui::surface).width(Length::Fill);

        let update_status_desc = match &self.update_state {
            updater::UpdateState::Checking => "Checking GitHub CI for new releases...".to_string(),
            updater::UpdateState::Available(info) => format!("New release v{} is available!", info.version),
            updater::UpdateState::UpToDate(_) => format!("Netray is up to date (v{}).", APP_VERSION),
            updater::UpdateState::Downloading { version, status } => format!("Upgrading to v{version}: {status}"),
            updater::UpdateState::ReadyToRestart { version } => format!("Version v{version} installed! Restart required."),
            updater::UpdateState::Downloaded { info, .. } => format!("Version v{} ready to apply.", info.version),
            updater::UpdateState::Error(err) => format!("Update check error: {err}"),
            updater::UpdateState::Idle => format!("Current version: v{APP_VERSION}"),
            updater::UpdateState::Applying => "Applying update...".to_string(),
        };

        let update_action_btn: Element<'_, Message> = match &self.update_state {
            updater::UpdateState::Available(_) => {
                button(text("Upgrade Now").size(12).color(Color::WHITE))
                    .padding(iced::Padding { top: 4.0, bottom: 4.0, left: 12.0, right: 12.0 })
                    .style(primary_action_btn)
                    .on_press(Message::StartInAppUpgrade)
                    .into()
            }
            updater::UpdateState::ReadyToRestart { .. } => {
                button(text("Restart Now").size(12).color(Color::WHITE))
                    .padding(iced::Padding { top: 4.0, bottom: 4.0, left: 12.0, right: 12.0 })
                    .style(primary_action_btn)
                    .on_press(Message::ApplyUpdateAndRestart)
                    .into()
            }
            updater::UpdateState::Checking | updater::UpdateState::Downloading { .. }
            | updater::UpdateState::Downloaded { .. } | updater::UpdateState::Applying => {
                button(text("Check for Updates").size(12).color(Color::from_rgb(0.62, 0.64, 0.68)))
                    .padding(iced::Padding { top: 4.0, bottom: 4.0, left: 12.0, right: 12.0 })
                    .style(disabled_update_btn)
                    .on_press_maybe(None::<Message>)
                    .into()
            }
            _ => {
                button(text("Check for Updates").size(12).color(Color::WHITE))
                    .padding(iced::Padding { top: 4.0, bottom: 4.0, left: 12.0, right: 12.0 })
                    .style(primary_action_btn)
                    .on_press(Message::CheckForUpdates)
                    .into()
            }
        };

        let update_card = container(
            column![
                row![
                    icons::lucide_icon(icons::DOWNLOAD, 16, accent),
                    Space::with_width(6),
                    text("Updates").size(16).color(self.text_primary()),
                ]
                .align_y(Alignment::Center),
                settings_ui::switch_row("Check automatically", "Look for new releases when Netray starts.", self.auto_check_updates, true, Message::ToggleAutoCheckUpdates, is_dark_active),
                Space::with_height(6),
                row![
                    text(update_status_desc).size(12).color(self.text_primary()),
                    Space::with_width(12),
                    update_action_btn,
                ]
                .align_y(Alignment::Center),
            ]
            .spacing(6)
            .width(Length::Fill)
        )
        .padding(20)
        .style(settings_ui::surface)
        .width(Length::Fill);

        let mut chips_col = column![].spacing(6);
        for chunk in self.cached_app_chips.chunks(3) {
            let mut r = row![].spacing(6);
            for app in chunk {
                let chip = container(
                    row![
                        text(app.as_str()).size(12).color(Color::WHITE),
                        Space::with_width(4),
                        button(icons::lucide_icon(icons::CLOSE, 10, Color::from_rgb(0.85, 0.85, 0.85)))
                            .padding(2)
                            .style(chip_close_btn_style)
                            .on_press(Message::RemoveWhitelistApp(app.clone())),
                    ]
                    .align_y(Alignment::Center)
                )
                .padding(iced::Padding {
                    top: 3.0,
                    bottom: 3.0,
                    left: 8.0,
                    right: 4.0,
                })
                .style(whitelist_chip_style);
                r = r.push(chip);
            }
            chips_col = chips_col.push(r);
        }

        let app_picker_row = row![
            pick_list(
                &self.available_apps[..],
                self.selected_app_preset.clone(),
                Message::PickAppSelected,
            )
            .placeholder("Choose app to whitelist...")
            .padding(8)
            .text_size(13)
            .style(move |_, status| settings_ui::select(is_dark_active, status))
            .menu_style(move |_| theme::pick_list_menu_style(is_dark_active))
            .width(Length::FillPortion(3)),
            Space::with_width(8),
            if locked {
                button(
                    row![
                        icons::lucide_icon(icons::LOCK, 15, muted),
                        Space::with_width(4),
                        text("Browse...").size(12).color(muted),
                    ]
                    .align_y(Alignment::Center)
                )
                .padding(iced::Padding {
                    top: 6.0,
                    bottom: 6.0,
                    left: 10.0,
                    right: 10.0,
                })
                .style(toast_cancel_btn)
            } else {
                button(
                    row![
                        icons::lucide_icon(icons::FILE_PLUS, 15, Color::WHITE),
                        Space::with_width(4),
                        text("Browse...").size(12).color(Color::WHITE),
                    ]
                    .align_y(Alignment::Center)
                )
                .padding(iced::Padding {
                    top: 6.0,
                    bottom: 6.0,
                    left: 10.0,
                    right: 10.0,
                })
                .style(primary_action_btn)
                .on_press(Message::BrowseAppExecutable)
            },
        ]
        .align_y(Alignment::Center)
        .width(Length::Fill);

        let whitelist_apps_input: Element<'_, Message> = if locked {
            container(
                text(if self.whitelist_apps.is_empty() { "No application whitelist configured" } else { &self.whitelist_apps })
                    .size(13)
                    .color(muted)
            )
            .padding(iced::Padding { top: 8.0, bottom: 8.0, left: 10.0, right: 10.0 })
            .style(move |_| container::Style {
                background: Some(if is_dark_active { Color::from_rgb8(25, 31, 41) } else { Color::from_rgb8(248, 250, 254) }.into()),
                border: iced::Border {
                    color: Color::from_rgba(1.0, 1.0, 1.0, 0.1),
                    width: 0.0,
                    radius: 12.0.into(),
                },
                ..Default::default()
            })
            .width(Length::Fill)
            .into()
        } else {
            modern_text_input("e.g. curl.exe, chrome.exe, discord.exe", &self.whitelist_apps, is_dark_active)
                        .style(move |_, status| settings_ui::input(is_dark_active, status))
                .on_input(Message::WhitelistAppsChanged)
                .padding(8)
                .width(Length::Fill)
                .into()
        };

        let whitelist_urls_input: Element<'_, Message> = if locked {
            container(
                text(if self.whitelist_urls.is_empty() { "No domain whitelist configured" } else { &self.whitelist_urls })
                    .size(13)
                    .color(muted)
            )
            .padding(iced::Padding { top: 8.0, bottom: 8.0, left: 10.0, right: 10.0 })
            .style(move |_| container::Style {
                background: Some(if is_dark_active { Color::from_rgb8(25, 31, 41) } else { Color::from_rgb8(248, 250, 254) }.into()),
                border: iced::Border {
                    color: Color::from_rgba(1.0, 1.0, 1.0, 0.1),
                    width: 0.0,
                    radius: 12.0.into(),
                },
                ..Default::default()
            })
            .width(Length::Fill)
            .into()
        } else {
            modern_text_input("e.g. *.internal.net, 10.0.0.0/8, https://work.com", &self.whitelist_urls, is_dark_active)
                        .style(move |_, status| settings_ui::input(is_dark_active, status))
                .on_input(Message::WhitelistUrlsChanged)
                .padding(8)
                .width(Length::Fill)
                .into()
        };

        let whitelist_card = container(
            column![
                row![
                    text("Split tunneling").size(16).color(accent),
                    Space::with_width(Length::Fill),
                    if locked {
                        text("Locked while active").size(11).color(warning)
                    } else {
                        text("")
                    },
                ]
                .align_y(Alignment::Center),
                text("Traffic from these applications and URLs/domains will bypass the proxy tunnel and connect directly. Active in both TUN and System Proxy modes.")
                    .size(12)
                    .color(muted),
                Space::with_height(10),
                text("Whitelist Applications:").size(13).color(accent),
                app_picker_row,
                chips_col,
                whitelist_apps_input,
                text("Select from catalog, browse .exe, or enter comma/newline-separated names.")
                    .size(11)
                    .color(muted),
                Space::with_height(8),
                text("Whitelist URLs & Domains:").size(13).color(accent),
                whitelist_urls_input,
                text("Comma or newline-separated URLs, domains, wildcards (*.site.com), or IP CIDRs.")
                    .size(11)
                    .color(muted),
            ]
            .spacing(6)
            .width(Length::Fill)
        )
        .padding(20)
        .style(settings_ui::surface)
        .width(Length::Fill);

        let ip_text = if self.is_checking_ip {
            text("Checking ifconfig.me...").size(13).color(muted)
        } else if let Some(ip) = &self.public_ip {
            let color = if ip.starts_with("Error") || ip.starts_with("Failed") || ip.starts_with("curl exited") {
                FAILURE_RED
            } else {
                if is_dark_active { SUCCESS_GREEN } else { Color::from_rgb8(27, 114, 69) }
            };
            text(ip.clone()).size(14).color(color)
        } else {
            text("Click to check current public IP").size(13).color(muted)
        };

        let ip_card = container(
            row![
                column![
                    text("Public IP address").size(14).color(primary),
                    Space::with_height(2),
                    ip_text,
                ]
                .spacing(2)
                .width(Length::Fill),
                button(
                    row![
                        icons::lucide_icon(icons::ACTIVITY, 14, Color::WHITE),
                        Space::with_width(4),
                        text(if self.is_checking_ip { "Checking..." } else { "Check IP" })
                            .size(12)
                            .color(Color::WHITE),
                    ]
                    .align_y(Alignment::Center)
                )
                .padding(iced::Padding {
                    top: 6.0,
                    bottom: 6.0,
                    left: 10.0,
                    right: 10.0,
                })
                .style(primary_action_btn)
                .on_press_maybe(if self.is_checking_ip { None } else { Some(Message::CheckPublicIp) }),
            ]
            .align_y(Alignment::Center)
            .width(Length::Fill)
        )
        .padding(20)
        .style(settings_ui::surface)
        .width(Length::Fill);

        let is_ssh_mode = matches!(self.tunnel_mode, TunnelMode::SshPayload | TunnelMode::SshSslSni);

        let mode_selector: Element<'_, Message> = if locked {
            container(
                row![
                    icons::lucide_icon(icons::LOCK, 14, warning),
                    Space::with_width(8),
                    text(format!("{} (Locked while connected)", self.tunnel_mode))
                        .size(13)
                        .color(muted),
                ]
                .align_y(Alignment::Center)
            )
            .padding(iced::Padding {
                top: 8.0,
                bottom: 8.0,
                left: 10.0,
                right: 8.0,
            })
            .style(move |_| container::Style {
                background: Some(if is_dark_active { Color::from_rgb8(25, 31, 41) } else { Color::from_rgb8(248, 250, 254) }.into()),
                border: iced::Border {
                    color: Color::from_rgba(1.0, 1.0, 1.0, 0.1),
                    width: 0.0,
                    radius: 12.0.into(),
                },
                ..Default::default()
            })
            .width(Length::Fill)
            .into()
        } else {
            pick_list(
                &TUNNEL_MODES[..],
                Some(self.tunnel_mode),
                Message::TunnelModeChanged
            )
            .padding(8)
            .text_size(13)
            .style(move |_, status| settings_ui::select(is_dark_active, status))
            .menu_style(move |_| theme::pick_list_menu_style(is_dark_active))
            .width(Length::Fill)
            .into()
        };

        let ssh_card = if is_ssh_mode {
            let mut ssh_col = column![
                row![
                    icons::lucide_icon(icons::SERVER, 16, accent),
                    Space::with_width(6),
                    text("SSH Tunnel & Payload Configuration").size(14).color(self.text_primary()),
                ]
                .align_y(Alignment::Center),
                text("Configure custom HTTP payload injection, Bug Host, TLS SNI, and remote SSH credentials.")
                    .size(12)
                    .color(muted),
                Space::with_height(10),
                text("HTTP Payload Template:").size(13).color(accent),
            ];

            if locked {
                ssh_col = ssh_col.push(
                    container(text(if self.payload_text.is_empty() { "No payload template set" } else { &self.payload_text }).size(12).color(muted))
                        .padding(8)
                        .style(move |_| container::Style {
                            background: Some(if is_dark_active { Color::from_rgb8(25, 31, 41) } else { Color::from_rgb8(248, 250, 254) }.into()),
                            border: iced::Border { color: Color::from_rgba(1.0, 1.0, 1.0, 0.1), width: 0.0, radius: 12.0.into() },
                            ..Default::default()
                        })
                        .width(Length::Fill)
                );
                ssh_col = ssh_col.push(Space::with_height(8));
                ssh_col = ssh_col.push(text("Bug Host / CDN SNI:").size(13).color(accent));
                ssh_col = ssh_col.push(
                    container(text(if self.bug_host.is_empty() { "None" } else { &self.bug_host }).size(12).color(muted))
                        .padding(8)
                        .style(move |_| container::Style {
                            background: Some(if is_dark_active { Color::from_rgb8(25, 31, 41) } else { Color::from_rgb8(248, 250, 254) }.into()),
                            border: iced::Border { color: Color::from_rgba(1.0, 1.0, 1.0, 0.1), width: 0.0, radius: 12.0.into() },
                            ..Default::default()
                        })
                        .width(Length::Fill)
                );
                ssh_col = ssh_col.push(Space::with_height(8));
                ssh_col = ssh_col.push(text("SSH Server Details:").size(13).color(accent));
                ssh_col = ssh_col.push(
                    container(text(format!("{}:{} (User: {})", self.ssh_host, self.ssh_port, self.ssh_user)).size(12).color(muted))
                        .padding(8)
                        .style(move |_| container::Style {
                            background: Some(if is_dark_active { Color::from_rgb8(25, 31, 41) } else { Color::from_rgb8(248, 250, 254) }.into()),
                            border: iced::Border { color: Color::from_rgba(1.0, 1.0, 1.0, 0.1), width: 0.0, radius: 12.0.into() },
                            ..Default::default()
                        })
                        .width(Length::Fill)
                );
            } else {
                ssh_col = ssh_col.push(
                    modern_text_input("Enter HTTP payload...", &self.payload_text, is_dark_active)
                        .style(move |_, status| settings_ui::input(is_dark_active, status))
                        .on_input(Message::PayloadChanged)
                        .padding(8)
                );
                ssh_col = ssh_col.push(Space::with_height(10));
                ssh_col = ssh_col.push(
                    row![
                        checkbox("Keep-Alive", self.gen_keep_alive).on_toggle(Message::ToggleKeepAlive),
                        checkbox("Online-Host", self.gen_online_host).on_toggle(Message::ToggleOnlineHost),
                        checkbox("Split", self.gen_split).on_toggle(Message::ToggleSplit),
                    ]
                    .spacing(12)
                );
                ssh_col = ssh_col.push(Space::with_height(10));
                ssh_col = ssh_col.push(text("Bug Host / CDN SNI:").size(13).color(accent));
                ssh_col = ssh_col.push(
                    modern_text_input("cdn.example.com", &self.bug_host, is_dark_active)
                        .style(move |_, status| settings_ui::input(is_dark_active, status))
                        .on_input(Message::BugHostChanged)
                        .padding(8)
                );
                ssh_col = ssh_col.push(Space::with_height(10));
                ssh_col = ssh_col.push(button("Generate HTTP Payload").padding([8, 16]).style(primary_action_btn).on_press(Message::GeneratePayload));
                ssh_col = ssh_col.push(Space::with_height(20));
                ssh_col = ssh_col.push(text("SSH Server & Account:").size(13).color(accent));
                ssh_col = ssh_col.push(
                    row![
                        modern_text_input("SSH Host", &self.ssh_host, is_dark_active)
                        .style(move |_, status| settings_ui::input(is_dark_active, status))
                            .on_input(Message::SshHostChanged)
                            .width(Length::FillPortion(3))
                            .padding(8),
                        modern_text_input("Port", &self.ssh_port, is_dark_active)
                        .style(move |_, status| settings_ui::input(is_dark_active, status))
                            .on_input(Message::SshPortChanged)
                            .width(Length::FillPortion(1))
                            .padding(8),
                    ]
                    .spacing(8)
                );
                ssh_col = ssh_col.push(Space::with_height(10));
                ssh_col = ssh_col.push(
                    row![
                        modern_text_input("Username", &self.ssh_user, is_dark_active)
                        .style(move |_, status| settings_ui::input(is_dark_active, status))
                            .on_input(Message::SshUserChanged)
                            .width(Length::FillPortion(1))
                            .padding(8),
                        modern_text_input("Password", &self.ssh_pass, is_dark_active)
                        .style(move |_, status| settings_ui::input(is_dark_active, status))
                            .on_input(Message::SshPassChanged)
                            .width(Length::FillPortion(1))
                            .padding(8),
                    ]
                    .spacing(8)
                );
            }

            Some(
                container(ssh_col.spacing(8).width(Length::Fill))
                    .padding(12)
                    .style(settings_ui::surface)
                    .width(Length::Fill)
            )
        } else {
            None
        };

        let mut settings_col = column![];

        if locked {
            settings_col = settings_col.push(
                container(
                    row![
                        icons::lucide_icon(icons::LOCK, 16, warning),
                        Space::with_width(8),
                        column![
                            text("Tunnel settings are locked").size(13).color(warning),
                            text("Disconnect to change network, security, or routing options.")
                                .size(11)
                                .color(muted),
                        ]
                        .spacing(2)
                    ]
                    .align_y(Alignment::Center)
                )
                .padding(12)
                .style(settings_ui::surface)
                .width(Length::Fill)
            );
            settings_col = settings_col.push(Space::with_height(12));
        }

        let mut usage_dates = vec!["All dates".to_string()];
        usage_dates.extend(self.usage_records.iter().map(|record| record.date.clone()));
        let (uploaded, downloaded) = self.usage_records.iter()
            .filter(|record| self.usage_date_filter == "All dates" || record.date == self.usage_date_filter)
            .fold((0u64, 0u64), |(up, down), record| (up.saturating_add(record.uploaded), down.saturating_add(record.downloaded)));
        let usage_card = container(column![
            row![text("Data usage").size(16).color(self.text_primary()), Space::with_width(Length::Fill), pick_list(usage_dates, Some(self.usage_date_filter.clone()), Message::UsageDateChanged).width(Length::Fixed(150.0)).padding(8).text_size(13).style(move |_, status| theme::pick_list_style(self.is_dark(), status)).menu_style(move |_| theme::pick_list_menu_style(self.is_dark())).width(Length::Fixed(150.0))].align_y(Alignment::Center),
            row![text("Uploaded").size(12).color(muted), Space::with_width(Length::Fill), text(storage::format_bytes(uploaded)).size(13).color(self.text_primary())],
            row![text("Downloaded").size(12).color(muted), Space::with_width(Length::Fill), text(storage::format_bytes(downloaded)).size(13).color(self.text_primary())],
            text("Netray tunnel traffic").size(12).color(muted),
        ].spacing(8)).padding(20).style(settings_ui::surface).width(Length::Fill);

        settings_col = settings_col.push(theme_card);
        settings_col = settings_col.push(Space::with_height(12));
        settings_col = settings_col.push(startup_card);
        settings_col = settings_col.push(Space::with_height(12));
        settings_col = settings_col.push(network_card);
        settings_col = settings_col.push(Space::with_height(12));
        settings_col = settings_col.push(activity_card);
        settings_col = settings_col.push(Space::with_height(12));
        settings_col = settings_col.push(update_card);
        settings_col = settings_col.push(Space::with_height(12));
        settings_col = settings_col.push(usage_card);
        settings_col = settings_col.push(Space::with_height(12));
        settings_col = settings_col.push(ip_card);
        settings_col = settings_col.push(Space::with_height(12));
        settings_col = settings_col.push(whitelist_card);
        settings_col = settings_col.push(Space::with_height(14));
        settings_col = settings_col.push(
            row![
                text("Tunnel mode").size(14).color(muted),
                Space::with_width(Length::Fill),
                if locked {
                    text("Disconnect to switch mode").size(11).color(warning)
                } else {
                    text("")
                },
            ]
            .align_y(Alignment::Center)
        );
        settings_col = settings_col.push(mode_selector);

        if let Some(card) = ssh_card {
            settings_col = settings_col.push(Space::with_height(14));
            settings_col = settings_col.push(card);
        }

        let content = scrollable(
            settings_col
                .spacing(0)
                .width(Length::Fill)
                .padding(iced::Padding {
                    top: 0.0,
                    bottom: 20.0,
                    left: 16.0,
                    right: 16.0,
                }),
        )
        .id(scrollable::Id::new("settings_scroll"))
        .width(Length::Fill)
        .height(Length::Fill);

        column![
            top_bar,
            content,
            text(format!("Version {APP_VERSION}"))
                .size(11)
                .color(muted)
                .width(Length::Fill)
                .align_x(Alignment::Center),
        ]
        .spacing(6)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}

#[cfg(test)]
mod performance_tests {
    use super::*;

    #[test]
    fn xray_connecting_state_waits_for_the_successful_remote_ping() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        app.worker_active = true;
        let _ = app.update(Message::NewLog(LogEntry::new(
            "Xray",
            "Core process started",
            LogLevel::Info,
        )));
        assert!(!app.is_running);
        assert!(app.connected_at.is_none());
        let _ = app.update(Message::NewLog(LogEntry::new(
            "Error",
            "Connection check failed: HTTP 502",
            LogLevel::Error,
        )));
        assert!(!app.is_running);
        assert!(app.connected_at.is_none());
        let _ = app.update(Message::NewLog(LogEntry::new(
            "Supervisor",
            "Tunnel ready · HTTP 204 through proxy in 73 ms.",
            LogLevel::Success,
        )));
        assert!(app.is_running);
        assert!(app.connected_at.is_some());
        assert_eq!(app.ping_ms, Some(73));
    }

    #[test]
    fn errors_do_not_release_worker_before_cleanup() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        app.worker_active = true;
        app.is_running = true;
        let _ = app.update(Message::NewLog(LogEntry::new(
            "Error",
            "Engine failed: test",
            LogLevel::Error,
        )));
        assert!(app.worker_active);
        assert!(!app.is_running);
        let _ = app.update(Message::NewLog(LogEntry::new(
            "Supervisor",
            "Tunnel is offline.",
            LogLevel::Info,
        )));
        assert!(!app.worker_active);
    }

    #[test]
    fn disabling_auto_scroll_keeps_incoming_logs() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        let _ = app.update(Message::SetViewMode(ViewMode::Logs));
        let _ = app.update(Message::AutoScrollLogs(false));
        let count = app.logs.len();
        let _ = app.update(Message::NewLog(LogEntry::new(
            "Test",
            "still collected",
            LogLevel::Info,
        )));
        assert!(!app.auto_scroll_logs);
        assert_eq!(app.logs.len(), count + 1);
        let _ = app.update(Message::AutoScrollLogs(true));
        assert!(app.auto_scroll_logs);
    }

    #[test]
    fn log_batch_preserves_entry_order() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        app.logs.clear();
        let entries = vec![LogEntry::new("Xray", "first", LogLevel::Info), LogEntry::new("Xray", "second", LogLevel::Info)];
        let _ = app.update_message(Message::LogBatch(entries));
        assert_eq!(app.logs.len(), 2);
        assert_eq!(app.logs[0].message, "first");
        assert_eq!(app.logs[1].message, "second");
    }

    #[test]
    fn uptime_only_subscribes_when_visible_and_running() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        let subscriptions =
            |app: &NetrayApp| iced::advanced::subscription::into_recipes(app.subscription()).len();
        let visible_idle = subscriptions(&app);
        app.is_running = true;
        assert_eq!(subscriptions(&app), visible_idle + 1);
        for view in [ViewMode::Logs, ViewMode::EditNode, ViewMode::SshSettings] {
            app.view_mode = view;
            assert_eq!(subscriptions(&app), visible_idle);
        }
        app.view_mode = ViewMode::Profiles;
        assert_eq!(subscriptions(&app), visible_idle + 1);
        app.hidden_to_tray = true;
        assert_eq!(subscriptions(&app), visible_idle - 1);
        app.hidden_to_tray = false;
        app.is_running = false;
        assert_eq!(subscriptions(&app), visible_idle);
    }
    #[tokio::test]
    async fn logging_waits_for_capacity_without_losing_entries() {
        let (tx, mut rx) = mpsc::channel(1);
        send_log(&tx, "Test", "first", LogLevel::Info).await;
        let pending = send_log(&tx, "Test", "second", LogLevel::Info);
        tokio::pin!(pending);
        assert!(
            tokio::time::timeout(Duration::from_millis(10), &mut pending)
                .await
                .is_err()
        );
        assert_eq!(rx.recv().await.unwrap().message, "first");
        pending.await;
        assert_eq!(rx.recv().await.unwrap().message, "second");
    }

    #[test]
    fn test_windows_add_menu_toggle_and_cancel() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        assert!(!app.show_add_menu);
        let _ = app.update_message(Message::OpenAddMenu);
        assert!(app.show_add_menu);
        let _ = app.update_message(Message::CloseAddMenu);
        assert!(!app.show_add_menu);
    }

    #[test]
    fn test_windows_create_profile_across_all_eleven_protocols() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        let initial_count = app.v2ray_profiles.len();
        for (i, proto) in protocols::ProfileType::ALL.iter().enumerate() {
            let _ = app.update_message(Message::CreateProfileWithType(*proto));
            assert_eq!(app.v2ray_profiles.len(), initial_count + i);
            assert_eq!(app.view_mode, ViewMode::EditNode);
            assert!(app.node_edit_state.is_new);
            assert!(!app.show_add_menu);

            let _ = app.update_message(Message::SaveEditedNode);
            assert_eq!(app.v2ray_profiles.len(), initial_count + i + 1);
            assert_eq!(app.view_mode, ViewMode::Profiles);
            let created = &app.v2ray_profiles[0];
            assert_eq!(app.active_profile_id, Some(created.id.clone()));
        }
    }

    #[test]
    fn test_windows_create_profile_cancel_setup_flow() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        let initial_count = app.v2ray_profiles.len();
        let _ = app.update_message(Message::OpenAddMenu);
        assert!(app.show_add_menu);
        let _ = app.update_message(Message::CreateProfileWithType(protocols::ProfileType::VLess));
        assert!(!app.show_add_menu);
        assert_eq!(app.view_mode, ViewMode::EditNode);
        assert!(app.node_edit_state.is_new);
        assert_eq!(app.v2ray_profiles.len(), initial_count);

        let _ = app.update_message(Message::CancelEditNode);
        assert_eq!(app.view_mode, ViewMode::Profiles);
        assert_eq!(app.v2ray_profiles.len(), initial_count);
        assert!(!app.node_edit_state.is_new);
    }

    #[test]
    fn test_windows_clipboard_import_confirmation_flow() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        app.instant_import = false;
        let sample_link = "vless://00000000-0000-0000-0000-000000000000@1.2.3.4:443?type=tcp#WinConfirmedNode";
        let initial_count = app.v2ray_profiles.len();

        let _ = app.update_message(Message::ClipboardImported(Some(sample_link.to_string())));
        assert_eq!(app.clipboard_import_candidate, Some(sample_link.to_string()));
        assert_eq!(app.v2ray_profiles.len(), initial_count);

        let _ = app.update_message(Message::ConfirmClipboardImport);
        assert_eq!(app.clipboard_import_candidate, None);
        assert_eq!(app.v2ray_profiles.len(), initial_count + 1);
        let top = &app.v2ray_profiles[0];
        assert_eq!(top.name, "WinConfirmedNode");
        assert_eq!(top.config, sample_link);
        assert_eq!(app.active_profile_id, Some(top.id.clone()));
    }

    #[test]
    fn test_windows_clipboard_import_cancel_flow() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        app.instant_import = false;
        let sample_link = "trojan://secret@9.9.9.9:443#WinCancelledNode";
        let initial_count = app.v2ray_profiles.len();

        let _ = app.update_message(Message::ClipboardImported(Some(sample_link.to_string())));
        assert_eq!(app.clipboard_import_candidate, Some(sample_link.to_string()));
        assert_eq!(app.v2ray_profiles.len(), initial_count);

        let _ = app.update_message(Message::CancelClipboardImport);
        assert_eq!(app.clipboard_import_candidate, None);
        assert_eq!(app.v2ray_profiles.len(), initial_count);
    }

    #[test]
    fn test_windows_clipboard_instant_import_toggle() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        let _ = app.update_message(Message::ToggleInstantImport(true));
        assert!(app.instant_import);

        let sample_link = "ss://YWVzLTEyOC1nY206cGFzcw@8.8.8.8:8388#WinInstantNode";
        let initial_count = app.v2ray_profiles.len();

        let _ = app.update_message(Message::ClipboardImported(Some(sample_link.to_string())));
        assert_eq!(app.clipboard_import_candidate, None);
        assert_eq!(app.v2ray_profiles.len(), initial_count + 1);
        assert_eq!(app.v2ray_profiles[0].name, "WinInstantNode");

        let _ = app.update_message(Message::ToggleInstantImport(false));
        assert!(!app.instant_import);
    }

    #[test]
    fn test_windows_share_modal_open_copy_and_close_flow() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        let profile_id = app.v2ray_profiles[0].id.clone();
        assert!(app.share_modal.is_none());

        let _ = app.update_message(Message::ShareProfile(profile_id.clone()));
        assert!(app.share_modal.is_some());
        let modal = app.share_modal.as_ref().unwrap();
        assert_eq!(modal.profile_id, profile_id);
        assert!(!modal.link.is_empty());
        assert!(!modal.qr_svg.is_empty());
        assert!(!modal.is_protected);
        assert!(!modal.copied);

        let _ = app.update_message(Message::ToggleShareProtect);
        assert!(app.share_modal.as_ref().unwrap().is_protected);

        let _ = app.update_message(Message::CopyShareLink);
        assert!(app.share_modal.as_ref().unwrap().copied);

        let _ = app.update_message(Message::CloseShareModal);
        assert!(app.share_modal.is_none());
    }

    #[test]
    fn test_windows_theme_preference_lifecycle_and_switching() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        assert_eq!(app.theme_preference, storage::ThemePreference::System);

        let _ = app.update_message(Message::ThemePreferenceChanged(storage::ThemePreference::Light));
        assert_eq!(app.theme_preference, storage::ThemePreference::Light);
        assert!(!app.is_dark());
        assert_eq!(app.theme(), Theme::Light);

        let _ = app.update_message(Message::ThemePreferenceChanged(storage::ThemePreference::Dark));
        assert_eq!(app.theme_preference, storage::ThemePreference::Dark);
        assert!(app.is_dark());
        assert_eq!(app.theme(), Theme::Dark);

        let _ = app.update_message(Message::ThemePreferenceChanged(storage::ThemePreference::System));
        assert_eq!(app.theme_preference, storage::ThemePreference::System);
        assert!(matches!(app.theme(), Theme::Dark | Theme::Light));
    }

    #[test]
    fn test_windows_scrolling_memoization_and_cache_integrity() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());

        // 1. Verify profile display cache initialized for all default profiles
        assert_eq!(app.profile_display_cache.len(), app.v2ray_profiles.len());
        for p in &app.v2ray_profiles {
            let cached = app.profile_display_cache.get(&p.id).expect("profile must be in display cache");
            assert_eq!(cached.0, v2ray::extract_node_address(&p.config));
            assert_eq!(cached.1, v2ray::extract_protocol_tag(&p.config));
        }

        // 2. Add profile and verify cache updates automatically
        let _ = app.update_message(Message::CreateProfileWithType(protocols::ProfileType::VLess));
        let _ = app.update_message(Message::SaveEditedNode);
        let created_id = app.active_profile_id.clone().unwrap();
        assert!(app.profile_display_cache.contains_key(&created_id));

        // 3. Delete profile and verify cache cleans up
        let _ = app.update_message(Message::DeleteProfile(created_id.clone()));
        let _ = app.update_message(Message::ConfirmDeleteProfile);
        assert!(!app.profile_display_cache.contains_key(&created_id));

        // 4. Verify whitelist chips cache updates without re-parsing during view
        let _ = app.update_message(Message::WhitelistAppsChanged("firefox.exe, chrome.exe, discord.exe".into()));
        assert_eq!(app.cached_app_chips, vec!["firefox.exe", "chrome.exe", "discord.exe"]);

        // 5. Simulate multiple scroll view frames for Profiles screen
        app.view_mode = ViewMode::Profiles;
        for _ in 0..10 {
            let _element = app.view();
        }

        // 6. Simulate multiple scroll view frames for Settings screen
        app.view_mode = ViewMode::SshSettings;
        for _ in 0..10 {
            let _element = app.view();
        }
    }

    #[test]
    fn test_windows_startup_settings_and_connect_prompt_flow() {
        let mut data = storage::AppData::default();
        data.connect_at_startup = true;
        data.run_at_startup = true;
        let (mut app, _) = NetrayApp::from_saved(data);

        // connect_at_startup is true and default has profiles => prompt should be active
        assert!(app.run_at_startup);
        assert!(app.connect_at_startup);
        assert!(app.startup_connect_prompt);

        // Verify view renders confirmation prompt modal
        drop(app.view());

        // Toggle autostart settings
        let _ = app.update_message(Message::ToggleRunAtStartup(false));
        assert!(!app.run_at_startup);
        let _ = app.update_message(Message::ToggleConnectAtStartup(false));
        assert!(!app.connect_at_startup);

        // Dismiss confirmation prompt
        app.startup_connect_prompt = true;
        let _ = app.update_message(Message::DismissStartupConnectPrompt);
        assert!(!app.startup_connect_prompt);
        assert!(!app.worker_active);

        // Confirm confirmation prompt -> immediately connects tunnel directly without auth modal
        app.startup_connect_prompt = true;
        let _ = app.update_message(Message::ConfirmStartupConnectPrompt);
        assert!(!app.startup_connect_prompt);
        assert!(app.worker_active);
    }

    #[test]
    fn retry_limits_and_notification_preferences_remain_independent() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        app.notify_connection_drops = false;
        app.reconnect_attempts = MAX_RECONNECT_ATTEMPTS;
        let _ = app.handle_detected_error("core exited");
        assert!(!app.worker_active);
        assert_eq!(app.error_notification.as_ref().unwrap().countdown_secs, None);
        app.reconnect_attempts = 0;
        app.error_notification = None;
        let _ = app.handle_detected_error("core exited");
        assert!(app.error_notification.as_ref().unwrap().countdown_secs.is_some());
        let _ = app.update_message(Message::ToggleAutoReconnect(false));
        assert_eq!(app.error_notification.as_ref().unwrap().countdown_secs, None);
        let _ = app.update_message(Message::ReconnectCountdownTick);
        assert!(!app.worker_active);
        let _ = app.update_message(Message::ToggleConnectionDropNotifications(true));
        assert!(app.notify_connection_drops);
        assert!(!app.auto_reconnect);
        let _ = app.update_message(Message::ToggleConnectionDropNotifications(false));
        assert!(!app.notify_connection_drops);
    }

    #[test]
    fn disabled_ping_uses_inactive_colors_in_both_themes() {
        for is_dark in [true, false] {
            let style = ping_btn_style(is_dark, false, false)(&Theme::Dark, button::Status::Disabled);
            assert_eq!(style.text_color, theme::icon_color_for_state(is_dark, false));
            assert_ne!(style.background, Some(ACCENT_BLUE.into()));
        }
    }

    #[test]
    fn profile_deletion_requires_confirmation_and_respects_lock() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        let id = app.v2ray_profiles[0].id.clone();
        let count = app.v2ray_profiles.len();
        let _ = app.update_message(Message::DeleteProfile(id.clone()));
        assert_eq!(app.v2ray_profiles.len(), count);
        assert_eq!(app.delete_confirmation.as_ref(), Some(&id));
        let _ = app.update_message(Message::CancelDeleteProfile);
        let _ = app.update_message(Message::ConfirmDeleteProfile);
        assert_eq!(app.v2ray_profiles.len(), count);
        let _ = app.update_message(Message::DeleteProfile(id.clone()));
        app.worker_active = true;
        let _ = app.update_message(Message::ConfirmDeleteProfile);
        assert_eq!(app.v2ray_profiles.len(), count);
        app.worker_active = false;
        let _ = app.update_message(Message::ConfirmDeleteProfile);
        assert!(!app.v2ray_profiles.iter().any(|profile| profile.id == id));
        assert!(app.delete_confirmation.is_none());
        assert!(app.v2ray_uri_or_json.is_empty());
        assert!(storage::load_data().v2ray_profiles.is_empty());
    }

    #[test]
    fn only_worker_exit_triggers_recovery() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        app.notify_connection_drops = false;
        app.worker_active = true;
        let _ = app.update_message(Message::NewLog(LogEntry::new("DNS", "resolved", LogLevel::Success)));
        assert!(!app.is_running, "DNS success is not tunnel readiness");
        let _ = app.update_message(Message::NewLog(LogEntry::new("Supervisor", "Tunnel ready · HTTP 204 through proxy in 64 ms.", LogLevel::Success)));
        let connected_at = app.connected_at;
        for _ in 0..10 {
            let _ = app.update_message(Message::NewLog(LogEntry::new("Error", "Connection check failed: HTTP 503", LogLevel::Error)));
        }
        assert!(app.is_running);
        assert!(app.connection_error);
        assert_eq!(app.connected_at, connected_at);
        assert!(app.error_notification.is_none());
        let _ = app.update_message(Message::ExecuteAutoReconnect);
        assert!(app.is_running, "Retry must not replace a live worker");
        let _ = app.update_message(Message::NewLog(LogEntry::new(
            "Xray",
            "proxy/vless/outbound: tunneling request to tcp:api.github.com:443 via server:443",
            LogLevel::Info,
        )));
        assert!(!app.connection_error, "Successful outbound traffic clears the error state");
        app.refresh_status_badge();
        assert!(app.status_badge_text.starts_with("CONNECTED"));
        let _ = app.update_message(Message::NewLog(LogEntry::new("Supervisor", "Tunnel is offline.", LogLevel::Warn)));
        assert!(!app.worker_active);
        assert_eq!(app.reconnect_attempts, 1);
        assert_eq!(app.error_notification.as_ref().unwrap().countdown_secs, Some(RECONNECT_COUNTDOWN_SECONDS));
        for _ in 0..RECONNECT_COUNTDOWN_SECONDS {
            let _ = app.update_message(Message::ReconnectCountdownTick);
        }
        assert!(app.worker_active);
        let _ = app.update_message(Message::ToggleTunnel);
        assert!(app.user_stopped);
        let _ = app.update_message(Message::NewLog(LogEntry::new("Supervisor", "Tunnel ready · HTTP 204 through proxy in 64 ms.", LogLevel::Success)));
        assert!(!app.is_running, "Late readiness must not undo Disconnect");
        let _ = app.update_message(Message::NewLog(LogEntry::new("Supervisor", "Tunnel is offline.", LogLevel::Warn)));
        assert!(!app.worker_active);
        assert!(app.error_notification.is_none());
    }

    #[test]
    fn test_windows_both_tun_and_proxy_can_be_selected_simultaneously() {
        let mut data = storage::AppData::default();
        data.enable_tun = true;
        data.enable_sysproxy = true;
        let (mut app, _) = NetrayApp::from_saved(data);

        // 1. Initial state from saved preserves both flags
        assert!(app.enable_tun);
        assert!(app.enable_sysproxy);

        // 2. Toggle proxy off -> tun remains true
        let _ = app.update_message(Message::ToggleSysProxy(false));
        assert!(app.enable_tun);
        assert!(!app.enable_sysproxy);

        // 3. Toggle proxy on -> tun remains true (both active)
        let _ = app.update_message(Message::ToggleSysProxy(true));
        assert!(app.enable_tun);
        assert!(app.enable_sysproxy);

        // 4. Toggle tun off -> proxy remains true
        let _ = app.update_message(Message::ToggleTunMode(false));
        assert!(!app.enable_tun);
        assert!(app.enable_sysproxy);

        // 5. Toggle tun on -> proxy remains true (both active)
        let _ = app.update_message(Message::ToggleTunMode(true));
        assert!(app.enable_tun);
        assert!(app.enable_sysproxy);

        // 6. View rendering check for both selected
        drop(app.view());
    }

    #[test]
    fn test_windows_file_import_lifecycle() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        let initial_count = app.v2ray_profiles.len();

        // 1. User cancels file picker -> None
        let _ = app.update_message(Message::ConfigFileSelected(None));
        assert_eq!(app.v2ray_profiles.len(), initial_count);

        // 2. Empty file selected -> warns, doesn't add
        let _ = app.update_message(Message::ConfigFileSelected(Some(("empty.txt".into(), "   ".into()))));
        assert_eq!(app.v2ray_profiles.len(), initial_count);

        // 3. Multi-node file selected
        let sample_file_content = "\
vless://11111111-1111-1111-1111-111111111111@server1.com:443?type=tcp#WinNodeA
trojan://secretpass@server2.com:443?type=ws#WinNodeB
";
        let _ = app.update_message(Message::ConfigFileSelected(Some((
            "exported_nodes.txt".into(),
            sample_file_content.into(),
        ))));

        assert_eq!(app.v2ray_profiles.len(), initial_count + 2);
        assert_eq!(app.v2ray_profiles[0].name, "WinNodeA");
        assert_eq!(app.v2ray_profiles[1].name, "WinNodeB");
        assert_eq!(app.active_profile_id, Some(app.v2ray_profiles[0].id.clone()));
        assert_eq!(app.v2ray_uri_or_json, app.v2ray_profiles[0].config);
        assert_eq!(app.tunnel_mode, TunnelMode::V2RayXray);

        // 4. Single JSON config file selected
        let json_content = r#"{"outbounds":[{"protocol":"vless","tag":"CustomWinJsonServer"}]}"#;
        let _ = app.update_message(Message::ConfigFileSelected(Some((
            "xray_custom.json".into(),
            json_content.into(),
        ))));

        assert_eq!(app.v2ray_profiles.len(), initial_count + 3);
        assert_eq!(app.v2ray_profiles[0].name, "CustomWinJsonServer");
        assert_eq!(app.active_profile_id, Some(app.v2ray_profiles[0].id.clone()));

        // 5. Verify view renders with new profiles
        drop(app.view());
    }

    #[test]
    fn test_qr_scanner_modal_and_import_flow() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());

        // 1. Initially QR scanner modal is closed
        assert!(!app.qr_scan_modal.is_open);
        assert_eq!(app.qr_scan_modal.status, QrScanStatus::Idle);

        // 2. Open QR modal
        let _ = app.update_message(Message::OpenQrScanModal);
        assert!(app.qr_scan_modal.is_open);
        assert_eq!(app.qr_scan_modal.status, QrScanStatus::Idle);
        drop(app.view());

        // 3. Trigger Desktop Screenshot scan
        let _ = app.update_message(Message::TriggerQrScan(QrScanSource::DesktopScreenshot));
        assert_eq!(
            app.qr_scan_modal.status,
            QrScanStatus::Scanning(QrScanSource::DesktopScreenshot)
        );
        drop(app.view());

        // 4. Trigger Webcam scan
        let _ = app.update_message(Message::TriggerQrScan(QrScanSource::Webcam));
        assert_eq!(
            app.qr_scan_modal.status,
            QrScanStatus::Scanning(QrScanSource::Webcam)
        );
        drop(app.view());

        // 5. Trigger Photo Upload scan
        let _ = app.update_message(Message::TriggerQrScan(QrScanSource::PhotoUpload));
        assert_eq!(
            app.qr_scan_modal.status,
            QrScanStatus::Scanning(QrScanSource::PhotoUpload)
        );
        drop(app.view());

        // 6. User cancels file selection
        let _ = app.update_message(Message::QrScanFinished(Err("File selection was cancelled".into())));
        assert_eq!(app.qr_scan_modal.status, QrScanStatus::Idle);

        // 7. Scan error simulation
        let _ = app.update_message(Message::QrScanFinished(Err("No QR code detected".into())));
        assert_eq!(
            app.qr_scan_modal.status,
            QrScanStatus::Error("No QR code detected".into())
        );
        drop(app.view());

        // 8. Successful scan with VLESS node link
        let initial_profiles = app.v2ray_profiles.len();
        let scanned_vless = "vless://a1b2c3d4-e5f6-7890-abcd-ef1234567890@myvless.example.com:443?security=reality&type=tcp#Tokyo-Reality-01";
        let _ = app.update_message(Message::QrScanFinished(Ok(scanned_vless.into())));

        assert_eq!(app.v2ray_profiles.len(), initial_profiles + 1);
        let imported = &app.v2ray_profiles[0];
        assert_eq!(imported.name, "Tokyo-Reality-01");
        assert_eq!(imported.config, scanned_vless);
        assert_eq!(app.active_profile_id, Some(imported.id.clone()));
        assert_eq!(app.v2ray_uri_or_json, scanned_vless);
        assert_eq!(app.tunnel_mode, TunnelMode::V2RayXray);
        assert!(matches!(app.qr_scan_modal.status, QrScanStatus::Success(_)));
        drop(app.view());

        // 9. Close QR modal
        let _ = app.update_message(Message::CloseQrScanModal);
        assert!(!app.qr_scan_modal.is_open);
        drop(app.view());
    }

    #[test]
    fn test_cpu_optimization_zero_allocation_rendering_benchmark() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());

        // Clear default initial profiles and populate 10 sample profiles
        app.v2ray_profiles.clear();
        for i in 1..=10 {
            app.v2ray_profiles.push(storage::V2RayProfile {
                id: format!("node-{}", i),
                name: format!("Node {}", i),
                config: format!("vless://uuid-{}@node{}.com:443?type=ws#Node{}", i, i, i),
                created_at: "2026-10-31T00:00:00Z".into(),
            });
        }
        app.sync_profile_display_cache();
        assert_eq!(app.profile_count_label, "10 profiles");

        // 1. Test index-dispatched profile operations
        let _ = app.update_message(Message::SelectProfileIndex(0));
        assert_eq!(app.active_profile_id, Some("node-1".into()));

        let _ = app.update_message(Message::SelectProfileIndex(4));
        assert_eq!(app.active_profile_id, Some("node-5".into()));

        // 2. Test uptime badge memoization
        app.is_running = true;
        app.connected_at = Some(Instant::now());
        app.ping_ms = Some(42);
        app.refresh_status_badge();
        assert!(app.status_badge_text.contains("CONNECTED"));
        assert!(app.status_badge_text.contains("42 ms"));

        // 3. High-frequency view rendering benchmark (simulating 1000 frames of mouse hover and scrolling)
        let start = Instant::now();
        for _ in 0..1000 {
            let el = app.view();
            drop(el);
        }
        let elapsed = start.elapsed();
        assert!(elapsed.as_millis() < 2000, "1000 view iterations took {:?}", elapsed);

        // 4. Test index-based deletion (when tunnel is stopped / config unlocked)
        app.is_running = false;
        let _ = app.update_message(Message::DeleteProfileIndex(0));
        let _ = app.update_message(Message::ConfirmDeleteProfile);
        app.sync_profile_display_cache();
        assert_eq!(app.profile_count_label, "9 profiles");
    }

}

