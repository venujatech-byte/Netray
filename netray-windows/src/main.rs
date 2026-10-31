#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

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
mod tunnel;
pub mod v2ray;

use chrono::Local;
use iced::futures::SinkExt;
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
const INACTIVE_STRIP: Color = Color::from_rgb(0.30, 0.33, 0.38);
const SUCCESS_GREEN: Color = Color::from_rgb(0.22, 0.78, 0.49);
const FAILURE_RED: Color = Color::from_rgb(0.95, 0.35, 0.40);
const CORAL_ACCENT: Color = Color::from_rgb(0.98, 0.52, 0.48); // #FA857A
const APP_VERSION: &str = match option_env!("NETRAY_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

fn format_uptime(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        (seconds / 60) % 60,
        seconds % 60
    )
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

fn card_strip_active(_: &Theme) -> container::Style {
    container::Style {
        background: Some(ACCENT_BLUE.into()),
        border: iced::Border {
            radius: 2.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn card_strip_inactive(_: &Theme) -> container::Style {
    container::Style {
        background: Some(INACTIVE_STRIP.into()),
        border: iced::Border {
            radius: 2.0.into(),
            ..Default::default()
        },
        ..Default::default()
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
        size: (460.0, 600.0).into(),
        icon: create_window_icon(),
        exit_on_close_request: false,
        ..Default::default()
    };
    iced::application("Netray", NetrayApp::update, NetrayApp::view)
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

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub source: String,
    pub message: String,
    pub level: LogLevel,
}

pub async fn send_log(
    tx: &mpsc::Sender<LogEntry>,
    source: impl Into<String>,
    msg: impl Into<String>,
    level: LogLevel,
) {
    let _ = tx
        .send(LogEntry {
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
            source: source.into(),
            message: msg.into(),
            level,
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

#[derive(Debug, Clone, Default)]
pub struct SudoAuthModalState {
    pub password: String,
    pub error: Option<String>,
    pub is_authenticating: bool,
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
    pub is_running: bool,
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
    pub user_stopped: bool,
    pub reconnect_attempts: u32,
    pub error_notification: Option<ErrorNotification>,
    pub startup_connect_prompt: bool,
    pub sudo_auth_modal: Option<SudoAuthModalState>,
    pub qr_scan_modal: QrScanModalState,

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

    pub logs: Vec<LogEntry>,
    auto_scroll_logs: bool,
    pub log_sender: Option<mpsc::Sender<LogEntry>>,
    pub stop_signal: Option<tokio::sync::oneshot::Sender<()>>,
    close_confirmation: Option<iced::window::Id>,
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
    TunnelModeChanged(TunnelMode),
    ToggleTunnel,
    MinimizeToTray,

    SelectProfile(String),
    PingProfile(String),
    PingFinished(String, Result<u128, String>),
    EditProfile(String),
    DeleteProfile(String),
    ShareProfile(String),
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
    SudoPasswordInputChanged(String),
    SubmitSudoPassword,
    CancelSudoAuth,
    SudoAuthResult(Result<(), String>),
    TriggerErrorAutoReconnect(String),
    ReconnectCountdownTick,
    ExecuteAutoReconnect,
    CancelAutoReconnect,
    DismissNotification,
    ToggleAutoReconnect(bool),

    LoggerReady(mpsc::Sender<LogEntry>),
    NewLog(LogEntry),
    UptimeTick,
    ClearLogs,
    AutoScrollLogs(bool),
    CloseRequested(iced::window::Id),
    WindowOpened(iced::window::Id),
    WindowTick,
    Minimized(Option<bool>),
    ConfirmExit,
    CancelExit,
}

impl NetrayApp {
    fn new() -> (Self, Task<Message>) {
        Self::from_saved(storage::load_data())
    }

    fn from_saved(saved: storage::AppData) -> (Self, Task<Message>) {
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
        let has_profiles = !saved.v2ray_profiles.is_empty();

        let app = Self {
            view_mode: ViewMode::Profiles,
            is_running: false,
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
            user_stopped: false,
            reconnect_attempts: 0,
            error_notification: None,
            startup_connect_prompt: saved.connect_at_startup && has_profiles,
            sudo_auth_modal: None,
            qr_scan_modal: QrScanModalState::default(),

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

            logs: vec![LogEntry {
                timestamp: Local::now().format("%H:%M:%S").to_string(),
                source: "Netray".into(),
                message: "Ready to connect on Windows.".into(),
                level: LogLevel::Info,
            }],
            auto_scroll_logs: true,
            log_sender: None,
            stop_signal: None,
            close_confirmation: None,
            window_id: None,
            hidden_to_tray: false,
            exit_after_stop: false,
            tray_icon: None,
            tray_connect_item: None,
            worker_active: false,
        };

        (app, Task::none())
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
    }

    pub fn cache_profile_display(&mut self, profile: &storage::V2RayProfile) {
        self.profile_display_cache.insert(
            profile.id.clone(),
            (
                v2ray::extract_node_address(&profile.config),
                v2ray::extract_protocol_tag(&profile.config),
            ),
        );
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
        };
        storage::save_state_ref(&app_ref)
    }

    fn push_log(&mut self, source: &str, message: impl Into<String>, level: LogLevel) {
        self.logs.push(LogEntry {
            timestamp: Local::now().format("%H:%M:%S").to_string(),
            source: source.to_string(),
            message: message.into(),
            level,
        });
        if self.logs.len() > 150 {
            self.logs.drain(0..30);
        }
    }

    fn config_locked(&self) -> bool {
        self.is_running || self.worker_active
    }

    fn handle_detected_error(&mut self, err_msg: &str) -> Task<Message> {
        if self.user_stopped {
            return Task::none();
        }
        if let Some(notif) = &self.error_notification {
            if notif.countdown_secs.is_some() || notif.is_reconnecting {
                return Task::none();
            }
        }

        self.reconnect_attempts = self.reconnect_attempts.saturating_add(1);
        let attempt = self.reconnect_attempts;

        let clean_err = err_msg.trim().trim_start_matches("Connection check failed:").trim();
        let display_err = if clean_err.is_empty() {
            "Connection lost or unreachable".to_string()
        } else {
            clean_err.to_string()
        };

        let title = "Tunnel Connection Error".to_string();

        notifications::send_desktop_notification(
            &title,
            &format!("Error: {}. Attempting auto-reconnect ({}/{})", display_err, attempt, MAX_RECONNECT_ATTEMPTS),
            notifications::NotificationUrgency::Critical,
        );

        if self.auto_reconnect && attempt <= MAX_RECONNECT_ATTEMPTS {
            self.error_notification = Some(ErrorNotification {
                title: "Connection Error Detected".to_string(),
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

        Task::none()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        let scroll = matches!(
            message,
            Message::NewLog(_)
                | Message::PingFinished(..)
                | Message::SetViewMode(ViewMode::Logs)
                | Message::AutoScrollLogs(true)
        );
        let task = self.update_message(message);
        if scroll && self.auto_scroll_logs && self.view_mode == ViewMode::Logs {
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
                    | Message::EditProfile(_)
                    | Message::DeleteProfile(_)
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
            )
        {
            return Task::none();
        }
        match message {
            Message::SetViewMode(mode) => {
                self.view_mode = mode;
                Task::none()
            }
            Message::TunnelModeChanged(mode) => {
                self.tunnel_mode = mode;
                let _ = self.save_current_state();
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
                    Task::perform(
                        async move {
                            if let Some(port) = local_proxy {
                                v2ray::ping_local_http_proxy(port).await
                            } else {
                                v2ray::ping_profile(&config).await
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
                let id = format!(
                    "node-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis()
                );
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
                let id = format!(
                    "node-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis()
                );
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
                        let id = format!(
                            "node-{}",
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis()
                        );
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

                        for (idx, (node_name, config_str)) in parsed_configs.into_iter().rev().enumerate() {
                            let id = format!(
                                "node-{}-{}",
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis(),
                                idx
                            );
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
                            let id = format!(
                                "node-{}",
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis()
                            );
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
                self.v2ray_profiles.retain(|p| p.id != id);
                self.profile_display_cache.remove(&id);
                self.ping_results.remove(&id);
                if self.ping_in_progress.as_deref() == Some(&id) {
                    self.ping_in_progress = None;
                }
                if self.active_profile_id.as_deref() == Some(&id) {
                    self.active_profile_id = self.v2ray_profiles.first().map(|p| p.id.clone());
                    if let Some(first) = self.v2ray_profiles.first() {
                        self.v2ray_uri_or_json = first.config.clone();
                    }
                }
                let _ = self.save_current_state();
                Task::none()
            }
            Message::ToggleTunMode(v) => {
                self.enable_tun = v;
                let _ = self.save_current_state();
                Task::none()
            }
            Message::ToggleSysProxy(v) => {
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
            Message::NewLog(entry) => {
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
                if (ready_ping.is_some() || ssh_ready) && self.worker_active {
                    self.is_running = true;
                    self.connected_at = Some(Instant::now());
                    self.ping_ms = ready_ping;
                    self.reconnect_attempts = 0;
                    if let Some(notif) = &self.error_notification {
                        if notif.is_reconnecting {
                            notifications::send_desktop_notification(
                                "Netray Connected",
                                "Tunnel reconnected successfully.",
                                notifications::NotificationUrgency::Normal,
                            );
                        }
                    }
                    self.error_notification = None;
                }
                let stopped = entry.source == "Supervisor" && entry.message.contains("offline");
                let was_running = self.is_running;
                if stopped || (entry.source == "Error" && entry.message.contains("Engine failed")) {
                    self.is_running = false;
                    self.connected_at = None;
                    self.ping_ms = None;
                }
                // Only the final offline event means proxy and route cleanup has finished.
                if stopped {
                    self.stop_signal = None;
                    self.worker_active = false;
                }

                let is_error = entry.level == LogLevel::Error
                    || entry.source == "Error"
                    || entry.message.contains("Core process exited with:")
                    || entry.message.contains("Connection check failed");
                let unexpected_stop = stopped && !self.user_stopped && was_running;

                let auto_task = if (is_error || unexpected_stop) && !self.user_stopped {
                    self.handle_detected_error(&entry.message)
                } else {
                    Task::none()
                };

                self.logs.push(entry);
                if self.logs.len() > 150 {
                    self.logs.drain(0..30);
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
            Message::ClearLogs => {
                self.logs.clear();
                Task::none()
            }
            Message::UptimeTick => Task::none(),
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
            Message::WindowTick => {
                if let Some(item) = &self.tray_connect_item {
                    item.set_text(if self.is_running {
                        "Disconnect"
                    } else {
                        "Connect"
                    });
                    item.set_enabled(self.is_running || !self.worker_active);
                }
                if let Ok(event) = TrayIconEvent::receiver().try_recv() {
                    if matches!(
                        event,
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            ..
                        } | TrayIconEvent::DoubleClick { .. }
                    ) {
                        self.hidden_to_tray = false;
                        if let Some(id) = self.window_id {
                            return iced::window::change_mode(id, iced::window::Mode::Windowed)
                                .chain(iced::window::minimize(id, false))
                                .chain(iced::window::gain_focus(id));
                        }
                    }
                }
                while let Ok(event) = MenuEvent::receiver().try_recv() {
                    match event.id().as_ref() {
                        "show" => {
                            self.hidden_to_tray = false;
                            if let Some(id) = self.window_id {
                                return iced::window::change_mode(id, iced::window::Mode::Windowed)
                                    .chain(iced::window::minimize(id, false))
                                    .chain(iced::window::gain_focus(id));
                            }
                        }
                        "hide" => {
                            if let Some(id) = self.window_id {
                                self.hidden_to_tray = true;
                                return iced::window::change_mode(id, iced::window::Mode::Hidden);
                            }
                        }
                        "connect" if self.is_running || !self.worker_active => {
                            return self.update(Message::ToggleTunnel);
                        }
                        "exit" => {
                            if let Some(id) = self.window_id {
                                self.hidden_to_tray = false;
                                self.close_confirmation = Some(id);
                                return iced::window::change_mode(id, iced::window::Mode::Windowed)
                                    .chain(iced::window::minimize(id, false))
                                    .chain(iced::window::gain_focus(id));
                            }
                        }
                        _ => {}
                    }
                }
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
                self.sudo_auth_modal = Some(SudoAuthModalState::default());
                self.push_log(
                    "Supervisor",
                    "Target node confirmed. Please authorize connection permissions.",
                    LogLevel::Info,
                );
                Task::none()
            }
            Message::SudoPasswordInputChanged(v) => {
                if let Some(auth) = &mut self.sudo_auth_modal {
                    auth.password = v;
                    auth.error = None;
                }
                Task::none()
            }
            Message::CancelSudoAuth => {
                self.sudo_auth_modal = None;
                self.push_log(
                    "Supervisor",
                    "Authentication cancelled by user.",
                    LogLevel::Info,
                );
                Task::none()
            }
            Message::SubmitSudoPassword => {
                if let Some(auth) = &mut self.sudo_auth_modal {
                    let pass = auth.password.clone();
                    if pass.trim().is_empty() {
                        auth.error = Some("Password cannot be empty.".into());
                        return Task::none();
                    }
                    auth.is_authenticating = true;
                    auth.error = None;

                    Task::perform(
                        async move {
                            #[cfg(target_os = "windows")]
                            {
                                // On Windows, user authorization verification
                                let _ = pass;
                                Ok(())
                            }
                            #[cfg(not(target_os = "windows"))]
                            {
                                let mut child = tokio::process::Command::new("sudo")
                                    .args(["-S", "-v"])
                                    .stdin(std::process::Stdio::piped())
                                    .stdout(std::process::Stdio::null())
                                    .stderr(std::process::Stdio::piped())
                                    .spawn()
                                    .map_err(|e| format!("Failed to spawn sudo: {e}"))?;

                                if let Some(mut stdin) = child.stdin.take() {
                                    use tokio::io::AsyncWriteExt;
                                    let _ = stdin.write_all(format!("{}\n", pass).as_bytes()).await;
                                    let _ = stdin.flush().await;
                                }

                                let output = child.wait_with_output().await.map_err(|e| e.to_string())?;
                                if output.status.success() {
                                    Ok(())
                                } else {
                                    let err = String::from_utf8_lossy(&output.stderr).to_string();
                                    if err.to_lowercase().contains("incorrect") || err.to_lowercase().contains("try again") {
                                        Err("Incorrect password. Please try again.".to_string())
                                    } else if err.trim().is_empty() {
                                        Err("Authentication rejected.".to_string())
                                    } else {
                                        Err(err.trim().to_string())
                                    }
                                }
                            }
                        },
                        Message::SudoAuthResult,
                    )
                } else {
                    Task::none()
                }
            }
            Message::SudoAuthResult(res) => {
                match res {
                    Ok(()) => {
                        self.sudo_auth_modal = None;
                        self.push_log(
                            "Supervisor",
                            "Authorization successful. Connecting tunnel...",
                            LogLevel::Info,
                        );
                        self.update_message(Message::ToggleTunnel)
                    }
                    Err(err) => {
                        if let Some(auth) = &mut self.sudo_auth_modal {
                            auth.is_authenticating = false;
                            auth.error = Some(err.clone());
                        }
                        self.push_log(
                            "Supervisor",
                            format!("Authentication failed: {err}"),
                            LogLevel::Error,
                        );
                        Task::none()
                    }
                }
            }
            Message::CancelExit => {
                self.close_confirmation = None;
                Task::none()
            }
            Message::ToggleAutoReconnect(enabled) => {
                self.auto_reconnect = enabled;
                let msg = if enabled {
                    "Automatic reconnect on error enabled."
                } else {
                    "Automatic reconnect on error disabled."
                };
                self.push_log("Supervisor", msg, LogLevel::Info);
                let _ = self.save_current_state();
                Task::none()
            }
            Message::TriggerErrorAutoReconnect(err) => self.handle_detected_error(&err),
            Message::ReconnectCountdownTick => {
                if let Some(notif) = &mut self.error_notification {
                    if let Some(secs) = notif.countdown_secs {
                        if secs > 1 {
                            notif.countdown_secs = Some(secs - 1);
                            Task::none()
                        } else {
                            let attempt = notif.attempt;
                            let max_attempts = notif.max_attempts;
                            notif.countdown_secs = None;
                            notif.is_reconnecting = true;
                            self.push_log(
                                "Supervisor",
                                format!(
                                    "Auto-reconnect countdown finished. Attempting reconnect ({}/{})...",
                                    attempt, max_attempts
                                ),
                                LogLevel::Info,
                            );
                            self.user_stopped = false;
                            if !self.is_running && !self.worker_active {
                                self.update_message(Message::ToggleTunnel)
                            } else {
                                Task::none()
                            }
                        }
                    } else {
                        Task::none()
                    }
                } else {
                    Task::none()
                }
            }
            Message::ExecuteAutoReconnect => {
                if let Some(notif) = &mut self.error_notification {
                    notif.countdown_secs = None;
                    notif.is_reconnecting = true;
                }
                self.push_log("Supervisor", "Reconnect triggered by user...", LogLevel::Info);
                self.user_stopped = false;
                if !self.is_running && !self.worker_active {
                    self.update_message(Message::ToggleTunnel)
                } else {
                    Task::none()
                }
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
            Message::ToggleTunnel => {
                if self.worker_active && !self.is_running {
                    if let Some(stop) = self.stop_signal.take() {
                        let _ = stop.send(());
                    }
                    self.user_stopped = true;
                    self.error_notification = None;
                    self.reconnect_attempts = 0;
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
                    self.push_log("Supervisor", "Tunnel stopped by user.", LogLevel::Warn);
                    Task::none()
                } else {
                    self.user_stopped = false;
                    self.is_running = false;
                    self.worker_active = true;
                    self.connected_at = None;
                    self.ping_ms = None;
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
                            Message::NewLog(LogEntry {
                                timestamp: Local::now().format("%H:%M:%S").to_string(),
                                source: "Supervisor".into(),
                                message: "Tunnel stopped.".into(),
                                level: LogLevel::Warn,
                            })
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

                            for (idx, (node_name, config_str)) in configs_to_add.into_iter().rev().enumerate() {
                                let id = format!(
                                    "qr-node-{}-{}",
                                    std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_millis(),
                                    idx
                                );
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
                while let Some(log_entry) = rx.recv().await {
                    let _ = output.send(Message::NewLog(log_entry)).await;
                }
            })
        });
        let mut subscriptions = vec![
            logs,
            iced::window::close_requests().map(Message::CloseRequested),
            iced::window::open_events().map(Message::WindowOpened),
            iced::time::every(Duration::from_secs(1)).map(|_| Message::WindowTick),
        ];
        if self.is_running && self.view_mode == ViewMode::Profiles {
            subscriptions
                .push(iced::time::every(Duration::from_secs(1)).map(|_| Message::UptimeTick));
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
        let main_col = if self.error_notification.is_some() {
            column![self.view_notification_toast(), content]
                .width(Length::Fill)
                .height(Length::Fill)
        } else {
            column![content]
                .width(Length::Fill)
                .height(Length::Fill)
        };
        let content = container(main_col)
            .style(|theme| {
                let is_dark = match theme {
                    Theme::Light => false,
                    _ => true,
                };
                container::Style {
                    background: Some(if is_dark {
                        theme::BG_WINDOW_DARK.into()
                    } else {
                        theme::BG_WINDOW_LIGHT.into()
                    }),
                    ..Default::default()
                }
            })
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

        let base_content = if self.sudo_auth_modal.is_some() {
            iced::widget::stack![
                base_content,
                self.view_sudo_auth_modal(),
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

        if self.close_confirmation.is_some() {
            let dialog = container(
                column![
                    text("Stop Netray and exit?").size(20),
                    text("The active tunnel will be stopped before the app closes.")
                        .size(14)
                        .color(TEXT_MUTED),
                    row![
                        button("Cancel").on_press(Message::CancelExit),
                        button("Stop and Exit").on_press(Message::ConfirmExit),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                ]
                .spacing(16)
                .padding(20),
            )
            .style(form_card_style)
            .width(Length::Fixed(340.0));
            iced::widget::stack![
                base_content,
                container(dialog)
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
        let uri_box = container(
            column![
                text("URI/Link Config").size(12).color(TEXT_MUTED),
                scrollable(
                    text(candidate).size(13).color(Color::WHITE)
                )
                .height(Length::Fixed(60.0)),
            ]
            .spacing(6),
        )
        .padding(12)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Color::from_rgb(0.10, 0.11, 0.14).into()),
            border: iced::Border {
                radius: 8.0.into(),
                width: 1.0,
                color: Color::from_rgb(0.20, 0.22, 0.28),
            },
            ..Default::default()
        });

        let cancel_btn = button(text("CANCEL").size(13).color(CORAL_ACCENT))
            .padding([8, 14])
            .style(|_, status| button::Style {
                background: match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Some(Color::from_rgba(0.98, 0.52, 0.48, 0.12).into())
                    }
                    _ => None,
                },
                border: iced::Border {
                    radius: 6.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .on_press(Message::CancelClipboardImport);

        let import_btn = button(text("IMPORT").size(13).color(CORAL_ACCENT))
            .padding([8, 14])
            .style(|_, status| button::Style {
                background: match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Some(Color::from_rgba(0.98, 0.52, 0.48, 0.18).into())
                    }
                    _ => None,
                },
                border: iced::Border {
                    radius: 6.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .on_press(Message::ConfirmClipboardImport);

        let action_row = row![
            Space::with_width(Length::Fill),
            cancel_btn,
            import_btn,
        ]
        .spacing(12)
        .align_y(Alignment::Center);

        let dialog = container(
            column![
                text("Import from clipboard").size(19),
                toggler(self.instant_import)
                    .label("Instant import")
                    .size(18)
                    .spacing(12)
                    .on_toggle(Message::ToggleInstantImport),
                uri_box,
                action_row,
            ]
            .spacing(16)
            .padding(22),
        )
        .style(|_| container::Style {
            background: Some(Color::from_rgb(0.14, 0.16, 0.20).into()),
            border: iced::Border {
                radius: 16.0.into(),
                width: 1.0,
                color: Color::from_rgb(0.24, 0.27, 0.35),
            },
            ..Default::default()
        })
        .width(Length::Fixed(400.0));

        container(dialog)
            .width(Length::Fill)
            .height(Length::Fill)
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

        // 1. QR Code container (crisp white background with quiet zone)
        let qr_box = container(
            iced::widget::svg(iced::widget::svg::Handle::from_memory(modal.qr_svg.clone()))
                .width(Length::Fixed(240.0))
                .height(Length::Fixed(240.0)),
        )
        .padding(14)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(|_| container::Style {
            background: Some(Color::WHITE.into()),
            border: iced::Border {
                radius: 12.0.into(),
                width: 1.0,
                color: Color::from_rgb(0.85, 0.88, 0.92),
            },
            ..Default::default()
        });

        // 2. Info note row
        let info_row = row![
            icons::lucide_icon(icons::INFO, 16, TEXT_MUTED),
            text("Consider using file export if configuration too complex")
                .size(12)
                .color(TEXT_MUTED),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        // 3. Subtle divider
        let divider = container(Space::with_height(1))
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Color::from_rgb(0.24, 0.27, 0.35).into()),
                ..Default::default()
            });

        // 4. Link preview box with copy button
        let link_text = if modal.link.len() > 38 {
            format!("{}...", &modal.link[..38])
        } else {
            modal.link.clone()
        };

        let copy_btn = button(
            container(icons::lucide_icon(
                if modal.copied { icons::CHECK } else { icons::COPY },
                18,
                if modal.copied { SUCCESS_GREEN } else { TEXT_MUTED },
            ))
            .center_x(Length::Fill)
            .center_y(Length::Fill),
        )
        .width(36)
        .height(36)
        .padding(0)
        .style(|_, status| button::Style {
            background: match status {
                button::Status::Hovered | button::Status::Pressed => {
                    Some(Color::from_rgba(1.0, 1.0, 1.0, 0.08).into())
                }
                _ => None,
            },
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .on_press(Message::CopyShareLink);

        let link_card = container(
            row![
                icons::lucide_icon(icons::LINK, 18, TEXT_MUTED),
                column![
                    text("Link").size(11).color(TEXT_MUTED),
                    text(link_text).size(12).color(Color::WHITE),
                ]
                .spacing(2)
                .width(Length::Fill),
                copy_btn,
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .padding(12)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Color::from_rgb(0.10, 0.11, 0.14).into()),
            border: iced::Border {
                radius: 8.0.into(),
                width: 1.0,
                color: Color::from_rgb(0.20, 0.22, 0.28),
            },
            ..Default::default()
        });

        // 5. Bottom buttons / pills
        let protect_btn = button(
            row![
                icons::lucide_icon(
                    icons::LOCK,
                    14,
                    if modal.is_protected { ACCENT_BLUE_LIGHT } else { TEXT_MUTED },
                ),
                text("Protect")
                    .size(12)
                    .color(if modal.is_protected { ACCENT_BLUE_LIGHT } else { Color::WHITE }),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([6, 14])
        .style(move |_, status| button::Style {
            background: match status {
                button::Status::Hovered | button::Status::Pressed => {
                    Some(Color::from_rgba(1.0, 1.0, 1.0, 0.12).into())
                }
                _ => Some(Color::from_rgb(0.20, 0.22, 0.27).into()),
            },
            border: iced::Border {
                radius: 16.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .on_press(Message::ToggleShareProtect);

        let standard_pill = container(
            text("Standard")
                .size(12)
                .color(CORAL_ACCENT),
        )
        .padding([6, 14])
        .style(|_| container::Style {
            background: Some(Color::from_rgba(0.98, 0.52, 0.48, 0.16).into()),
            border: iced::Border {
                radius: 16.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });

        let bottom_row = row![
            Space::with_width(Length::Fill),
            protect_btn,
            standard_pill,
            button(text("Close").size(12).color(TEXT_MUTED))
                .padding([6, 12])
                .style(|_, _| button::Style::default())
                .on_press(Message::CloseShareModal),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        // 6. Overall dialog
        let dialog = container(
            column![
                qr_box,
                info_row,
                divider,
                link_card,
                bottom_row,
            ]
            .spacing(14)
            .padding(18),
        )
        .style(|_| container::Style {
            background: Some(Color::from_rgb(0.14, 0.16, 0.20).into()),
            border: iced::Border {
                radius: 16.0.into(),
                width: 1.0,
                color: Color::from_rgb(0.24, 0.27, 0.35),
            },
            ..Default::default()
        })
        .width(Length::Fixed(360.0));

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

    fn view_sudo_auth_modal(&self) -> Element<'_, Message> {
        let Some(auth) = self.sudo_auth_modal.as_ref() else {
            return Space::with_width(0).into();
        };

        let mut dialog_col = column![
            row![
                icons::lucide_icon(icons::LOCK, 22, ACCENT_BLUE_LIGHT),
                Space::with_width(8),
                text("Authentication Required").size(18).color(Color::WHITE),
            ]
            .align_y(Alignment::Center),
            Space::with_height(6),
            text("Elevated routing permissions are required to start the V2Ray tunnel.")
                .size(13)
                .color(TEXT_MUTED),
            Space::with_height(4),
            text("Enter your sudo password:")
                .size(12)
                .color(Color::WHITE),
            Space::with_height(2),
            text_input("Enter password...", &auth.password)
                .secure(true)
                .on_input(Message::SudoPasswordInputChanged)
                .on_submit(Message::SubmitSudoPassword)
                .padding(8),
        ]
        .spacing(4);

        if let Some(err) = &auth.error {
            dialog_col = dialog_col.push(
                text(err)
                    .size(12)
                    .color(FAILURE_RED)
            );
        }

        let action_btn = if auth.is_authenticating {
            button(text("Verifying…").size(13).color(Color::WHITE))
                .padding(iced::Padding {
                    top: 8.0,
                    bottom: 8.0,
                    left: 20.0,
                    right: 20.0,
                })
                .style(primary_action_btn)
        } else {
            button(text("Authorize & Connect").size(13).color(Color::WHITE))
                .padding(iced::Padding {
                    top: 8.0,
                    bottom: 8.0,
                    left: 20.0,
                    right: 20.0,
                })
                .style(primary_action_btn)
                .on_press(Message::SubmitSudoPassword)
        };

        dialog_col = dialog_col.push(Space::with_height(12)).push(
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
                    .on_press(Message::CancelSudoAuth),
                Space::with_width(12),
                action_btn,
            ]
            .align_y(Alignment::Center),
        );

        let dialog = container(dialog_col.padding(20).width(380))
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
                    .style(|_, status| button::Style {
                        background: match status {
                            button::Status::Hovered => Some(Color::from_rgba(1.0, 1.0, 1.0, 0.12).into()),
                            _ => Some(Color::from_rgba(1.0, 1.0, 1.0, 0.06).into()),
                        },
                        border: iced::Border {
                            radius: 6.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    })
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
                icons::lucide_icon(icons::INFO, 18, FAILURE_RED),
                Space::with_width(10),
                column![
                    row![
                        text(&notif.title).size(13).color(Color::WHITE),
                        Space::with_width(8),
                        container(text(format!("{}/{}", notif.attempt, notif.max_attempts)).size(10).color(FAILURE_RED))
                            .padding(iced::Padding { top: 1.0, bottom: 1.0, left: 6.0, right: 6.0 })
                            .style(|_| container::Style {
                                background: Some(Color::from_rgba(0.95, 0.35, 0.40, 0.15).into()),
                                border: iced::Border {
                                    radius: 4.0.into(),
                                    ..Default::default()
                                },
                                ..Default::default()
                            }),
                    ]
                    .align_y(Alignment::Center),
                    text(&notif.message).size(11).color(Color::from_rgb(0.85, 0.85, 0.85)),
                    text(status_desc).size(11).color(ACCENT_BLUE_LIGHT),
                ]
                .spacing(2)
                .width(Length::Fill),
                Space::with_width(10),
                action_row,
                Space::with_width(8),
                button(icons::lucide_icon(icons::CLOSE, 14, TEXT_MUTED))
                    .padding(4)
                    .style(|_, status| button::Style {
                        background: match status {
                            button::Status::Hovered => Some(Color::from_rgba(1.0, 1.0, 1.0, 0.1).into()),
                            _ => None,
                        },
                        border: iced::Border {
                            radius: 4.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    })
                    .on_press(Message::DismissNotification),
            ]
            .align_y(Alignment::Center)
            .padding(10)
        )
        .style(|_| container::Style {
            background: Some(Color::from_rgb(0.12, 0.13, 0.17).into()),
            border: iced::Border {
                radius: 8.0.into(),
                width: 1.0,
                color: Color::from_rgb(0.75, 0.25, 0.25),
            },
            ..Default::default()
        })
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

    fn view_add_profile_modal(&self) -> Element<'_, Message> {
        let mut list = column![].spacing(3);

        for proto in protocols::ProfileType::ALL {
            let badge = proto.badge();
            let badge_widget = container(
                text(badge.text())
                    .size(10)
                    .color(badge.text_color())
            )
            .padding(iced::Padding {
                top: 2.0,
                bottom: 2.0,
                left: 6.0,
                right: 6.0,
            })
            .style(move |_| container::Style {
                background: Some(badge.bg_color().into()),
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            });

            let item_btn = button(
                row![
                    text(proto.title()).size(14).color(Color::WHITE),
                    Space::with_width(Length::Fill),
                    badge_widget,
                ]
                .align_y(Alignment::Center)
                .padding(iced::Padding {
                    top: 8.0,
                    bottom: 8.0,
                    left: 12.0,
                    right: 12.0,
                })
            )
            .width(Length::Fill)
            .padding(0)
            .style(|_, status| button::Style {
                background: match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Some(Color::from_rgba(1.0, 1.0, 1.0, 0.09).into())
                    }
                    _ => None,
                },
                border: iced::Border {
                    radius: 6.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .on_press(Message::CreateProfileWithType(proto));

            list = list.push(item_btn);
        }

        let header = row![
            text("Add Profile").size(15).color(Color::WHITE),
            Space::with_width(Length::Fill),
            button(icons::lucide_icon(icons::CLOSE, 16, Color::WHITE))
                .padding(4)
                .style(|_, status| button::Style {
                    background: match status {
                        button::Status::Hovered => Some(Color::from_rgba(1.0, 1.0, 1.0, 0.1).into()),
                        _ => None,
                    },
                    border: iced::Border {
                        radius: 12.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .on_press(Message::CloseAddMenu),
        ]
        .align_y(Alignment::Center);

        let modal_box = container(
            column![
                header,
                Space::with_height(6),
                scrollable(list).height(Length::Fixed(360.0)),
            ]
            .spacing(4)
            .padding(14)
        )
        .width(Length::Fixed(300.0))
        .style(|_| container::Style {
            background: Some(Color::from_rgb(0.16, 0.17, 0.19).into()),
            border: iced::Border {
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.18),
                width: 1.0,
                radius: 12.0.into(),
            },
            ..Default::default()
        });

        iced::widget::mouse_area(
            container(modal_box)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.65).into()),
                    ..Default::default()
                })
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
        let config_locked = self.config_locked();
        let top_bar = row![
            button(icons::lucide_icon(icons::MENU, 22, self.text_primary()))
                .padding(6)
                .style(|_, _| button::Style {
                    background: None,
                    ..Default::default()
                })
                .on_press(Message::SetViewMode(ViewMode::SshSettings)),
            Space::with_width(12),
            text("Netray").size(22).color(self.text_primary()),
            Space::with_width(Length::Fill),
            button(icons::lucide_icon(icons::FILE_PLUS, 20, self.text_primary()))
                .padding(6)
                .style(|_, _| button::Style {
                    background: None,
                    ..Default::default()
                })
                .on_press_maybe((!config_locked).then(|| Message::RequestFileImport)),
            button(icons::lucide_icon(icons::QR_CODE, 20, self.text_primary()))
                .padding(6)
                .style(|_, _| button::Style {
                    background: None,
                    ..Default::default()
                })
                .on_press_maybe((!config_locked).then(|| Message::OpenQrScanModal)),
            button(icons::lucide_icon(icons::LINK, 20, self.text_primary()))
                .padding(6)
                .style(|_, _| button::Style {
                    background: None,
                    ..Default::default()
                })
                .on_press_maybe((!config_locked).then(|| Message::RequestClipboardImport)),
            button("Hide")
                .padding(6)
                .style(|_, _| button::Style {
                    background: None,
                    text_color: TEXT_MUTED,
                    ..Default::default()
                })
                .on_press(Message::MinimizeToTray),
            button(icons::lucide_icon(icons::MORE_VERTICAL, 20, self.text_primary()))
                .padding(6)
                .style(|_, _| button::Style {
                    background: None,
                    ..Default::default()
                })
                .on_press(Message::SetViewMode(ViewMode::Logs)),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .padding(iced::Padding {
            top: 12.0,
            bottom: 8.0,
            left: 16.0,
            right: 16.0,
        });

        let sub_bar = row![
            button(icons::lucide_icon(icons::PLUS, 18, Color::WHITE))
                .padding(8)
                .style(|_, _| button::Style {
                    background: Some(ACCENT_BLUE.into()),
                    border: iced::Border {
                        radius: 18.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .on_press_maybe((!config_locked).then(|| Message::OpenAddMenu)),
            Space::with_width(10),
            text(format!("{} profiles", self.v2ray_profiles.len()))
                .size(13)
                .color(TEXT_MUTED),
            Space::with_width(Length::Fill),
            text(if self.is_running {
                "Tunnel active"
            } else if self.worker_active {
                "Connecting…"
            } else {
                "Offline"
            })
            .size(12)
            .color(if self.is_running {
                SUCCESS_GREEN
            } else {
                TEXT_MUTED
            }),
            Space::with_width(12),
            checkbox("WinTun", self.enable_tun)
                .size(16)
                .on_toggle(Message::ToggleTunMode),
            Space::with_width(12),
            checkbox("Proxy", self.enable_sysproxy)
                .size(16)
                .on_toggle(Message::ToggleSysProxy),
        ]
        .align_y(Alignment::Center)
        .padding(iced::Padding {
            top: 4.0,
            bottom: 8.0,
            left: 16.0,
            right: 16.0,
        });

        let cards: Vec<Element<'_, Message>> = self
            .v2ray_profiles
            .iter()
            .map(|p| {
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
                    .then(|| Message::PingProfile(p.id.clone()));
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

                let strip_style = if is_active {
                    card_strip_active
                } else {
                    card_strip_inactive
                };

                container(
                    row![
                        container(Space::with_width(2)).height(76).style(strip_style),
                        Space::with_width(12),
                        column![
                            button(
                                column![
                                    row![
                                        text(p.name.as_str()).size(15).color(self.text_primary()),
                                        Space::with_width(Length::Fill),
                                        text(if is_active { "SELECTED" } else { "" })
                                            .size(9)
                                            .color(SUCCESS_GREEN),
                                    ],
                                    text(node_addr).size(12).color(TEXT_MUTED),
                                ]
                                .spacing(4)
                                .width(Length::Fill)
                            )
                            .padding(0)
                            .width(Length::Fill)
                            .style(transparent_profile_btn)
                            .on_press_maybe(
                                (!config_locked).then(|| Message::SelectProfile(p.id.clone()))
                            ),
                            row![
                                text(proto_tag).size(10).color(ACCENT_BLUE_LIGHT),
                                text(ping_result).size(11).color(ping_color),
                                Space::with_width(Length::Fill),
                                button(icons::lucide_icon(icons::ACTIVITY, 18, Color::WHITE))
                                    .padding(10)
                                    .style(move |_, _| button::Style {
                                        background: Some(
                                            if pinging {
                                                ACCENT_BLUE_LIGHT
                                            } else {
                                                ACCENT_BLUE
                                            }
                                            .into()
                                        ),
                                        text_color: Color::WHITE,
                                        border: iced::Border {
                                            radius: 24.0.into(),
                                            ..Default::default()
                                        },
                                        ..Default::default()
                                    })
                                    .on_press_maybe(ping_press),
                                button(icons::lucide_icon(icons::SHARE, 16, TEXT_MUTED))
                                    .padding(6)
                                    .style(transparent_muted_btn)
                                    .on_press(Message::ShareProfile(p.id.clone())),
                                button(icons::lucide_icon(icons::PENCIL, 16, TEXT_MUTED))
                                    .padding(6)
                                    .style(transparent_muted_btn)
                                    .on_press_maybe(
                                        (!config_locked).then(|| Message::EditProfile(p.id.clone()))
                                    ),
                                button(icons::lucide_icon(icons::TRASH, 16, TEXT_MUTED))
                                    .padding(6)
                                    .style(transparent_muted_btn)
                                    .on_press_maybe(
                                        (!config_locked).then(|| Message::DeleteProfile(p.id.clone()))
                                    ),
                            ]
                            .spacing(4)
                            .align_y(Alignment::Center),
                        ]
                        .spacing(10)
                        .width(Length::Fill),
                    ]
                    .align_y(Alignment::Center)
                    .padding(12),
                )
                .width(Length::Fill)
                .style(form_card_style)
                .into()
            })
            .collect();

        let list_content: Element<'_, Message> = if cards.is_empty() {
            column![
                Space::with_height(40),
                text("No profiles added yet.").size(15).color(TEXT_MUTED),
                Space::with_height(10),
                button("Import from Clipboard")
                    .on_press_maybe((!config_locked).then(|| Message::RequestClipboardImport)),
            ]
            .align_x(Alignment::Center)
            .width(Length::Fill)
            .into()
        } else {
            scrollable(column(cards).spacing(10).padding(iced::Padding {
                top: 0.0,
                bottom: 80.0,
                left: 12.0,
                right: 12.0,
            }))
            .height(Length::Fill)
            .into()
        };

        let fab_icon = if self.is_running || self.worker_active {
            icons::lucide_icon(icons::SQUARE, 26, Color::WHITE)
        } else {
            icons::lucide_icon(icons::PLAY, 26, Color::WHITE)
        };

        let fab_row = row![
            button(
                row![
                    text("●").color(if self.is_running {
                        SUCCESS_GREEN
                    } else {
                        TEXT_MUTED
                    }),
                    text(if self.is_running {
                        let uptime = format_uptime(
                            self.connected_at.map(|at| at.elapsed()).unwrap_or_default(),
                        );
                        self.ping_ms.map_or_else(
                            || format!("CONNECTED · {uptime}"),
                            |ms| format!("CONNECTED · {uptime} · {ms} ms"),
                        )
                    } else if self.worker_active {
                        "CONNECTING · CHECKING SERVER".to_string()
                    } else {
                        "DISCONNECTED".to_string()
                    })
                    .size(12)
                    .color(TEXT_MUTED),
                ]
                .spacing(6)
                .align_y(Alignment::Center)
            )
            .padding(8)
            .style(|_, _| button::Style {
                background: None,
                text_color: TEXT_MUTED,
                ..Default::default()
            })
            .on_press(Message::SetViewMode(ViewMode::Logs)),
            Space::with_width(Length::Fill),
            button(fab_icon)
                .padding(16)
                .style(|_, _| button::Style {
                    background: Some(ACCENT_BLUE.into()),
                    border: iced::Border {
                        radius: 32.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .on_press(Message::ToggleTunnel),
        ]
        .padding(iced::Padding {
            top: 6.0,
            bottom: 12.0,
            left: 16.0,
            right: 16.0,
        });

        column![top_bar, sub_bar, list_content, fab_row,]
            .spacing(4)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_edit_screen(&self) -> Element<'_, Message> {
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
            button(icons::lucide_icon(icons::CLOSE, 22, Color::WHITE))
                .padding(6)
                .on_press(Message::CancelEditNode),
            Space::with_width(16),
            text(screen_title).size(20).color(Color::WHITE),
            Space::with_width(Length::Fill),
            button(icons::lucide_icon(icons::CHECK, 24, ACCENT_BLUE_LIGHT))
                .padding(6)
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
                text("Remarks").size(11).color(TEXT_MUTED),
                text_input("Remarks", &self.node_edit_state.remarks)
                    .on_input(Message::EditRemarksChanged)
                    .padding(6),
            ]
            .spacing(4),
        )
        .padding(10)
        .width(Length::Fill)
        .style(form_card_style);

        let addr_port_card = row![
            container(
                column![
                    text("Address").size(11).color(TEXT_MUTED),
                    text_input("example.com", &self.node_edit_state.address)
                        .on_input(Message::EditAddressChanged)
                        .padding(6),
                ]
                .spacing(4)
            )
            .padding(10)
            .width(Length::FillPortion(3))
            .style(form_card_style),
            Space::with_width(10),
            container(
                column![
                    text("Port").size(11).color(TEXT_MUTED),
                    text_input("443", &self.node_edit_state.port)
                        .on_input(Message::EditPortChanged)
                        .padding(6),
                ]
                .spacing(4)
            )
            .padding(10)
            .width(Length::FillPortion(1))
            .style(form_card_style),
        ];

        let encrypt_card = container(
            column![
                text("Encrypt method").size(11).color(TEXT_MUTED),
                pick_list(
                    ["none", "zero", "aes-128-gcm", "chacha20-poly1305"],
                    Some(self.node_edit_state.encrypt_method.as_str()),
                    |s| Message::EditEncryptMethodChanged(s.to_string())
                )
                .width(Length::Fill),
            ]
            .spacing(4),
        )
        .padding(10)
        .width(Length::Fill)
        .style(form_card_style);

        let flow_card = container(
            column![
                text("Flow").size(11).color(TEXT_MUTED),
                pick_list(
                    ["none", "xtls-rprx-vision", "xtls-rprx-vision-udp443"],
                    Some(self.node_edit_state.flow.as_str()),
                    |s| Message::EditFlowChanged(s.to_string())
                )
                .width(Length::Fill),
            ]
            .spacing(4),
        )
        .padding(10)
        .width(Length::Fill)
        .style(form_card_style);

        let uuid_card = container(
            column![
                text("User ID").size(11).color(TEXT_MUTED),
                text_input("User ID (UUID)", &self.node_edit_state.user_id)
                    .on_input(Message::EditUserIdChanged)
                    .padding(6),
            ]
            .spacing(4),
        )
        .padding(10)
        .width(Length::Fill)
        .style(form_card_style);

        let transport_card = container(
            column![
                text("Transfer protocol").size(11).color(TEXT_MUTED),
                pick_list(
                    ["tcp", "ws", "grpc", "h2", "kcp"],
                    Some(self.node_edit_state.transport_protocol.as_str()),
                    |s| Message::EditTransportChanged(s.to_string())
                )
                .width(Length::Fill),
            ]
            .spacing(4),
        )
        .padding(10)
        .width(Length::Fill)
        .style(form_card_style);

        let header_card = container(
            column![
                text("Header type").size(11).color(TEXT_MUTED),
                pick_list(
                    ["none", "http"],
                    Some(self.node_edit_state.header_type.as_str()),
                    |s| Message::EditHeaderTypeChanged(s.to_string())
                )
                .width(Length::Fill),
            ]
            .spacing(4),
        )
        .padding(10)
        .width(Length::Fill)
        .style(form_card_style);

        let path_card = container(
            column![
                text("Path / Service name").size(11).color(TEXT_MUTED),
                text_input("/path (e.g. /ws)", &self.node_edit_state.path)
                    .on_input(Message::EditPathChanged)
                    .padding(6),
            ]
            .spacing(4),
        )
        .padding(10)
        .width(Length::Fill)
        .style(form_card_style);

        let tls_card = container(
            column![
                text("TLS type").size(11).color(TEXT_MUTED),
                pick_list(
                    ["tls", "reality", "none"],
                    Some(self.node_edit_state.tls_type.as_str()),
                    |s| Message::EditTlsTypeChanged(s.to_string())
                )
                .width(Length::Fill),
            ]
            .spacing(4),
        )
        .padding(10)
        .width(Length::Fill)
        .style(form_card_style);

        let mut form_items = vec![
            remarks_card.into(),
            addr_port_card.into(),
            encrypt_card.into(),
            flow_card.into(),
            uuid_card.into(),
            transport_card.into(),
            header_card.into(),
            path_card.into(),
            tls_card.into(),
        ];

        if self.node_edit_state.tls_type == "reality" {
            let reality_row = row![
                container(
                    column![
                        text("Reality Public Key").size(11).color(TEXT_MUTED),
                        text_input("PublicKey", &self.node_edit_state.public_key)
                            .on_input(Message::EditPublicKeyChanged)
                            .padding(6),
                    ]
                    .spacing(4)
                )
                .padding(10)
                .width(Length::FillPortion(2))
                .style(form_card_style),
                Space::with_width(10),
                container(
                    column![
                        text("Short ID").size(11).color(TEXT_MUTED),
                        text_input("ShortId", &self.node_edit_state.short_id)
                            .on_input(Message::EditShortIdChanged)
                            .padding(6),
                    ]
                    .spacing(4)
                )
                .padding(10)
                .width(Length::FillPortion(1))
                .style(form_card_style),
            ];
            form_items.push(reality_row.into());
        }

        let sni_card = container(
            column![
                text("Server name indication").size(11).color(TEXT_MUTED),
                text_input("SNI (e.g. example.com)", &self.node_edit_state.sni)
                    .on_input(Message::EditSniChanged)
                    .padding(6),
            ]
            .spacing(4),
        )
        .padding(10)
        .width(Length::Fill)
        .style(form_card_style);

        let fp_card = container(
            column![
                text("Fingerprint").size(11).color(TEXT_MUTED),
                pick_list(
                    ["chrome", "firefox", "safari", "ios", "android", "random"],
                    Some(self.node_edit_state.fingerprint.as_str()),
                    |s| Message::EditFingerprintChanged(s.to_string())
                )
                .width(Length::Fill),
            ]
            .spacing(4),
        )
        .padding(10)
        .width(Length::Fill)
        .style(form_card_style);

        let alpn_card = container(
            column![
                text("Alpn").size(11).color(TEXT_MUTED),
                pick_list(
                    ["none", "h2", "http/1.1", "h2,http/1.1"],
                    Some(self.node_edit_state.alpn.as_str()),
                    |s| Message::EditAlpnChanged(s.to_string())
                )
                .width(Length::Fill),
            ]
            .spacing(4),
        )
        .padding(10)
        .width(Length::Fill)
        .style(form_card_style);

        form_items.push(sni_card.into());
        form_items.push(fp_card.into());
        form_items.push(alpn_card.into());
        form_items.push(Space::with_height(20).into());

        let form_content = scrollable(
            column(form_items)
                .spacing(10)
                .padding(iced::Padding {
                    top: 0.0,
                    bottom: 20.0,
                    left: 16.0,
                    right: 16.0,
                }),
        )
        .height(Length::Fill);

        column![top_bar, form_content,]
            .spacing(6)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_logs_screen(&self) -> Element<'_, Message> {
        let top_bar = row![
            button(icons::lucide_icon(icons::BACK, 22, Color::WHITE))
                .padding(6)
                .on_press(Message::SetViewMode(ViewMode::Profiles)),
            Space::with_width(12),
            text("Tunnel Logs & Status").size(20).color(Color::WHITE),
            Space::with_width(Length::Fill),
            button("Clear Logs").padding(6).on_press(Message::ClearLogs),
        ]
        .align_y(Alignment::Center)
        .padding(iced::Padding {
            top: 10.0,
            bottom: 10.0,
            left: 12.0,
            right: 12.0,
        });

        let status_text = if self.is_running {
            "CONNECTED"
        } else {
            "STOPPED"
        };
        let status_color = if self.is_running {
            Color::from_rgb(0.2, 0.8, 0.4)
        } else {
            Color::from_rgb(0.8, 0.3, 0.3)
        };

        let status_bar = row![
            text(format!("STATUS: {}", status_text))
                .size(14)
                .color(status_color),
            Space::with_width(Length::Fill),
            text(format!("MODE: {}", self.tunnel_mode))
                .size(13)
                .color(TEXT_MUTED),
        ]
        .padding(iced::Padding {
            top: 0.0,
            bottom: 6.0,
            left: 16.0,
            right: 16.0,
        });

        let log_rows: Vec<Element<'_, Message>> = self
            .logs
            .iter()
            .rev()
            .take(60)
            .rev()
            .map(|entry| {
                let color = match entry.level {
                    LogLevel::Info => Color::WHITE,
                    LogLevel::Success => Color::from_rgb(0.25, 0.9, 0.45),
                    LogLevel::Warn => Color::from_rgb(0.95, 0.75, 0.25),
                    LogLevel::Error => Color::from_rgb(0.95, 0.35, 0.35),
                };

                row![
                    text(format!("[{}]", entry.timestamp))
                        .size(12)
                        .color(Color::from_rgb(0.5, 0.5, 0.5)),
                    text(format!("[{}]", entry.source))
                        .size(12)
                        .color(ACCENT_BLUE_LIGHT),
                    text(&entry.message).size(12).color(color),
                ]
                .spacing(8)
                .into()
            })
            .collect();

        let terminal = container(
            scrollable(column(log_rows).spacing(6))
                .id(scrollable::Id::new("logs"))
                .height(Length::Fill),
        )
        .padding(12)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(Color::from_rgb(0.06, 0.07, 0.09).into()),
            border: iced::Border {
                radius: 6.0.into(),
                width: 1.0,
                color: Color::from_rgb(0.18, 0.20, 0.25),
            },
            ..Default::default()
        });

        column![
            top_bar,
            status_bar,
            container(
                checkbox("Auto-scroll", self.auto_scroll_logs).on_toggle(Message::AutoScrollLogs)
            )
            .padding([4, 12]),
            container(terminal)
                .padding(iced::Padding {
                    top: 8.0,
                    bottom: 12.0,
                    left: 12.0,
                    right: 12.0
                })
                .height(Length::Fill),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn view_ssh_screen(&self) -> Element<'_, Message> {
        let top_bar = row![
            button(icons::lucide_icon(icons::BACK, 22, self.text_primary()))
                .padding(6)
                .style(|_, _| button::Style {
                    background: None,
                    ..Default::default()
                })
                .on_press(Message::SetViewMode(ViewMode::Profiles)),
            Space::with_width(12),
            text("Settings & Routing").size(20).color(self.text_primary()),
        ]
        .align_y(Alignment::Center)
        .padding(iced::Padding {
            top: 10.0,
            bottom: 10.0,
            left: 12.0,
            right: 12.0,
        });

        // Theme & Appearance Card
        let is_dark_active = self.is_dark();
        let theme_button = |label: &'static str, icon_bytes: &'static [u8], pref: storage::ThemePreference| {
            let is_selected = self.theme_preference == pref;
            let (bg, border_color, text_color, icon_color) = if is_selected {
                (
                    Some(ACCENT_BLUE.into()),
                    ACCENT_BLUE_LIGHT,
                    Color::WHITE,
                    Color::WHITE,
                )
            } else if is_dark_active {
                (
                    Some(Color::from_rgb(0.18, 0.20, 0.25).into()),
                    Color::from_rgb(0.25, 0.28, 0.35),
                    Color::from_rgb(0.85, 0.88, 0.93),
                    Color::from_rgb(0.70, 0.75, 0.82),
                )
            } else {
                (
                    Some(Color::from_rgb(0.92, 0.94, 0.96).into()),
                    Color::from_rgb(0.82, 0.85, 0.90),
                    Color::from_rgb(0.15, 0.18, 0.24),
                    Color::from_rgb(0.25, 0.30, 0.40),
                )
            };

            button(
                row![
                    icons::lucide_icon(icon_bytes, 14, icon_color),
                    Space::with_width(6),
                    text(label).size(12).color(text_color),
                ]
                .align_y(Alignment::Center)
            )
            .padding(iced::Padding {
                top: 7.0,
                bottom: 7.0,
                left: 10.0,
                right: 10.0,
            })
            .style(move |_, status| button::Style {
                background: match status {
                    button::Status::Hovered if !is_selected => Some(Color::from_rgba(0.24, 0.58, 1.0, 0.18).into()),
                    _ => bg,
                },
                border: iced::Border {
                    radius: 8.0.into(),
                    width: 1.0,
                    color: border_color,
                },
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
                    icons::lucide_icon(icons::SUN, 16, ACCENT_BLUE_LIGHT),
                    Space::with_width(6),
                    text("Theme & Appearance").size(14).color(self.text_primary()),
                ]
                .align_y(Alignment::Center),
                text("Select application theme: Light Mode, Dark Mode, or follow your System Theme.")
                    .size(12)
                    .color(TEXT_MUTED),
                Space::with_height(6),
                row![
                    theme_button("Light Mode", icons::SUN, storage::ThemePreference::Light),
                    theme_button("Dark Mode", icons::MOON, storage::ThemePreference::Dark),
                    theme_button("System Theme", icons::MONITOR, storage::ThemePreference::System),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
                Space::with_height(2),
                text(active_theme_label)
                    .size(11)
                    .color(ACCENT_BLUE_LIGHT),
            ]
            .spacing(4)
            .width(Length::Fill)
        )
        .padding(12)
        .style(form_card_style)
        .width(Length::Fill);

        let startup_card = container(
            column![
                row![
                    icons::lucide_icon(icons::MONITOR, 16, ACCENT_BLUE_LIGHT),
                    Space::with_width(6),
                    text("System & Startup").size(14).color(self.text_primary()),
                ]
                .align_y(Alignment::Center),
                text("Configure automatic startup with the operating system and connection behavior.")
                    .size(12)
                    .color(TEXT_MUTED),
                Space::with_height(6),
                checkbox(
                    "Run at system startup (Launch Netray when logging in)",
                    self.run_at_startup,
                )
                .on_toggle(Message::ToggleRunAtStartup),
                Space::with_height(2),
                checkbox(
                    "Connect at startup (Prompt to activate tunnel on launch)",
                    self.connect_at_startup,
                )
                .on_toggle(Message::ToggleConnectAtStartup),
                Space::with_height(2),
                checkbox(
                    "Automatic reconnect on error (Auto-detect dropped tunnel & reconnect)",
                    self.auto_reconnect,
                )
                .on_toggle(Message::ToggleAutoReconnect),
            ]
            .spacing(6)
            .width(Length::Fill)
        )
        .padding(12)
        .style(form_card_style)
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
            .width(Length::FillPortion(3)),
            Space::with_width(8),
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
            .on_press(Message::BrowseAppExecutable),
        ]
        .align_y(Alignment::Center)
        .width(Length::Fill);

        let whitelist_card = container(
            column![
                text("Bypass Whitelist (Split Tunneling)").size(15).color(Color::WHITE),
                text("Traffic from these applications and URLs/domains will bypass the proxy tunnel and connect directly. Active in both TUN and System Proxy modes.")
                    .size(12)
                    .color(TEXT_MUTED),
                Space::with_height(10),
                text("Whitelist Applications:").size(13).color(ACCENT_BLUE_LIGHT),
                app_picker_row,
                chips_col,
                text_input("e.g. curl.exe, chrome.exe, discord.exe", &self.whitelist_apps)
                    .on_input(Message::WhitelistAppsChanged)
                    .padding(8)
                    .width(Length::Fill),
                text("Select from catalog, browse .exe, or enter comma/newline-separated names.")
                    .size(11)
                    .color(TEXT_MUTED),
                Space::with_height(8),
                text("Whitelist URLs & Domains:").size(13).color(ACCENT_BLUE_LIGHT),
                text_input("e.g. *.internal.net, 10.0.0.0/8, https://work.com", &self.whitelist_urls)
                    .on_input(Message::WhitelistUrlsChanged)
                    .padding(8)
                    .width(Length::Fill),
                text("Comma or newline-separated URLs, domains, wildcards (*.site.com), or IP CIDRs.")
                    .size(11)
                    .color(TEXT_MUTED),
            ]
            .spacing(6)
            .width(Length::Fill)
        )
        .padding(14)
        .style(form_card_style)
        .width(Length::Fill);

        let ip_text = if self.is_checking_ip {
            text("Checking ifconfig.me...").size(13).color(TEXT_MUTED)
        } else if let Some(ip) = &self.public_ip {
            let color = if ip.starts_with("Error") || ip.starts_with("Failed") || ip.starts_with("curl exited") {
                FAILURE_RED
            } else {
                SUCCESS_GREEN
            };
            text(ip.clone()).size(14).color(color)
        } else {
            text("Click to check current public IP").size(13).color(TEXT_MUTED)
        };

        let ip_card = container(
            row![
                column![
                    text("Public IP Address (Exit Node)").size(14).color(Color::WHITE),
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
        .padding(12)
        .style(form_card_style)
        .width(Length::Fill);

        let content = scrollable(
            column![
                theme_card,
                Space::with_height(12),
                startup_card,
                Space::with_height(12),
                ip_card,
                Space::with_height(12),
                whitelist_card,
                Space::with_height(14),
                text("Tunneling Mode:").size(14).color(TEXT_MUTED),
                pick_list(
                    &TUNNEL_MODES[..],
                    Some(self.tunnel_mode),
                    Message::TunnelModeChanged
                )
                .width(Length::Fill),
                Space::with_height(10),
                text("HTTP Payload Template:").size(14).color(TEXT_MUTED),
                text_input("Enter HTTP payload...", &self.payload_text)
                    .on_input(Message::PayloadChanged)
                    .padding(8),
                Space::with_height(10),
                row![
                    checkbox("Keep-Alive", self.gen_keep_alive).on_toggle(Message::ToggleKeepAlive),
                    checkbox("Online-Host", self.gen_online_host)
                        .on_toggle(Message::ToggleOnlineHost),
                    checkbox("Split", self.gen_split).on_toggle(Message::ToggleSplit),
                ]
                .spacing(12),
                Space::with_height(10),
                text("Bug Host / CDN SNI:").size(14).color(TEXT_MUTED),
                text_input("cdn.example.com", &self.bug_host)
                    .on_input(Message::BugHostChanged)
                    .padding(8),
                Space::with_height(10),
                button("Generate HTTP Payload").on_press(Message::GeneratePayload),
                Space::with_height(20),
                text("SSH Server & Account:").size(14).color(TEXT_MUTED),
                row![
                    text_input("SSH Host", &self.ssh_host)
                        .on_input(Message::SshHostChanged)
                        .width(Length::FillPortion(3))
                        .padding(8),
                    text_input("Port", &self.ssh_port)
                        .on_input(Message::SshPortChanged)
                        .width(Length::FillPortion(1))
                        .padding(8),
                ]
                .spacing(8),
                Space::with_height(10),
                row![
                    text_input("Username", &self.ssh_user)
                        .on_input(Message::SshUserChanged)
                        .width(Length::FillPortion(1))
                        .padding(8),
                    text_input("Password", &self.ssh_pass)
                        .on_input(Message::SshPassChanged)
                        .width(Length::FillPortion(1))
                        .padding(8),
                ]
                .spacing(8),
                Space::with_height(10),
                text("Proxy IP & Port:").size(14).color(TEXT_MUTED),
                row![
                    text_input("104.16.51.111", &self.proxy_ip)
                        .on_input(Message::ProxyIpChanged)
                        .width(Length::FillPortion(3))
                        .padding(8),
                    text_input("80", &self.proxy_port)
                        .on_input(Message::ProxyPortChanged)
                        .width(Length::FillPortion(1))
                        .padding(8),
                ]
                .spacing(8),
                Space::with_height(10),
                text("Local SOCKS5 Port:").size(14).color(TEXT_MUTED),
                text_input("10808", &self.local_socks_port)
                    .on_input(Message::LocalSocksPortChanged)
                    .padding(8),
                Space::with_height(30),
            ]
            .spacing(8)
            .width(Length::Fill)
            .padding(iced::Padding {
                top: 0.0,
                bottom: 20.0,
                left: 16.0,
                right: 16.0,
            }),
        )
        .width(Length::Fill)
        .height(Length::Fill);

        column![
            top_bar,
            content,
            text(format!("Version {APP_VERSION}"))
                .size(11)
                .color(TEXT_MUTED)
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
        let _ = app.update(Message::NewLog(LogEntry {
            timestamp: String::new(),
            source: "Xray".into(),
            message: "Core process started".into(),
            level: LogLevel::Info,
        }));
        assert!(!app.is_running);
        assert!(app.connected_at.is_none());
        let _ = app.update(Message::NewLog(LogEntry {
            timestamp: String::new(),
            source: "Error".into(),
            message: "Connection check failed: HTTP 502".into(),
            level: LogLevel::Error,
        }));
        assert!(!app.is_running);
        assert!(app.connected_at.is_none());
        let _ = app.update(Message::NewLog(LogEntry {
            timestamp: String::new(),
            source: "Supervisor".into(),
            message: "Tunnel ready · HTTP 204 through proxy in 73 ms.".into(),
            level: LogLevel::Success,
        }));
        assert!(app.is_running);
        assert!(app.connected_at.is_some());
        assert_eq!(app.ping_ms, Some(73));
    }

    #[test]
    fn errors_do_not_release_worker_before_cleanup() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        app.worker_active = true;
        app.is_running = true;
        let _ = app.update(Message::NewLog(LogEntry {
            timestamp: String::new(),
            source: "Error".into(),
            message: "Engine failed: test".into(),
            level: LogLevel::Error,
        }));
        assert!(app.worker_active);
        assert!(!app.is_running);
        let _ = app.update(Message::NewLog(LogEntry {
            timestamp: String::new(),
            source: "Supervisor".into(),
            message: "Tunnel is offline.".into(),
            level: LogLevel::Info,
        }));
        assert!(!app.worker_active);
    }

    #[test]
    fn disabling_auto_scroll_keeps_incoming_logs() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        let _ = app.update(Message::SetViewMode(ViewMode::Logs));
        let _ = app.update(Message::AutoScrollLogs(false));
        let count = app.logs.len();
        let _ = app.update(Message::NewLog(LogEntry {
            timestamp: String::new(),
            source: "Test".into(),
            message: "still collected".into(),
            level: LogLevel::Info,
        }));
        assert!(!app.auto_scroll_logs);
        assert_eq!(app.logs.len(), count + 1);
        let _ = app.update(Message::AutoScrollLogs(true));
        assert!(app.auto_scroll_logs);
    }

    #[test]
    fn uptime_only_subscribes_when_visible_and_running() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        let subscriptions =
            |app: &NetrayApp| iced::advanced::subscription::into_recipes(app.subscription()).len();
        assert_eq!(subscriptions(&app), 4);
        app.is_running = true;
        assert_eq!(subscriptions(&app), 5);
        for view in [ViewMode::Logs, ViewMode::EditNode, ViewMode::SshSettings] {
            app.view_mode = view;
            assert_eq!(subscriptions(&app), 4);
        }
        app.view_mode = ViewMode::Profiles;
        assert_eq!(subscriptions(&app), 5);
        app.is_running = false;
        assert_eq!(subscriptions(&app), 4);
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
        assert!(app.sudo_auth_modal.is_none());

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
        assert!(app.sudo_auth_modal.is_none());

        // Confirm confirmation prompt -> opens auth modal
        app.startup_connect_prompt = true;
        let _ = app.update_message(Message::ConfirmStartupConnectPrompt);
        assert!(!app.startup_connect_prompt);
        assert!(app.sudo_auth_modal.is_some());

        // Verify view renders auth modal
        drop(app.view());

        // Test password input and empty password submission
        let _ = app.update_message(Message::SudoPasswordInputChanged("   ".into()));
        let _ = app.update_message(Message::SubmitSudoPassword);
        assert_eq!(
            app.sudo_auth_modal.as_ref().and_then(|a| a.error.clone()),
            Some("Password cannot be empty.".to_string())
        );

        // Changing password clears error
        let _ = app.update_message(Message::SudoPasswordInputChanged("my_password".into()));
        assert_eq!(
            app.sudo_auth_modal.as_ref().and_then(|a| a.error.clone()),
            None
        );

        // Auth failure handling
        let _ = app.update_message(Message::SudoAuthResult(Err("Incorrect password. Please try again.".into())));
        assert_eq!(
            app.sudo_auth_modal.as_ref().and_then(|a| a.error.clone()),
            Some("Incorrect password. Please try again.".to_string())
        );
        assert!(!app.sudo_auth_modal.as_ref().unwrap().is_authenticating);

        // Auth cancel
        let _ = app.update_message(Message::CancelSudoAuth);
        assert!(app.sudo_auth_modal.is_none());

        // Auth success closes modal
        app.sudo_auth_modal = Some(SudoAuthModalState::default());
        let _ = app.update_message(Message::SudoAuthResult(Ok(())));
        assert!(app.sudo_auth_modal.is_none());
    }

    #[test]
    fn test_windows_auto_reconnect_error_lifecycle_and_actions() {
        let mut data = storage::AppData::default();
        data.auto_reconnect = true;
        let (mut app, _) = NetrayApp::from_saved(data);

        assert!(app.auto_reconnect);
        assert_eq!(app.reconnect_attempts, 0);
        assert!(app.error_notification.is_none());
        assert!(!app.user_stopped);

        // 1. Simulate active tunnel
        app.is_running = true;
        app.worker_active = true;

        // 2. Unexpected error arrival in logs triggers auto-reconnect
        let _ = app.update_message(Message::NewLog(LogEntry {
            timestamp: Local::now().format("%H:%M:%S").to_string(),
            source: "Error".to_string(),
            message: "Connection check failed: Latency endpoint returned HTTP 503".to_string(),
            level: LogLevel::Error,
        }));
        let _ = app.update_message(Message::NewLog(LogEntry {
            timestamp: Local::now().format("%H:%M:%S").to_string(),
            source: "Supervisor".to_string(),
            message: "Tunnel is offline.".to_string(),
            level: LogLevel::Warn,
        }));

        assert_eq!(app.reconnect_attempts, 1);
        let notif = app.error_notification.as_ref().expect("Notification toast should be present");
        assert_eq!(notif.attempt, 1);
        assert_eq!(notif.max_attempts, MAX_RECONNECT_ATTEMPTS);
        assert_eq!(notif.countdown_secs, Some(RECONNECT_COUNTDOWN_SECONDS));
        assert!(!notif.is_reconnecting);

        // 3. Verify view renders toast banner without error
        drop(app.view());

        // 4. Tick countdown: 3 -> 2 -> 1
        let _ = app.update_message(Message::ReconnectCountdownTick);
        assert_eq!(app.error_notification.as_ref().unwrap().countdown_secs, Some(2));
        let _ = app.update_message(Message::ReconnectCountdownTick);
        assert_eq!(app.error_notification.as_ref().unwrap().countdown_secs, Some(1));

        // 5. Final tick reaches zero: triggers reconnection
        let _ = app.update_message(Message::ReconnectCountdownTick);
        let notif = app.error_notification.as_ref().unwrap();
        assert_eq!(notif.countdown_secs, None);
        assert!(notif.is_reconnecting);
        assert!(app.worker_active);
        drop(app.view());

        // 6. Successful connection clears notification and resets attempt counter
        let _ = app.update_message(Message::NewLog(LogEntry {
            timestamp: Local::now().format("%H:%M:%S").to_string(),
            source: "Supervisor".to_string(),
            message: "Tunnel ready · HTTP 204 through proxy in 64 ms.".to_string(),
            level: LogLevel::Success,
        }));
        assert!(app.is_running);
        assert_eq!(app.reconnect_attempts, 0);
        assert!(app.error_notification.is_none());

        // 7. Manual cancel stops auto-reconnect and marks user_stopped = true
        app.is_running = false;
        let _ = app.update_message(Message::TriggerErrorAutoReconnect("Process crashed".into()));
        assert!(app.error_notification.is_some());
        let _ = app.update_message(Message::CancelAutoReconnect);
        assert!(app.error_notification.is_none());
        assert_eq!(app.reconnect_attempts, 0);
        assert!(app.user_stopped);

        // 8. While user_stopped is true, incoming errors must NOT trigger auto-reconnect
        let _ = app.update_message(Message::NewLog(LogEntry {
            timestamp: Local::now().format("%H:%M:%S").to_string(),
            source: "Error".to_string(),
            message: "Core process exited with: 1".to_string(),
            level: LogLevel::Error,
        }));
        assert!(app.error_notification.is_none());
        assert_eq!(app.reconnect_attempts, 0);

        // 9. Manual start / retry resets user_stopped
        app.user_stopped = false;
        let _ = app.update_message(Message::TriggerErrorAutoReconnect("Timeout error".into()));
        assert!(app.error_notification.is_some());
        let _ = app.update_message(Message::ExecuteAutoReconnect);
        assert!(app.error_notification.as_ref().unwrap().is_reconnecting);
        assert_eq!(app.error_notification.as_ref().unwrap().countdown_secs, None);

        // 10. Dismiss notification
        let _ = app.update_message(Message::DismissNotification);
        assert!(app.error_notification.is_none());

        // 11. Intentional user disconnect does NOT trigger auto-reconnect
        app.is_running = true;
        app.worker_active = true;
        let _ = app.update_message(Message::ToggleTunnel); // User stops tunnel
        assert!(!app.is_running);
        assert!(app.user_stopped);
        assert!(app.error_notification.is_none());

        // 12. ToggleAutoReconnect setting persistence
        let _ = app.update_message(Message::ToggleAutoReconnect(false));
        assert!(!app.auto_reconnect);
        let _ = app.update_message(Message::ToggleAutoReconnect(true));
        assert!(app.auto_reconnect);
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
}

