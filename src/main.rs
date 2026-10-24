pub mod apps;
pub mod icons;
pub mod protocols;
pub mod qr;
pub mod storage;
pub mod sysproxy;
pub mod theme;
mod tunnel;
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

// ============================================================================
// COLOR PALETTE & NETRAY BLUE ACCENT THEME
// ============================================================================
const ACCENT_BLUE: Color = Color::from_rgb(0.15, 0.48, 0.98); // #2563EB
const ACCENT_BLUE_LIGHT: Color = Color::from_rgb(0.24, 0.58, 1.0); // #3B82F6
const SURFACE_CARD: Color = Color::from_rgb(0.14, 0.16, 0.20);
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

pub fn main() -> iced::Result {
    // shortcut: Iced 0.13's software renderer maps a helper window on Wayland; remove when upstream fixes it.
    #[cfg(target_os = "linux")]
    if std::env::var_os("DISPLAY").is_some_and(|display| !display.is_empty()) {
        std::env::remove_var("WAYLAND_DISPLAY");
        std::env::remove_var("WAYLAND_SOCKET");
    }
    iced::daemon("Netray", NetrayApp::update, NetrayApp::view)
        .subscription(NetrayApp::subscription)
        .theme(|app, _| app.theme())
        .run_with(NetrayApp::new)
}

fn window_settings() -> iced::window::Settings {
    let mut window = iced::window::Settings {
        size: (460.0, 600.0).into(),
        icon: create_window_icon(),
        decorations: false,
        exit_on_close_request: false,
        ..Default::default()
    };
    #[cfg(target_os = "linux")]
    {
        window.platform_specific.application_id = "netray".into();
    }
    window
}

fn load_logo_pixels() -> Result<(Vec<u8>, u32, u32), Box<dyn std::error::Error>> {
    let decoder = png::Decoder::new(std::io::Cursor::new(include_bytes!(
        "../packaging/netray-tray.png"
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

// ============================================================================
// 1. APPLICATION STATE
// ============================================================================

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
            TunnelMode::V2RayXray => write!(f, "V2Ray / Xray Core"),
            TunnelMode::SlowDns => write!(f, "DNSTT (SlowDNS)"),
        }
    }
}

#[derive(Debug, Clone)]
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

pub struct NetrayApp {
    pub view_mode: ViewMode,
    pub is_running: bool,
    pub connected_at: Option<Instant>,
    pub ping_ms: Option<u128>,
    pub tunnel_mode: TunnelMode,

    // Payload & SNI State
    pub payload_text: String,
    pub bug_host: String,
    pub sni_host: String,
    pub gen_keep_alive: bool,
    pub gen_online_host: bool,
    pub gen_split: bool,

    // SSH & Proxy State
    pub ssh_host: String,
    pub ssh_port: String,
    pub ssh_user: String,
    pub ssh_pass: String,
    pub proxy_ip: String,
    pub proxy_port: String,
    pub local_socks_port: String,

    // V2Ray State & Profiles
    pub v2ray_uri_or_json: String,
    pub v2ray_profiles: Vec<storage::V2RayProfile>,
    pub active_profile_id: Option<String>,
    pub node_edit_state: v2ray::NodeEditState,
    pub ping_in_progress: Option<String>,
    pub ping_results: HashMap<String, String>,
    pub show_add_menu: bool,
    pub instant_import: bool,
    pub clipboard_import_candidate: Option<String>,
    pub share_modal: Option<ShareModalState>,

    // TUN & System Routing Mode
    pub enable_tun: bool,
    pub enable_sysproxy: bool,
    pub whitelist_apps: String,
    pub whitelist_urls: String,
    pub available_apps: Vec<apps::AppPreset>,
    pub selected_app_preset: Option<apps::AppPreset>,
    pub public_ip: Option<String>,
    pub is_checking_ip: bool,
    pub theme_preference: storage::ThemePreference,

    // Real-time Logs & Background Worker Control
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

// ============================================================================
// 2. MESSAGES (EVENTS)
// ============================================================================

#[derive(Debug, Clone)]
pub enum Message {
    SetViewMode(ViewMode),
    TunnelModeChanged(TunnelMode),
    ToggleTunnel,
    MinimizeToTray,

    // Node & Profile management
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

    // Node Editor Form fields (Screenshots 2 & 3)
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
    SaveEditedNode,
    CancelEditNode,

    // Payload & SSH Inputs
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

    // V2Ray & Network Controls
    V2RayConfigChanged(String),
    LocalSocksPortChanged(String),
    ValidateV2RayConfig,
    LoadSampleV2Ray,
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

    // Real-time Logger Events
    LoggerReady(mpsc::Sender<LogEntry>),
    NewLog(LogEntry),
    UptimeTick,
    ClearLogs,
    AutoScrollLogs(bool),
    CloseRequested(iced::window::Id),
    WindowOpened(iced::window::Id),
    ShowWindow,
    DragWindow,
    MaximizeWindow,
    RequestExit,
    ConfirmExit,
    CancelExit,
}

impl NetrayApp {
    fn new() -> (Self, Task<Message>) {
        let (mut app, _) = Self::from_saved(storage::load_data());
        let task = app.show_window();
        (app, task)
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
            instant_import: saved.instant_import,
            clipboard_import_candidate: None,
            share_modal: None,

            enable_tun: saved.enable_tun,
            enable_sysproxy: saved.enable_sysproxy && !saved.enable_tun,
            whitelist_apps: saved.whitelist_apps,
            whitelist_urls: saved.whitelist_urls,
            available_apps: apps::discover_installed_linux_apps(),
            selected_app_preset: None,
            public_ip: None,
            is_checking_ip: false,
            theme_preference: saved.theme_preference,

            logs: vec![LogEntry {
                timestamp: Local::now().format("%H:%M:%S").to_string(),
                source: "Netray".into(),
                message: "Ready to connect.".into(),
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

    fn show_window(&mut self) -> Task<Message> {
        self.hidden_to_tray = false;
        if let Some(id) = self.window_id {
            iced::window::minimize(id, false).chain(iced::window::gain_focus(id))
        } else {
            let (id, task) = iced::window::open(window_settings());
            self.window_id = Some(id);
            task.map(Message::WindowOpened)
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

    // ========================================================================
    // 3. STATE UPDATE & TUNNEL SUPERVISOR
    // ========================================================================

    fn update(&mut self, message: Message) -> Task<Message> {
        let scroll = matches!(
            message,
            Message::NewLog(_)
                | Message::PingFinished(..)
                | Message::SetViewMode(ViewMode::Logs)
                | Message::AutoScrollLogs(true)
        );
        let previous = (self.is_running, self.worker_active);
        let opened = matches!(message, Message::WindowOpened(_));
        let task = self.update_message(message);
        if opened || previous != (self.is_running, self.worker_active) {
            if let Some(item) = &self.tray_connect_item {
                item.set_text(if self.is_running {
                    "Disconnect"
                } else {
                    "Connect"
                });
                item.set_enabled(self.is_running || !self.worker_active);
            }
        }
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
                    | Message::RequestClipboardImport
                    | Message::ClipboardImported(_)
                    | Message::ConfirmClipboardImport
                    | Message::ClipboardCandidateChanged(_)
                    | Message::LoadSampleV2Ray
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
                self.node_edit_state =
                    v2ray::NodeEditState::from_profile(&id, "New VLess Node", "");
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
                let profile = storage::V2RayProfile {
                    id: id.clone(),
                    name,
                    config: config.clone(),
                    created_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                };
                self.push_log(
                    "Netray",
                    format!("Added new {} profile '{}'.", proto.title(), profile.name),
                    LogLevel::Info,
                );
                self.active_profile_id = Some(id);
                self.v2ray_uri_or_json = config;
                self.node_edit_state =
                    v2ray::NodeEditState::from_profile(&profile.id, &profile.name, &profile.config);
                self.v2ray_profiles.push(profile);
                let _ = self.save_current_state();
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
            Message::CancelEditNode => {
                self.view_mode = ViewMode::Profiles;
                Task::none()
            }
            Message::SaveEditedNode => {
                let uri = self.node_edit_state.to_vless_uri();
                let id = self.node_edit_state.id.clone();
                self.ping_results.remove(&id);
                if self.ping_in_progress.as_deref() == Some(&id) {
                    self.ping_in_progress = None;
                }
                let name = if self.node_edit_state.remarks.is_empty() {
                    v2ray::extract_node_name(&uri)
                } else {
                    self.node_edit_state.remarks.clone()
                };

                if let Some(existing) = self.v2ray_profiles.iter_mut().find(|p| p.id == id) {
                    existing.name = name.clone();
                    existing.config = uri.clone();
                } else {
                    let profile = storage::V2RayProfile {
                        id: id.clone(),
                        name: name.clone(),
                        config: uri.clone(),
                        created_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                    };
                    self.v2ray_profiles.insert(0, profile);
                }

                self.active_profile_id = Some(id);
                self.v2ray_uri_or_json = uri;
                let _ = self.save_current_state();
                self.push_log(
                    "Config",
                    format!("Saved node '{}'.", name),
                    LogLevel::Success,
                );
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
            Message::ValidateV2RayConfig => {
                let socks_p: u16 = self.local_socks_port.parse().unwrap_or(10808);
                let http_p = socks_p.saturating_add(1);
                match v2ray::parse_and_generate_xray_config(
                    &self.v2ray_uri_or_json,
                    socks_p,
                    http_p,
                    self.enable_tun,
                ) {
                    Ok(json_cfg) => match v2ray::find_xray_binary() {
                        Ok(bin) => {
                            self.push_log(
                                "Config",
                                "Validating configuration with Xray core...",
                                LogLevel::Info,
                            );
                            Task::perform(
                                async move { v2ray::test_xray_config(&bin, &json_cfg).await },
                                |res| {
                                    match res {
                                    Ok(()) => Message::NewLog(LogEntry {
                                        timestamp: Local::now().format("%H:%M:%S").to_string(),
                                        source: "Config".into(),
                                        message: "✓ Configuration is 100% valid and verified with Xray core!".into(),
                                        level: LogLevel::Success,
                                    }),
                                    Err(e) => Message::NewLog(LogEntry {
                                        timestamp: Local::now().format("%H:%M:%S").to_string(),
                                        source: "Config".into(),
                                        message: format!("✗ Config validation error: {}", e),
                                        level: LogLevel::Error,
                                    }),
                                }
                                },
                            )
                        }
                        Err(e) => {
                            self.push_log(
                                "Config",
                                format!("Core binary not found: {}", e),
                                LogLevel::Warn,
                            );
                            Task::none()
                        }
                    },
                    Err(e) => {
                        self.push_log(
                            "Config",
                            format!("Config parse error: {}", e),
                            LogLevel::Error,
                        );
                        Task::none()
                    }
                }
            }
            Message::LoadSampleV2Ray => {
                let sample = "vless://00000000-0000-0000-0000-000000000000@example.com:443?encryption=none&security=tls&sni=example.com&type=tcp#Sample-Node".to_string();
                self.v2ray_uri_or_json = sample.clone();
                let id = format!(
                    "sample-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis()
                );
                self.v2ray_profiles.insert(
                    0,
                    storage::V2RayProfile {
                        id: id.clone(),
                        name: "Sample-Node".into(),
                        config: sample,
                        created_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                    },
                );
                self.active_profile_id = Some(id);
                let _ = self.save_current_state();
                self.push_log("Config", "Loaded Netray VLESS node.", LogLevel::Info);
                Task::none()
            }
            Message::ToggleTunMode(v) => {
                self.enable_tun = v;
                if v {
                    self.enable_sysproxy = false;
                }
                let _ = self.save_current_state();
                Task::none()
            }
            Message::ToggleSysProxy(v) => {
                self.enable_sysproxy = v;
                if v {
                    self.enable_tun = false;
                }
                let _ = self.save_current_state();
                Task::none()
            }
            Message::WhitelistAppsChanged(val) => {
                self.whitelist_apps = val;
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
                let _ = self.save_current_state();
                Task::none()
            }
            Message::BrowseAppExecutable => {
                Task::perform(apps::pick_executable_file(), Message::AppExecutablePicked)
            }
            Message::AppExecutablePicked(maybe_bin) => {
                if let Some(bin) = maybe_bin {
                    self.whitelist_apps = apps::add_app_to_whitelist(&self.whitelist_apps, &bin);
                    let _ = self.save_current_state();
                }
                Task::none()
            }
            Message::RemoveWhitelistApp(app_name) => {
                self.whitelist_apps = apps::remove_app_from_whitelist(&self.whitelist_apps, &app_name);
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
            Message::PayloadChanged(val) => {
                self.payload_text = val;
                Task::none()
            }
            Message::BugHostChanged(val) => {
                self.bug_host = val;
                Task::none()
            }
            Message::SniChanged(val) => {
                self.sni_host = val;
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
                tpl.push_str("User-Agent: Netray-Linux/1.0[crlf][crlf]");
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
                }
                let stopped = entry.source == "Supervisor" && entry.message.contains("offline");
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
                self.logs.push(entry);
                if self.logs.len() > 150 {
                    self.logs.drain(0..30);
                }
                if stopped && self.exit_after_stop {
                    self.exit_after_stop = false;
                    iced::exit()
                } else {
                    Task::none()
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
            Message::ShowWindow => self.show_window(),
            Message::DragWindow => self
                .window_id
                .map(iced::window::drag)
                .unwrap_or_else(Task::none),
            Message::MaximizeWindow => self
                .window_id
                .map(iced::window::toggle_maximize)
                .unwrap_or_else(Task::none),
            Message::RequestExit => {
                let task = self.show_window();
                self.close_confirmation = self.window_id;
                task
            }
            Message::MinimizeToTray => {
                if self.tray_icon.is_some() {
                    self.hidden_to_tray = true;
                    // Destroy the surface: Wayland does not implement set_visible(false).
                    self.window_id
                        .take()
                        .map(iced::window::close)
                        .unwrap_or_else(Task::none)
                } else {
                    self.window_id
                        .map(|id| iced::window::minimize(id, true))
                        .unwrap_or_else(Task::none)
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
                    iced::exit()
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
            Message::CancelExit => {
                self.close_confirmation = None;
                Task::none()
            }
            Message::ToggleTunnel => {
                if self.worker_active && !self.is_running {
                    if let Some(stop) = self.stop_signal.take() {
                        let _ = stop.send(());
                    }
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
                    self.is_running = false;
                    self.connected_at = None;
                    self.ping_ms = None;
                    self.push_log("Supervisor", "Tunnel stopped by user.", LogLevel::Warn);
                    Task::none()
                } else {
                    self.is_running = false;
                    self.worker_active = true;
                    self.connected_at = None;
                    self.ping_ms = None;
                    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
                    self.stop_signal = Some(stop_tx);

                    if let Some(log_tx) = self.log_sender.clone() {
                        let cfg = TunnelConfig {
                            mode: self.tunnel_mode,
                            proxy_addr: format!("{}:{}", self.proxy_ip, self.proxy_port),
                            ssh_host: self.ssh_host.clone(),
                            ssh_port: self.ssh_port.parse().unwrap_or(22),
                            ssh_user: self.ssh_user.clone(),
                            ssh_pass: self.ssh_pass.clone(),
                            bug_host: self.bug_host.clone(),
                            sni_host: self.sni_host.clone(),
                            payload_template: self.payload_text.clone(),
                            local_socks_port: self.local_socks_port.parse().unwrap_or(10808),
                            v2ray_config: self.v2ray_uri_or_json.clone(),
                            enable_tun: self.enable_tun,
                            enable_sysproxy: self.enable_sysproxy,
                            whitelist_apps: self.whitelist_apps.clone(),
                            whitelist_urls: self.whitelist_urls.clone(),
                        };

                        tokio::spawn(async move {
                            run_real_tunnel_worker(log_tx, stop_rx, cfg).await;
                        });
                    }
                    Task::none()
                }
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
                            let _ = clicks.unbounded_send(Message::ShowWindow);
                        }
                    }));
                    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                        let message = match event.id().as_ref() {
                            "show" => Message::ShowWindow,
                            "hide" => Message::MinimizeToTray,
                            "connect" => Message::ToggleTunnel,
                            "exit" => Message::RequestExit,
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
        if !self.hidden_to_tray && self.is_running && self.view_mode == ViewMode::Profiles {
            subscriptions
                .push(iced::time::every(Duration::from_secs(1)).map(|_| Message::UptimeTick));
        }
        Subscription::batch(subscriptions)
    }

    // ========================================================================
    // 4. VIEW DISPATCHER
    // ========================================================================

    fn view(&self, _: iced::window::Id) -> Element<'_, Message> {
        let content = match self.view_mode {
            ViewMode::Profiles => self.view_profiles_screen(),
            ViewMode::EditNode => self.view_edit_screen(),
            ViewMode::Logs => self.view_logs_screen(),
            ViewMode::SshSettings => self.view_ssh_screen(),
        };
        let window_control = |icon, label, message, close| {
            iced::widget::tooltip(
                button(
                    container(icons::lucide_icon(icon, 16, Color::WHITE))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill),
                )
                .width(32)
                .height(32)
                .padding(0)
                .style(move |_, status| button::Style {
                    background: match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(if close { FAILURE_RED } else { SURFACE_CARD }.into())
                        }
                        _ => Some(Color::from_rgb(0.22, 0.23, 0.24).into()),
                    },
                    text_color: Color::WHITE,
                    border: iced::Border {
                        radius: 16.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .on_press(message),
                text(label).size(12),
                iced::widget::tooltip::Position::Bottom,
            )
        };
        let titlebar = row![
            iced::widget::mouse_area(
                container(text("Netray").size(14).color(self.text_primary()))
                    .padding([0, 10])
                    .center_y(32)
                    .width(Length::Fill)
            )
            .on_press(Message::DragWindow),
            window_control(
                icons::MINIMIZE,
                "Minimize to tray",
                Message::MinimizeToTray,
                false
            ),
            window_control(
                icons::MAXIMIZE,
                "Maximize / restore",
                Message::MaximizeWindow,
                false
            ),
            window_control(icons::CLOSE, "Close", Message::RequestExit, true),
        ]
        .height(48)
        .spacing(8)
        .padding(8)
        .align_y(Alignment::Center);
        let content = container(column![titlebar, content].height(Length::Fill))
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

    // ========================================================================
    // SCREEN 1: PROFILES SCREEN
    // ========================================================================

    fn view_profiles_screen(&self) -> Element<'_, Message> {
        let config_locked = self.config_locked();
        // 1. Top App Bar
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
                .on_press_maybe((!config_locked).then(|| Message::LoadSampleV2Ray)),
            button(icons::lucide_icon(icons::QR_CODE, 20, self.text_primary()))
                .padding(6)
                .style(|_, _| button::Style {
                    background: None,
                    ..Default::default()
                })
                .on_press(Message::ValidateV2RayConfig),
            button(icons::lucide_icon(icons::LINK, 20, self.text_primary()))
                .padding(6)
                .style(|_, _| button::Style {
                    background: None,
                    ..Default::default()
                })
                .on_press_maybe((!config_locked).then(|| Message::RequestClipboardImport)),
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
            top: 8.0,
            bottom: 8.0,
            left: 12.0,
            right: 12.0,
        });

        // 2. Category Sub-Bar
        let sub_bar = row![
            button(icons::lucide_icon(icons::PLUS, 18, Color::WHITE))
                .padding(6)
                .on_press_maybe((!config_locked).then(|| Message::OpenAddMenu)),
            Space::with_width(12),
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
            checkbox("TUN", self.enable_tun)
                .on_toggle(Message::ToggleTunMode)
                .size(13),
            Space::with_width(8),
            checkbox("Proxy", self.enable_sysproxy)
                .on_toggle(Message::ToggleSysProxy)
                .size(13),
        ]
        .align_y(Alignment::Center)
        .padding(iced::Padding {
            top: 4.0,
            bottom: 8.0,
            left: 12.0,
            right: 12.0,
        });

        // 3. Node Cards List
        let cards: Vec<Element<Message>> = self
            .v2ray_profiles
            .iter()
            .map(|p| {
                let is_active = self.active_profile_id.as_deref() == Some(&p.id)
                    || self.v2ray_uri_or_json == p.config;
                let strip_color = if is_active {
                    ACCENT_BLUE
                } else {
                    INACTIVE_STRIP
                };
                let node_addr = v2ray::extract_node_address(&p.config);
                let proto_tag = v2ray::extract_protocol_tag(&p.config);

                let p_id_1 = p.id.clone();
                let p_id_2 = p.id.clone();
                let p_id_3 = p.id.clone();
                let share_id = p.id.clone();
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

                container(
                    row![
                        container(Space::with_width(2)).height(76).style(move |_| {
                            container::Style {
                                background: Some(strip_color.into()),
                                border: iced::Border {
                                    radius: 2.0.into(),
                                    ..Default::default()
                                },
                                ..Default::default()
                            }
                        }),
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
                            .style(|_, _| button::Style {
                                background: None,
                                text_color: Color::WHITE,
                                ..Default::default()
                            })
                            .on_press_maybe(
                                (!config_locked).then(|| Message::SelectProfile(p_id_1))
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
                                    .style(|_, _| button::Style {
                                        background: None,
                                        text_color: TEXT_MUTED,
                                        ..Default::default()
                                    })
                                    .on_press(Message::ShareProfile(share_id)),
                                button(icons::lucide_icon(icons::PENCIL, 16, TEXT_MUTED))
                                    .padding(6)
                                    .style(|_, _| button::Style {
                                        background: None,
                                        text_color: TEXT_MUTED,
                                        ..Default::default()
                                    })
                                    .on_press_maybe(
                                        (!config_locked).then(|| Message::EditProfile(p_id_2))
                                    ),
                                button(icons::lucide_icon(icons::TRASH, 16, TEXT_MUTED))
                                    .padding(6)
                                    .style(|_, _| button::Style {
                                        background: None,
                                        text_color: TEXT_MUTED,
                                        ..Default::default()
                                    })
                                    .on_press_maybe(
                                        (!config_locked).then(|| Message::DeleteProfile(p_id_3))
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
                button("📋 Import from Clipboard")
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

        // 4. Floating Action Button (FAB) at bottom-right
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
        .align_y(Alignment::Center)
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

    // ========================================================================
    // SCREEN 2: EDIT VLESS / NODE EDITOR (Matching Screenshots 2 & 3)
    // ========================================================================

    fn view_edit_screen(&self) -> Element<'_, Message> {
        let top_bar = row![
            button(icons::lucide_icon(icons::CLOSE, 22, Color::WHITE))
                .padding(6)
                .on_press(Message::CancelEditNode),
            Space::with_width(16),
            text("Edit VLess").size(20).color(Color::WHITE),
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

        let form_content = scrollable(
            column![
                remarks_card,
                addr_port_card,
                encrypt_card,
                flow_card,
                uuid_card,
                transport_card,
                header_card,
                tls_card,
                sni_card,
                fp_card,
                alpn_card,
                Space::with_height(20),
            ]
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

    // ========================================================================
    // SCREEN 3: REAL-TIME LOGS & DIAGNOSTICS
    // ========================================================================

    fn view_logs_screen(&self) -> Element<'_, Message> {
        let top_bar = row![
            button(icons::lucide_icon(icons::BACK, 22, Color::WHITE))
                .padding(6)
                .on_press(Message::SetViewMode(ViewMode::Profiles)),
            Space::with_width(12),
            text("Tunnel Logs & Status").size(20).color(Color::WHITE),
            Space::with_width(Length::Fill),
            button("Clear").on_press(Message::ClearLogs),
        ]
        .align_y(Alignment::Center)
        .padding(iced::Padding {
            top: 10.0,
            bottom: 10.0,
            left: 12.0,
            right: 12.0,
        });

        let http_port = self
            .local_socks_port
            .parse::<u16>()
            .unwrap_or(10808)
            .saturating_add(1);

        let status_bar = container(
            row![
                text(format!("SOCKS5: 127.0.0.1:{}", self.local_socks_port))
                    .size(12)
                    .color(ACCENT_BLUE_LIGHT),
                Space::with_width(16),
                text(format!("HTTP: 127.0.0.1:{}", http_port))
                    .size(12)
                    .color(ACCENT_BLUE_LIGHT),
                Space::with_width(Length::Fill),
                text(if self.is_running { "ACTIVE" } else { "STOPPED" })
                    .size(12)
                    .color(if self.is_running {
                        Color::from_rgb(0.2, 0.9, 0.4)
                    } else {
                        TEXT_MUTED
                    }),
            ]
            .align_y(Alignment::Center),
        )
        .padding(iced::Padding {
            top: 6.0,
            bottom: 6.0,
            left: 16.0,
            right: 16.0,
        })
        .style(|_| container::Style {
            background: Some(SURFACE_CARD.into()),
            ..Default::default()
        });

        let log_rows: Vec<Element<Message>> = self
            .logs
            .iter()
            .rev()
            .take(60)
            .rev()
            .map(|entry| {
                let color = match entry.level {
                    LogLevel::Info => Color::from_rgb(0.75, 0.82, 0.9),
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

    // ========================================================================
    // SCREEN 4: SSH & PAYLOAD GENERATOR SETTINGS
    // ========================================================================

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

        let chips = apps::extract_app_chips(&self.whitelist_apps);
        let mut chips_col = column![].spacing(6);
        for chunk in chips.chunks(3) {
            let mut r = row![].spacing(6);
            for app in chunk {
                let app_name = app.clone();
                let chip = container(
                    row![
                        text(app.clone()).size(12).color(Color::WHITE),
                        Space::with_width(4),
                        button(icons::lucide_icon(icons::CLOSE, 10, Color::from_rgb(0.85, 0.85, 0.85)))
                            .padding(2)
                            .style(|_, _| button::Style {
                                background: None,
                                ..Default::default()
                            })
                            .on_press(Message::RemoveWhitelistApp(app_name)),
                    ]
                    .align_y(Alignment::Center)
                )
                .padding(iced::Padding {
                    top: 3.0,
                    bottom: 3.0,
                    left: 8.0,
                    right: 4.0,
                })
                .style(|_| container::Style {
                    background: Some(Color::from_rgba(0.24, 0.58, 1.0, 0.18).into()),
                    border: iced::Border {
                        radius: 12.0.into(),
                        width: 1.0,
                        color: Color::from_rgba(0.24, 0.58, 1.0, 0.45),
                    },
                    ..Default::default()
                });
                r = r.push(chip);
            }
            chips_col = chips_col.push(r);
        }

        let app_picker_row = row![
            pick_list(
                self.available_apps.clone(),
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
            .style(|_, _| button::Style {
                background: Some(ACCENT_BLUE.into()),
                border: iced::Border {
                    radius: 6.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
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
                text_input("e.g. curl, firefox, chrome, discord", &self.whitelist_apps)
                    .on_input(Message::WhitelistAppsChanged)
                    .padding(8)
                    .width(Length::Fill),
                text("Select from catalog, browse executable, or enter comma/newline-separated names.")
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
                .style(|_, _| button::Style {
                    background: Some(ACCENT_BLUE.into()),
                    border: iced::Border {
                        radius: 6.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
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
                ip_card,
                Space::with_height(12),
                whitelist_card,
                Space::with_height(14),
                text("Tunneling Mode:").size(14).color(TEXT_MUTED),
                pick_list(
                    [
                        TunnelMode::V2RayXray,
                        TunnelMode::SshPayload,
                        TunnelMode::SshSslSni,
                        TunnelMode::SlowDns,
                    ],
                    Some(self.tunnel_mode),
                    Message::TunnelModeChanged
                )
                .width(Length::Fill),
                Space::with_height(10),
                text("HTTP Payload Template:").size(14).color(TEXT_MUTED),
                text_input("Enter HTTP payload...", &self.payload_text)
                    .on_input(Message::PayloadChanged)
                    .padding(8),
                Space::with_height(6),
                row![
                    checkbox("Keep-Alive", self.gen_keep_alive).on_toggle(Message::ToggleKeepAlive),
                    checkbox("X-Online-Host", self.gen_online_host)
                        .on_toggle(Message::ToggleOnlineHost),
                    checkbox("Use [split]", self.gen_split).on_toggle(Message::ToggleSplit),
                ]
                .spacing(15),
                Space::with_height(6),
                button("Generate Template Payload").on_press(Message::GeneratePayload),
                Space::with_height(14),
                text("Bug Host / CDN Host:").size(14).color(TEXT_MUTED),
                text_input("cdn.host.com", &self.bug_host)
                    .on_input(Message::BugHostChanged)
                    .padding(8),
                Space::with_height(10),
                text("TLS SNI Host:").size(14).color(TEXT_MUTED),
                text_input("sni.host.com", &self.sni_host)
                    .on_input(Message::SniChanged)
                    .padding(8),
                Space::with_height(14),
                text("SSH Server Details:").size(14).color(TEXT_MUTED),
                row![
                    text_input("Host", &self.ssh_host)
                        .on_input(Message::SshHostChanged)
                        .padding(8)
                        .width(Length::FillPortion(3)),
                    Space::with_width(8),
                    text_input("Port", &self.ssh_port)
                        .on_input(Message::SshPortChanged)
                        .padding(8)
                        .width(Length::FillPortion(1)),
                ],
                Space::with_height(8),
                row![
                    text_input("User", &self.ssh_user)
                        .on_input(Message::SshUserChanged)
                        .padding(8)
                        .width(Length::Fill),
                    Space::with_width(8),
                    text_input("Pass", &self.ssh_pass)
                        .on_input(Message::SshPassChanged)
                        .secure(true)
                        .padding(8)
                        .width(Length::Fill),
                ],
                Space::with_height(20),
            ]
            .spacing(6)
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
        assert_eq!(subscriptions(&app), 3);
        app.is_running = true;
        assert_eq!(subscriptions(&app), 4);
        for view in [ViewMode::Logs, ViewMode::EditNode, ViewMode::SshSettings] {
            app.view_mode = view;
            assert_eq!(subscriptions(&app), 3);
        }
        app.view_mode = ViewMode::Profiles;
        assert_eq!(subscriptions(&app), 4);
        app.hidden_to_tray = true;
        assert_eq!(subscriptions(&app), 3);
        app.hidden_to_tray = false;
        app.is_running = false;
        assert_eq!(subscriptions(&app), 3);
    }
    #[test]
    fn repeated_show_reuses_window_and_preserves_tunnel() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        app.hidden_to_tray = true;
        app.is_running = true;
        app.worker_active = true;
        app.connected_at = Some(Instant::now());
        let connected_at = app.connected_at;
        let _ = app.update(Message::ShowWindow);
        let window = app.window_id;
        assert!(window.is_some());
        let _ = app.update(Message::ShowWindow);
        assert_eq!(app.window_id, window);
        assert!(!app.hidden_to_tray);
        assert!(app.worker_active && app.is_running);
        assert_eq!(app.connected_at, connected_at);
    }

    #[test]
    fn tray_exit_restores_confirmation_and_waits_for_worker() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        app.hidden_to_tray = true;
        app.worker_active = true;
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        app.stop_signal = Some(tx);
        let _ = app.update(Message::RequestExit);
        assert!(app.window_id.is_some());
        assert_eq!(app.close_confirmation, app.window_id);
        assert!(!app.exit_after_stop);
        assert!(rx.try_recv().is_err());
        let _ = app.update(Message::ConfirmExit);
        assert!(app.exit_after_stop);
        assert_eq!(rx.try_recv(), Ok(()));
        assert!(app.worker_active);
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
    fn test_add_menu_toggle_and_cancel() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        assert!(!app.show_add_menu);
        let _ = app.update(Message::OpenAddMenu);
        assert!(app.show_add_menu);
        let _ = app.update(Message::CloseAddMenu);
        assert!(!app.show_add_menu);
    }

    #[test]
    fn test_create_profile_across_all_eleven_protocols() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        let initial_count = app.v2ray_profiles.len();
        for (i, proto) in protocols::ProfileType::ALL.iter().enumerate() {
            let _ = app.update(Message::CreateProfileWithType(*proto));
            assert_eq!(app.v2ray_profiles.len(), initial_count + i + 1);
            let created = app.v2ray_profiles.last().unwrap();
            assert_eq!(created.name, proto.generate_template().0);
            assert_eq!(app.active_profile_id, Some(created.id.clone()));
            assert!(!app.show_add_menu);
        }
    }

    #[test]
    fn test_clipboard_import_confirmation_flow() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        app.instant_import = false;
        let sample_link = "vless://00000000-0000-0000-0000-000000000000@1.2.3.4:443?type=tcp#ConfirmedNode";
        let initial_count = app.v2ray_profiles.len();

        let _ = app.update(Message::ClipboardImported(Some(sample_link.to_string())));
        assert_eq!(app.clipboard_import_candidate, Some(sample_link.to_string()));
        assert_eq!(app.v2ray_profiles.len(), initial_count);

        let _ = app.update(Message::ConfirmClipboardImport);
        assert_eq!(app.clipboard_import_candidate, None);
        assert_eq!(app.v2ray_profiles.len(), initial_count + 1);
        let top = &app.v2ray_profiles[0];
        assert_eq!(top.name, "ConfirmedNode");
        assert_eq!(top.config, sample_link);
        assert_eq!(app.active_profile_id, Some(top.id.clone()));
    }

    #[test]
    fn test_clipboard_import_cancel_flow() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        app.instant_import = false;
        let sample_link = "trojan://secret@9.9.9.9:443#CancelledNode";
        let initial_count = app.v2ray_profiles.len();

        let _ = app.update(Message::ClipboardImported(Some(sample_link.to_string())));
        assert_eq!(app.clipboard_import_candidate, Some(sample_link.to_string()));
        assert_eq!(app.v2ray_profiles.len(), initial_count);

        let _ = app.update(Message::CancelClipboardImport);
        assert_eq!(app.clipboard_import_candidate, None);
        assert_eq!(app.v2ray_profiles.len(), initial_count);
    }

    #[test]
    fn test_clipboard_instant_import_toggle() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        let _ = app.update(Message::ToggleInstantImport(true));
        assert!(app.instant_import);

        let sample_link = "ss://YWVzLTEyOC1nY206cGFzcw@8.8.8.8:8388#InstantNode";
        let initial_count = app.v2ray_profiles.len();

        let _ = app.update(Message::ClipboardImported(Some(sample_link.to_string())));
        assert_eq!(app.clipboard_import_candidate, None);
        assert_eq!(app.v2ray_profiles.len(), initial_count + 1);
        assert_eq!(app.v2ray_profiles[0].name, "InstantNode");

        let _ = app.update(Message::ToggleInstantImport(false));
        assert!(!app.instant_import);
    }

    #[test]
    fn test_share_modal_open_copy_and_close_flow() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        let profile_id = app.v2ray_profiles[0].id.clone();
        assert!(app.share_modal.is_none());

        let _ = app.update(Message::ShareProfile(profile_id.clone()));
        assert!(app.share_modal.is_some());
        let modal = app.share_modal.as_ref().unwrap();
        assert_eq!(modal.profile_id, profile_id);
        assert!(!modal.link.is_empty());
        assert!(!modal.qr_svg.is_empty());
        assert!(!modal.is_protected);
        assert!(!modal.copied);

        let _ = app.update(Message::ToggleShareProtect);
        assert!(app.share_modal.as_ref().unwrap().is_protected);

        let _ = app.update(Message::CopyShareLink);
        assert!(app.share_modal.as_ref().unwrap().copied);

        let _ = app.update(Message::CloseShareModal);
        assert!(app.share_modal.is_none());
    }

    #[test]
    fn test_theme_preference_lifecycle_and_switching() {
        let (mut app, _) = NetrayApp::from_saved(storage::AppData::default());
        assert_eq!(app.theme_preference, storage::ThemePreference::System);

        let _ = app.update(Message::ThemePreferenceChanged(storage::ThemePreference::Light));
        assert_eq!(app.theme_preference, storage::ThemePreference::Light);
        assert!(!app.is_dark());
        assert_eq!(app.theme(), Theme::Light);

        let _ = app.update(Message::ThemePreferenceChanged(storage::ThemePreference::Dark));
        assert_eq!(app.theme_preference, storage::ThemePreference::Dark);
        assert!(app.is_dark());
        assert_eq!(app.theme(), Theme::Dark);

        let _ = app.update(Message::ThemePreferenceChanged(storage::ThemePreference::System));
        assert_eq!(app.theme_preference, storage::ThemePreference::System);
        assert!(matches!(app.theme(), Theme::Dark | Theme::Light));
    }
}
