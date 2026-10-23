#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

pub mod icons;
pub mod storage;
pub mod sysproxy;
mod tunnel;
pub mod v2ray;

use chrono::Local;
use iced::futures::SinkExt;
use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, text, text_input, Space,
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
const SURFACE_CARD: Color = Color::from_rgb(0.14, 0.16, 0.20);
const TEXT_MUTED: Color = Color::from_rgb(0.55, 0.60, 0.68);
const INACTIVE_STRIP: Color = Color::from_rgb(0.30, 0.33, 0.38);
const SUCCESS_GREEN: Color = Color::from_rgb(0.22, 0.78, 0.49);
const FAILURE_RED: Color = Color::from_rgb(0.95, 0.35, 0.40);
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

fn form_card_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(SURFACE_CARD.into()),
        border: iced::Border {
            radius: 8.0.into(),
            width: 1.0,
            color: Color::from_rgb(0.20, 0.22, 0.28),
        },
        ..Default::default()
    }
}

pub fn main() -> iced::Result {
    let window = iced::window::Settings {
        size: (460.0, 600.0).into(),
        icon: create_window_icon(),
        exit_on_close_request: false,
        ..Default::default()
    };
    iced::application("Netray", NetrayApp::update, NetrayApp::view)
        .subscription(NetrayApp::subscription)
        .theme(|_| Theme::Dark)
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
    Ok((pixels[..info.buffer_size()].to_vec(), info.width, info.height))
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

pub struct NetrayApp {
    pub view_mode: ViewMode,
    pub is_running: bool,
    pub connected_at: Option<Instant>,
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

    pub enable_tun: bool,
    pub enable_sysproxy: bool,

    pub logs: Vec<LogEntry>,
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
    AddNewProfile,

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
    RequestClipboardImport,
    ClipboardImported(Option<String>),
    ToggleTunMode(bool),
    ToggleSysProxy(bool),

    LoggerReady(mpsc::Sender<LogEntry>),
    NewLog(LogEntry),
    UptimeTick,
    ClearLogs,
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

        let app = Self {
            view_mode: ViewMode::Profiles,
            is_running: false,
            connected_at: None,
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

            enable_tun: saved.enable_tun,
            enable_sysproxy: saved.enable_sysproxy && !saved.enable_tun,

            logs: vec![LogEntry {
                timestamp: Local::now().format("%H:%M:%S").to_string(),
                source: "Netray".into(),
                message: "Ready to connect on Windows.".into(),
                level: LogLevel::Info,
            }],
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

    fn update(&mut self, message: Message) -> Task<Message> {
        if self.config_locked()
            && matches!(
                &message,
                Message::SetViewMode(ViewMode::EditNode)
                    | Message::SelectProfile(_)
                    | Message::EditProfile(_)
                    | Message::DeleteProfile(_)
                    | Message::AddNewProfile
                    | Message::SaveEditedNode
                    | Message::RequestClipboardImport
                    | Message::ClipboardImported(_)
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
                if self.ping_in_progress.is_some() {
                    Task::none()
                } else if let Some(config) = self
                    .v2ray_profiles
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| p.config.clone())
                {
                    self.ping_results.remove(&id);
                    self.ping_in_progress = Some(id.clone());
                    Task::perform(
                        async move { v2ray::ping_profile(&config).await },
                        move |result| Message::PingFinished(id.clone(), result),
                    )
                } else {
                    Task::none()
                }
            }
            Message::PingFinished(id, result) => {
                if self.ping_in_progress.as_deref() == Some(&id) {
                    self.ping_in_progress = None;
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
                    self.ping_results.insert(id, display);
                    self.push_log("Ping", log, level);
                }
                Task::none()
            }
            Message::ShareProfile(id) => {
                if let Some(config) = self
                    .v2ray_profiles
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| p.config.clone())
                {
                    self.push_log(
                        "Share",
                        "Copied configuration link to clipboard.",
                        LogLevel::Success,
                    );
                    iced::clipboard::write(config)
                } else {
                    Task::none()
                }
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
                let id = self.node_edit_state.id.clone();
                let name = self.node_edit_state.remarks.trim().to_string();
                let uri = self.node_edit_state.to_vless_uri();
                self.ping_results.remove(&id);
                if self.ping_in_progress.as_deref() == Some(&id) {
                    self.ping_in_progress = None;
                }

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
            Message::RequestClipboardImport => {
                iced::clipboard::read().map(Message::ClipboardImported)
            }
            Message::ClipboardImported(content) => {
                if let Some(text) = content {
                    let trimmed = text.trim();
                    if trimmed.starts_with("vless://")
                        || trimmed.starts_with("vmess://")
                        || trimmed.starts_with("trojan://")
                        || trimmed.starts_with("ss://")
                        || trimmed.starts_with('{')
                    {
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
                if (entry.source == "Supervisor"
                    && (entry.message.contains("offline")
                        || entry.message.contains("Tunnel stopped")))
                    || (entry.source == "Error" && entry.message.contains("Engine failed"))
                {
                    self.is_running = false;
                    self.connected_at = None;
                    self.stop_signal = None;
                    self.worker_active = false;
                }
                let stopped = entry.source == "Supervisor"
                    && (entry.message.contains("offline")
                        || entry.message.contains("Tunnel stopped"));
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
                    Task::none()
                }
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
                        Err(error) => self.push_log("Tray", format!("System tray unavailable: {error}"), LogLevel::Warn),
                    }
                }
                Task::none()
            }
            Message::WindowTick => {
                if let Some(item) = &self.tray_connect_item {
                    item.set_text(if self.is_running { "Disconnect" } else { "Connect" });
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
            Message::CancelExit => {
                self.close_confirmation = None;
                Task::none()
            }
            Message::ToggleTunnel => {
                if !self.is_running && self.worker_active {
                    return Task::none();
                }
                if self.is_running {
                    if let Some(stop_tx) = self.stop_signal.take() {
                        let _ = stop_tx.send(());
                    }
                    self.is_running = false;
                    self.connected_at = None;
                    self.push_log("Supervisor", "Tunnel stopped by user.", LogLevel::Warn);
                    Task::none()
                } else {
                    self.is_running = true;
                    self.worker_active = true;
                    self.connected_at = Some(Instant::now());
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
        Subscription::batch(subscriptions)
    }

    fn view(&self) -> Element<'_, Message> {
        let content = match self.view_mode {
            ViewMode::Profiles => self.view_profiles_screen(),
            ViewMode::EditNode => self.view_edit_screen(),
            ViewMode::Logs => self.view_logs_screen(),
            ViewMode::SshSettings => self.view_ssh_screen(),
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
                content,
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
            content
        }
    }

    fn view_profiles_screen(&self) -> Element<'_, Message> {
        let config_locked = self.config_locked();
        let top_bar = row![
            button(icons::lucide_icon(icons::MENU, 22, Color::WHITE))
                .padding(6)
                .style(|_, _| button::Style {
                    background: None,
                    ..Default::default()
                })
                .on_press(Message::SetViewMode(ViewMode::SshSettings)),
            Space::with_width(12),
            text("Netray").size(22).color(Color::WHITE),
            Space::with_width(Length::Fill),
            button(icons::lucide_icon(icons::FILE_PLUS, 20, Color::WHITE))
                .padding(6)
                .style(|_, _| button::Style {
                    background: None,
                    ..Default::default()
                })
                .on_press_maybe((!config_locked).then(|| Message::AddNewProfile)),
            button(icons::lucide_icon(icons::LINK, 20, Color::WHITE))
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
            button(icons::lucide_icon(icons::MORE_VERTICAL, 20, Color::WHITE))
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
                .on_press_maybe((!config_locked).then(|| Message::AddNewProfile)),
            Space::with_width(10),
            text(format!("{} profiles", self.v2ray_profiles.len()))
                .size(13)
                .color(TEXT_MUTED),
            Space::with_width(Length::Fill),
            text(if self.is_running {
                "Tunnel active"
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
                let ping_press = self
                    .ping_in_progress
                    .is_none()
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
                                        text(p.name.as_str()).size(15).color(Color::WHITE),
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
                            .on_press_maybe((!config_locked).then(|| Message::SelectProfile(p_id_1))),
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
                                    .on_press_maybe((!config_locked).then(|| Message::EditProfile(p_id_2))),
                                button(icons::lucide_icon(icons::TRASH, 16, TEXT_MUTED))
                                    .padding(6)
                                    .style(|_, _| button::Style {
                                        background: None,
                                        text_color: TEXT_MUTED,
                                        ..Default::default()
                                    })
                                    .on_press_maybe((!config_locked).then(|| Message::DeleteProfile(p_id_3))),
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
                button("Import from Clipboard").on_press_maybe((!config_locked).then(|| Message::RequestClipboardImport)),
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

        let fab_icon = if self.is_running {
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
                        format!(
                            "CONNECTED · {}",
                            format_uptime(
                                self.connected_at.map(|at| at.elapsed()).unwrap_or_default()
                            )
                        )
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

        let terminal = container(scrollable(column(log_rows).spacing(6)).height(Length::Fill))
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
            button(icons::lucide_icon(icons::BACK, 22, Color::WHITE))
                .padding(6)
                .on_press(Message::SetViewMode(ViewMode::Profiles)),
            Space::with_width(12),
            text("SSH & Payload Injector").size(20).color(Color::WHITE),
        ]
        .align_y(Alignment::Center)
        .padding(iced::Padding {
            top: 10.0,
            bottom: 10.0,
            left: 12.0,
            right: 12.0,
        });

        let content = scrollable(
            column![
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
            .padding(iced::Padding {
                top: 0.0,
                bottom: 20.0,
                left: 16.0,
                right: 16.0,
            }),
        )
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
}
