mod tunnel;
pub mod sysproxy;
pub mod v2ray;
pub mod storage;
pub mod icons;

use tunnel::{run_real_tunnel_worker, TunnelConfig};
use chrono::Local;
use iced::futures::SinkExt;
use std::collections::HashMap;
use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, text, text_input, Space,
};
use iced::{Alignment, Color, Element, Length, Subscription, Task, Theme};
use tokio::sync::mpsc;

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
    iced::application(
        "Netray (Rust)",
        NetrayApp::update,
        NetrayApp::view,
    )
    .subscription(NetrayApp::subscription)
    .theme(|_| Theme::Dark)
    .window_size((520.0, 780.0))
    .run_with(NetrayApp::new)
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
    tx: &mpsc::UnboundedSender<LogEntry>,
    source: impl Into<String>,
    msg: impl Into<String>,
    level: LogLevel,
) {
    let _ = tx.send(LogEntry {
        timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
        source: source.into(),
        message: msg.into(),
        level,
    });
}

pub struct NetrayApp {
    pub view_mode: ViewMode,
    pub is_running: bool,
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

    // TUN & System Routing Mode
    pub enable_tun: bool,
    pub enable_sysproxy: bool,

    // Real-time Logs & Background Worker Control
    pub logs: Vec<LogEntry>,
    pub log_sender: Option<mpsc::UnboundedSender<LogEntry>>,
    pub stop_signal: Option<tokio::sync::oneshot::Sender<()>>,
}

// ============================================================================
// 2. MESSAGES (EVENTS)
// ============================================================================

#[derive(Debug, Clone)]
pub enum Message {
    SetViewMode(ViewMode),
    TunnelModeChanged(TunnelMode),
    ToggleTunnel,

    // Node & Profile management
    SelectProfile(String),
    PingProfile(String),
    PingFinished(String, Result<u128, String>),
    EditProfile(String),
    DeleteProfile(String),
    ShareProfile(String),
    AddNewProfile,

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
    ToggleTunMode(bool),
    ToggleSysProxy(bool),

    // Real-time Logger Events
    LoggerReady(mpsc::UnboundedSender<LogEntry>),
    NewLog(LogEntry),
    ClearLogs,
}

impl NetrayApp {
    fn new() -> (Self, Task<Message>) {
        let saved = storage::load_data();

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
            enable_sysproxy: saved.enable_sysproxy,

            logs: vec![LogEntry {
                timestamp: Local::now().format("%H:%M:%S").to_string(),
                source: "Netray".into(),
                message: "Ready to connect.".into(),
                level: LogLevel::Info,
            }],
            log_sender: None,
            stop_signal: None,
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

    // ========================================================================
    // 3. STATE UPDATE & TUNNEL SUPERVISOR
    // ========================================================================

    fn update(&mut self, message: Message) -> Task<Message> {
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
                } else if let Some(config) = self.v2ray_profiles.iter().find(|p| p.id == id).map(|p| p.config.clone()) {
                    self.ping_results.remove(&id);
                    self.ping_in_progress = Some(id.clone());
                    Task::perform(async move { v2ray::ping_profile(&config).await }, move |result| Message::PingFinished(id.clone(), result))
                } else {
                    Task::none()
                }
            }
            Message::PingFinished(id, result) => {
                if self.ping_in_progress.as_deref() == Some(&id) {
                    self.ping_in_progress = None;
                    let (display, log, level) = match result {
                        Ok(ms) => (format!("TCP · {ms} ms"), format!("{}: TCP connection in {ms} ms", self.v2ray_profiles.iter().find(|p| p.id == id).map(|p| p.name.as_str()).unwrap_or("Profile")), LogLevel::Success),
                        Err(error) => ("Failed".to_string(), format!("Ping failed for {}: {error}", self.v2ray_profiles.iter().find(|p| p.id == id).map(|p| p.name.as_str()).unwrap_or("profile")), LogLevel::Warn),
                    };
                    self.ping_results.insert(id, display);
                    self.push_log("Ping", log, level);
                }
                Task::none()
            }
            Message::ShareProfile(cfg) => {
                self.push_log("Share", "Copied configuration link to clipboard.", LogLevel::Success);
                iced::clipboard::write(cfg)
            }
            Message::EditProfile(id) => {
                if let Some(p) = self.v2ray_profiles.iter().find(|p| p.id == id) {
                    self.node_edit_state = v2ray::NodeEditState::from_profile(&p.id, &p.name, &p.config);
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
                self.node_edit_state = v2ray::NodeEditState::from_profile(&id, "New VLess Node", "");
                self.view_mode = ViewMode::EditNode;
                Task::none()
            }
            Message::EditRemarksChanged(v) => { self.node_edit_state.remarks = v; Task::none() }
            Message::EditAddressChanged(v) => { self.node_edit_state.address = v; Task::none() }
            Message::EditPortChanged(v) => { self.node_edit_state.port = v; Task::none() }
            Message::EditEncryptMethodChanged(v) => { self.node_edit_state.encrypt_method = v; Task::none() }
            Message::EditFlowChanged(v) => { self.node_edit_state.flow = v; Task::none() }
            Message::EditUserIdChanged(v) => { self.node_edit_state.user_id = v; Task::none() }
            Message::EditTransportChanged(v) => { self.node_edit_state.transport_protocol = v; Task::none() }
            Message::EditHeaderTypeChanged(v) => { self.node_edit_state.header_type = v; Task::none() }
            Message::EditTlsTypeChanged(v) => { self.node_edit_state.tls_type = v; Task::none() }
            Message::EditSniChanged(v) => { self.node_edit_state.sni = v; Task::none() }
            Message::EditFingerprintChanged(v) => { self.node_edit_state.fingerprint = v; Task::none() }
            Message::EditAlpnChanged(v) => { self.node_edit_state.alpn = v; Task::none() }
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
                self.push_log("Config", format!("Saved node '{}'.", name), LogLevel::Success);
                self.view_mode = ViewMode::Profiles;
                Task::none()
            }
            Message::RequestClipboardImport => iced::clipboard::read().map(Message::ClipboardImported),
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
                match v2ray::parse_and_generate_xray_config(&self.v2ray_uri_or_json, socks_p, http_p, self.enable_tun) {
                    Ok(json_cfg) => {
                        match v2ray::find_xray_binary() {
                            Ok(bin) => {
                                self.push_log("Config", "Validating configuration with Xray core...", LogLevel::Info);
                                Task::perform(async move {
                                    v2ray::test_xray_config(&bin, &json_cfg).await
                                }, |res| match res {
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
                                })
                            }
                            Err(e) => {
                                self.push_log("Config", format!("Core binary not found: {}", e), LogLevel::Warn);
                                Task::none()
                            }
                        }
                    }
                    Err(e) => {
                        self.push_log("Config", format!("Config parse error: {}", e), LogLevel::Error);
                        Task::none()
                    }
                }
            }
            Message::LoadSampleV2Ray => {
                let sample = "vless://b831381d-6324-4d53-ad4f-8cda48b30811@vsserver.ddnsfree.com:443?encryption=none&security=tls&sni=zoom.us&type=tcp#SLT%20zoom-mu7acumuex".to_string();
                self.v2ray_uri_or_json = sample.clone();
                let id = format!("sample-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis());
                self.v2ray_profiles.insert(0, storage::V2RayProfile {
                    id: id.clone(),
                    name: "SLT zoom-mu7acumuex".into(),
                    config: sample,
                    created_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                });
                self.active_profile_id = Some(id);
                let _ = self.save_current_state();
                self.push_log("Config", "Loaded Netray VLESS node.", LogLevel::Info);
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
            Message::PayloadChanged(val) => { self.payload_text = val; Task::none() }
            Message::BugHostChanged(val) => { self.bug_host = val; Task::none() }
            Message::SniChanged(val) => { self.sni_host = val; Task::none() }
            Message::ToggleKeepAlive(v) => { self.gen_keep_alive = v; Task::none() }
            Message::ToggleOnlineHost(v) => { self.gen_online_host = v; Task::none() }
            Message::ToggleSplit(v) => { self.gen_split = v; Task::none() }
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
            Message::SshHostChanged(v) => { self.ssh_host = v; Task::none() }
            Message::SshPortChanged(v) => { self.ssh_port = v; Task::none() }
            Message::SshUserChanged(v) => { self.ssh_user = v; Task::none() }
            Message::SshPassChanged(v) => { self.ssh_pass = v; Task::none() }
            Message::ProxyIpChanged(v) => { self.proxy_ip = v; Task::none() }
            Message::ProxyPortChanged(v) => { self.proxy_port = v; Task::none() }
            Message::V2RayConfigChanged(v) => { self.v2ray_uri_or_json = v; Task::none() }
            Message::LocalSocksPortChanged(v) => { self.local_socks_port = v; Task::none() }
            Message::LoggerReady(sender) => {
                self.log_sender = Some(sender);
                Task::none()
            }
            Message::NewLog(entry) => {
                if (entry.source == "Supervisor" && entry.message.contains("offline"))
                    || (entry.source == "Error" && entry.message.contains("Engine failed"))
                {
                    self.is_running = false;
                    self.stop_signal = None;
                }
                self.logs.push(entry);
                if self.logs.len() > 150 {
                    self.logs.drain(0..30);
                }
                Task::none()
            }
            Message::ClearLogs => {
                self.logs.clear();
                Task::none()
            }
            Message::ToggleTunnel => {
                if self.is_running {
                    if let Some(stop_tx) = self.stop_signal.take() {
                        let _ = stop_tx.send(());
                    }
                    self.is_running = false;
                    self.push_log("Supervisor", "Tunnel stopped by user.", LogLevel::Warn);
                    Task::none()
                } else {
                    self.is_running = true;
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
        Subscription::run(|| {
            iced::stream::channel(100, |mut output| async move {
                let (tx, mut rx) = mpsc::unbounded_channel::<LogEntry>();
                let _ = output.send(Message::LoggerReady(tx)).await;
                while let Some(log_entry) = rx.recv().await {
                    let _ = output.send(Message::NewLog(log_entry)).await;
                }
            })
        })
    }

    // ========================================================================
    // 4. VIEW DISPATCHER
    // ========================================================================

    fn view(&self) -> Element<'_, Message> {
        match self.view_mode {
            ViewMode::Profiles => self.view_profiles_screen(),
            ViewMode::EditNode => self.view_edit_screen(),
            ViewMode::Logs => self.view_logs_screen(),
            ViewMode::SshSettings => self.view_ssh_screen(),
        }
    }

    // ========================================================================
    // SCREEN 1: PROFILES SCREEN
    // ========================================================================

    fn view_profiles_screen(&self) -> Element<'_, Message> {
        // 1. Top App Bar
        let top_bar = row![
            button(icons::lucide_icon(icons::MENU, 22, Color::WHITE))
                .padding(6)
                .on_press(Message::SetViewMode(ViewMode::SshSettings)),
            Space::with_width(12),
            text("Netray").size(22).color(Color::WHITE),
            Space::with_width(Length::Fill),
            button(icons::lucide_icon(icons::FILE_PLUS, 20, Color::WHITE))
                .padding(6)
                .on_press(Message::LoadSampleV2Ray),
            button(icons::lucide_icon(icons::QR_CODE, 20, Color::WHITE))
                .padding(6)
                .on_press(Message::ValidateV2RayConfig),
            button(icons::lucide_icon(icons::LINK, 20, Color::WHITE))
                .padding(6)
                .on_press(Message::RequestClipboardImport),
            button(icons::lucide_icon(icons::MORE_VERTICAL, 20, Color::WHITE))
                .padding(6)
                .on_press(Message::SetViewMode(ViewMode::Logs)),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .padding(iced::Padding { top: 8.0, bottom: 8.0, left: 12.0, right: 12.0 });

        // 2. Category Sub-Bar
        let sub_bar = row![
            button(icons::lucide_icon(icons::PLUS, 18, Color::WHITE))
                .padding(6)
                .on_press(Message::AddNewProfile),
            Space::with_width(12),
            text(format!("{} profiles", self.v2ray_profiles.len())).size(13).color(TEXT_MUTED),
            Space::with_width(Length::Fill),
            text(if self.is_running { "Tunnel active" } else { "Offline" })
                .size(12)
                .color(if self.is_running { SUCCESS_GREEN } else { TEXT_MUTED }),
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
        .padding(iced::Padding { top: 4.0, bottom: 8.0, left: 12.0, right: 12.0 });

        // 3. Node Cards List
        let cards: Vec<Element<Message>> = self
            .v2ray_profiles
            .iter()
            .map(|p| {
                let is_active = self.active_profile_id.as_deref() == Some(&p.id)
                    || self.v2ray_uri_or_json == p.config;
                let strip_color = if is_active { ACCENT_BLUE } else { INACTIVE_STRIP };
                let node_addr = v2ray::extract_node_address(&p.config);
                let proto_tag = v2ray::extract_protocol_tag(&p.config);

                let p_id_1 = p.id.clone();
                let p_id_2 = p.id.clone();
                let p_id_3 = p.id.clone();
                let p_cfg_1 = p.config.clone();
                let ping_label = if self.ping_in_progress.as_deref() == Some(&p.id) { "Testing…" } else { "Ping" };
                let ping_press = self.ping_in_progress.is_none().then(|| Message::PingProfile(p.id.clone()));
                let ping_result = self.ping_results.get(&p.id).cloned().unwrap_or_default();
                let ping_color = if ping_result == "Failed" { FAILURE_RED } else { SUCCESS_GREEN };

                container(row![
                    container(Space::with_width(2))
                        .height(76)
                        .style(move |_| container::Style {
                            background: Some(strip_color.into()),
                            border: iced::Border { radius: 2.0.into(), ..Default::default() },
                            ..Default::default()
                        }),
                    Space::with_width(12),
                    column![
                        button(column![
                            text(p.name.clone()).size(15).color(Color::WHITE),
                            text(node_addr).size(12).color(TEXT_MUTED),
                        ].spacing(4).width(Length::Fill))
                        .padding(0)
                        .width(Length::Fill)
                        .style(|_, _| button::Style { background: None, text_color: Color::WHITE, ..Default::default() })
                        .on_press(Message::SelectProfile(p_id_1)),
                        row![
                            text(proto_tag).size(10).color(ACCENT_BLUE_LIGHT),
                            text(ping_result).size(11).color(ping_color),
                            Space::with_width(Length::Fill),
                            button(ping_label).padding(6).style(|_, _| button::Style { background: Some(ACCENT_BLUE.into()), text_color: Color::WHITE, border: iced::Border { radius: 6.0.into(), ..Default::default() }, ..Default::default() }).on_press_maybe(ping_press),
                            button(icons::lucide_icon(icons::SHARE, 16, TEXT_MUTED)).padding(6).style(|_, _| button::Style { background: None, text_color: TEXT_MUTED, ..Default::default() }).on_press(Message::ShareProfile(p_cfg_1)),
                            button(icons::lucide_icon(icons::PENCIL, 16, TEXT_MUTED)).padding(6).style(|_, _| button::Style { background: None, text_color: TEXT_MUTED, ..Default::default() }).on_press(Message::EditProfile(p_id_2)),
                            button(icons::lucide_icon(icons::TRASH, 16, TEXT_MUTED)).padding(6).style(|_, _| button::Style { background: None, text_color: TEXT_MUTED, ..Default::default() }).on_press(Message::DeleteProfile(p_id_3)),
                        ].spacing(4).align_y(Alignment::Center),
                    ].spacing(10).width(Length::Fill),
                ].align_y(Alignment::Center).padding(12))
                .width(Length::Fill)
                .style(form_card_style)
                .into()
            })
            .collect();

        let list_content: Element<'_, Message> = if cards.is_empty() {
            column![
                Space::with_height(40),
                text("No profiles added yet.")
                    .size(15)
                    .color(TEXT_MUTED),
                Space::with_height(10),
                button("📋 Import from Clipboard").on_press(Message::RequestClipboardImport),
            ]
            .align_x(Alignment::Center)
            .width(Length::Fill)
            .into()
        } else {
            scrollable(column(cards).spacing(10).padding(iced::Padding { top: 0.0, bottom: 80.0, left: 12.0, right: 12.0 }))
                .height(Length::Fill)
                .into()
        };

        // 4. Floating Action Button (FAB) at bottom-right
        let fab_icon = if self.is_running {
            icons::lucide_icon(icons::SQUARE, 26, Color::WHITE)
        } else {
            icons::lucide_icon(icons::PLAY, 26, Color::WHITE)
        };

        let fab_row = row![
            button(
                text(if self.is_running { "● RUNNING (VIEW LOGS)" } else { "● DISCONNECTED" })
                    .size(12)
                    .color(if self.is_running { Color::from_rgb(0.2, 0.9, 0.4) } else { TEXT_MUTED })
            )
            .padding(8)
            .on_press(Message::SetViewMode(ViewMode::Logs)),
            Space::with_width(Length::Fill),
            button(fab_icon)
                .padding(14)
                .on_press(Message::ToggleTunnel),
        ]
        .align_y(Alignment::Center)
        .padding(iced::Padding { top: 6.0, bottom: 12.0, left: 16.0, right: 16.0 });

        column![
            top_bar,
            sub_bar,
            list_content,
            fab_row,
        ]
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
        .padding(iced::Padding { top: 10.0, bottom: 10.0, left: 12.0, right: 12.0 });

        let remarks_card = container(
            column![
                text("Remarks").size(11).color(TEXT_MUTED),
                text_input("Remarks", &self.node_edit_state.remarks)
                    .on_input(Message::EditRemarksChanged)
                    .padding(6),
            ]
            .spacing(4)
        )
        .padding(10)
        .width(Length::Fill)
        .style(form_card_style);

        let addr_port_card = row![
            container(
                column![
                    text("Address").size(11).color(TEXT_MUTED),
                    text_input("vsserver.ddnsfree.com", &self.node_edit_state.address)
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
            .spacing(4)
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
            .spacing(4)
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
            .spacing(4)
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
            .spacing(4)
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
            .spacing(4)
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
            .spacing(4)
        )
        .padding(10)
        .width(Length::Fill)
        .style(form_card_style);

        let sni_card = container(
            column![
                text("Server name indication").size(11).color(TEXT_MUTED),
                text_input("SNI (e.g. zoom.us)", &self.node_edit_state.sni)
                    .on_input(Message::EditSniChanged)
                    .padding(6),
            ]
            .spacing(4)
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
            .spacing(4)
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
            .spacing(4)
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
            .padding(iced::Padding { top: 0.0, bottom: 20.0, left: 16.0, right: 16.0 })
        )
        .height(Length::Fill);

        column![
            top_bar,
            form_content,
        ]
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
        .padding(iced::Padding { top: 10.0, bottom: 10.0, left: 12.0, right: 12.0 });

        let http_port = self
            .local_socks_port
            .parse::<u16>()
            .unwrap_or(10808)
            .saturating_add(1);

        let status_bar = container(
            row![
                text(format!("SOCKS5: 127.0.0.1:{}", self.local_socks_port)).size(12).color(ACCENT_BLUE_LIGHT),
                Space::with_width(16),
                text(format!("HTTP: 127.0.0.1:{}", http_port)).size(12).color(ACCENT_BLUE_LIGHT),
                Space::with_width(Length::Fill),
                text(if self.is_running { "ACTIVE" } else { "STOPPED" })
                    .size(12)
                    .color(if self.is_running { Color::from_rgb(0.2, 0.9, 0.4) } else { TEXT_MUTED }),
            ]
            .align_y(Alignment::Center)
        )
        .padding(iced::Padding { top: 6.0, bottom: 6.0, left: 16.0, right: 16.0 })
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
                    text(format!("[{}]", entry.timestamp)).size(12).color(Color::from_rgb(0.5, 0.5, 0.5)),
                    text(format!("[{}]", entry.source)).size(12).color(ACCENT_BLUE_LIGHT),
                    text(&entry.message).size(12).color(color),
                ]
                .spacing(8)
                .into()
            })
            .collect();

        let terminal = container(
            scrollable(column(log_rows).spacing(6))
                .height(Length::Fill)
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
            container(terminal).padding(iced::Padding { top: 8.0, bottom: 12.0, left: 12.0, right: 12.0 }).height(Length::Fill),
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
            button(icons::lucide_icon(icons::BACK, 22, Color::WHITE))
                .padding(6)
                .on_press(Message::SetViewMode(ViewMode::Profiles)),
            Space::with_width(12),
            text("SSH & Payload Injector").size(20).color(Color::WHITE),
        ]
        .align_y(Alignment::Center)
        .padding(iced::Padding { top: 10.0, bottom: 10.0, left: 12.0, right: 12.0 });

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
                Space::with_height(6),
                row![
                    checkbox("Keep-Alive", self.gen_keep_alive).on_toggle(Message::ToggleKeepAlive),
                    checkbox("X-Online-Host", self.gen_online_host).on_toggle(Message::ToggleOnlineHost),
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
                    text_input("Host", &self.ssh_host).on_input(Message::SshHostChanged).padding(8).width(Length::FillPortion(3)),
                    Space::with_width(8),
                    text_input("Port", &self.ssh_port).on_input(Message::SshPortChanged).padding(8).width(Length::FillPortion(1)),
                ],
                Space::with_height(8),
                row![
                    text_input("User", &self.ssh_user).on_input(Message::SshUserChanged).padding(8).width(Length::Fill),
                    Space::with_width(8),
                    text_input("Pass", &self.ssh_pass).on_input(Message::SshPassChanged).secure(true).padding(8).width(Length::Fill),
                ],
                Space::with_height(20),
            ]
            .spacing(6)
            .padding(iced::Padding { top: 0.0, bottom: 20.0, left: 16.0, right: 16.0 })
        )
        .height(Length::Fill);

        column![
            top_bar,
            content,
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
