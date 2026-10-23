mod tunnel;
pub mod sysproxy;
pub mod v2ray;
pub mod storage;
use tunnel::{run_real_tunnel_worker, TunnelConfig};
use chrono::Local;
use iced::futures::SinkExt;
use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, text, text_input, Space,
};
use iced::{Alignment, Color, Element, Length, Subscription, Task, Theme};
use tokio::sync::mpsc;

pub fn main() -> iced::Result {
    iced::application(
        "Netray Linux (Rust)",
        NetrayApp::update,
        NetrayApp::view,
    )
    .subscription(NetrayApp::subscription)
    .theme(|_| Theme::Dark)
    .window_size((920.0, 620.0))
    .run_with(NetrayApp::new)
}

// ============================================================================
// 1. UI & APPLICATION STATE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveTab {
    Home,
    PayloadSni,
    SshConfig,
    V2RayConfig,
    Logs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

pub struct NetrayApp {
    pub active_tab: ActiveTab,
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

    // V2Ray State
    pub v2ray_uri_or_json: String,

    // TUN & System Routing Mode
    pub enable_tun: bool,
    pub enable_sysproxy: bool,

    // Saved V2Ray Profiles & Persistence
    pub v2ray_profiles: Vec<storage::V2RayProfile>,
    pub active_profile_id: Option<String>,

    // Real-time Logs & Background Worker Control
    pub logs: Vec<LogEntry>,
    pub log_sender: Option<mpsc::UnboundedSender<LogEntry>>,
    pub stop_signal: Option<tokio::sync::oneshot::Sender<()>>,
    pub log_scroll_id: scrollable::Id,
}

// ============================================================================
// 2. MESSAGES (EVENTS)
// ============================================================================

#[derive(Debug, Clone)]
pub enum Message {
    TabSelected(ActiveTab),
    TunnelModeChanged(TunnelMode),
    ToggleTunnel,

    // Payload Generator Controls
    PayloadChanged(String),
    BugHostChanged(String),
    SniChanged(String),
    ToggleKeepAlive(bool),
    ToggleOnlineHost(bool),
    ToggleSplit(bool),
    GeneratePayload,

    // SSH & V2Ray Inputs
    SshHostChanged(String),
    SshPortChanged(String),
    SshUserChanged(String),
    SshPassChanged(String),
    ProxyIpChanged(String),
    ProxyPortChanged(String),
    V2RayConfigChanged(String),
    LocalSocksPortChanged(String),
    ValidateV2RayConfig,
    LoadSampleV2Ray,
    RequestClipboardImport,
    ClipboardImported(Option<String>),
    ToggleTunMode(bool),
    ToggleSysProxy(bool),

    // Profile Management & Persistence
    SaveCurrentProfile,
    SelectProfile(String),
    DeleteProfile(String),
    ExportActiveXrayJson,

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

        let app = Self {
            active_tab: ActiveTab::V2RayConfig,
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

            v2ray_uri_or_json: saved.v2ray_uri_or_json,
            enable_tun: saved.enable_tun,
            enable_sysproxy: saved.enable_sysproxy,
            v2ray_profiles: saved.v2ray_profiles,
            active_profile_id: saved.active_profile_id,

            logs: vec![LogEntry {
                timestamp: Local::now().format("%H:%M:%S").to_string(),
                source: "Storage".into(),
                message: format!(
                    "Loaded persistent configuration from {}",
                    storage::get_config_path().display()
                ),
                level: LogLevel::Info,
            }],
            log_sender: None,
            stop_signal: None,
            log_scroll_id: scrollable::Id::unique(),
        };

        (app, Task::none())
    }

    fn save_current_state(&self) -> Result<std::path::PathBuf, String> {
        let app_data = storage::AppData {
            v2ray_profiles: self.v2ray_profiles.clone(),
            active_profile_id: self.active_profile_id.clone(),
            v2ray_uri_or_json: self.v2ray_uri_or_json.clone(),
            local_socks_port: self.local_socks_port.clone(),
            enable_tun: self.enable_tun,
            enable_sysproxy: self.enable_sysproxy,
            tunnel_mode: match self.tunnel_mode {
                TunnelMode::SshPayload => "SshPayload".to_string(),
                TunnelMode::SshSslSni => "SshSslSni".to_string(),
                TunnelMode::V2RayXray => "V2RayXray".to_string(),
                TunnelMode::SlowDns => "SlowDns".to_string(),
            },
            ssh_host: self.ssh_host.clone(),
            ssh_port: self.ssh_port.clone(),
            ssh_user: self.ssh_user.clone(),
            ssh_pass: self.ssh_pass.clone(),
            proxy_ip: self.proxy_ip.clone(),
            proxy_port: self.proxy_port.clone(),
            payload_text: self.payload_text.clone(),
            bug_host: self.bug_host.clone(),
            sni_host: self.sni_host.clone(),
        };
        storage::save_data(&app_data)
    }

    // Helper to push logs from anywhere in the UI or background tasks
    fn push_log(&mut self, source: &str, message: impl Into<String>, level: LogLevel) {
        self.logs.push(LogEntry {
            timestamp: Local::now().format("%H:%M:%S").to_string(),
            source: source.to_string(),
            message: message.into(),
            level,
        });
    }

    // ========================================================================
    // 3. STATE UPDATE & TUNNEL SUPERVISOR
    // ========================================================================

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TabSelected(tab) => {
                self.active_tab = tab;
                Task::none()
            }
            Message::TunnelModeChanged(mode) => {
                self.tunnel_mode = mode;
                self.push_log("Config", format!("Switched mode to: {}", mode), LogLevel::Info);
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
                self.push_log("Generator", "Generated new HTTP payload template.", LogLevel::Info);
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

            Message::LoadSampleV2Ray => {
                self.v2ray_uri_or_json = "vless://b831381d-6324-4d53-ad4f-8cda48b30811@example.com:443?encryption=none&security=tls&sni=example.com&type=ws&path=%2F#Sample-VLESS-Node".to_string();
                self.push_log("Config", "Loaded sample VLESS configuration.", LogLevel::Info);
                Task::none()
            }

            Message::ValidateV2RayConfig => {
                let socks_p: u16 = self.local_socks_port.parse().unwrap_or(10808);
                let http_p = socks_p.saturating_add(1);
                match crate::v2ray::parse_and_generate_xray_config(&self.v2ray_uri_or_json, socks_p, http_p, self.enable_tun) {
                    Ok(json_cfg) => {
                        match crate::v2ray::find_xray_binary() {
                            Ok(bin) => {
                                self.push_log("Config", format!("Parsed config OK. Testing with core ({})...", bin.display()), LogLevel::Info);
                                Task::perform(async move {
                                    crate::v2ray::test_xray_config(&bin, &json_cfg).await
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
                                self.push_log("Config", format!("Parsed OK, but core binary not found: {}", e), LogLevel::Warn);
                                Task::none()
                            }
                        }
                    }
                    Err(e) => {
                        self.push_log("Config", format!("Config syntax error: {}", e), LogLevel::Error);
                        Task::none()
                    }
                }
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
                        let node_name = crate::v2ray::extract_node_name(trimmed);
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
                        self.active_tab = ActiveTab::V2RayConfig;

                        if let Ok(path) = self.save_current_state() {
                            self.push_log(
                                "Storage",
                                format!("Auto-saved node '{}' to {}", node_name, path.display()),
                                LogLevel::Success,
                            );
                        }
                        self.push_log(
                            "Clipboard",
                            format!(
                                "Imported V2Ray node '{}' ({} chars). Switched mode to V2Ray/Xray.",
                                node_name,
                                trimmed.len()
                            ),
                            LogLevel::Success,
                        );
                    } else {
                        self.push_log(
                            "Clipboard",
                            "Clipboard does not contain a valid V2Ray link (vless://, vmess://, trojan://, ss://, or JSON).",
                            LogLevel::Warn,
                        );
                    }
                } else {
                    self.push_log("Clipboard", "Clipboard is empty or inaccessible.", LogLevel::Warn);
                }
                Task::none()
            }

            Message::SaveCurrentProfile => {
                let trimmed = self.v2ray_uri_or_json.trim();
                if !trimmed.is_empty() {
                    let node_name = crate::v2ray::extract_node_name(trimmed);
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
                    if let Ok(path) = self.save_current_state() {
                        self.push_log(
                            "Storage",
                            format!("Saved profile '{}' to {}", node_name, path.display()),
                            LogLevel::Success,
                        );
                    }
                }
                Task::none()
            }

            Message::SelectProfile(id) => {
                if let Some(p) = self.v2ray_profiles.iter().find(|p| p.id == id).cloned() {
                    self.v2ray_uri_or_json = p.config;
                    self.active_profile_id = Some(id);
                    self.tunnel_mode = TunnelMode::V2RayXray;
                    self.active_tab = ActiveTab::V2RayConfig;
                    let _ = self.save_current_state();
                    self.push_log("Storage", format!("Activated profile '{}'.", p.name), LogLevel::Success);
                }
                Task::none()
            }

            Message::DeleteProfile(id) => {
                self.v2ray_profiles.retain(|p| p.id != id);
                if self.active_profile_id.as_deref() == Some(&id) {
                    self.active_profile_id = self.v2ray_profiles.first().map(|p| p.id.clone());
                    if let Some(first) = self.v2ray_profiles.first() {
                        self.v2ray_uri_or_json = first.config.clone();
                    }
                }
                let _ = self.save_current_state();
                self.push_log("Storage", "Deleted profile from saved configuration.", LogLevel::Info);
                Task::none()
            }

            Message::ExportActiveXrayJson => {
                let socks_p: u16 = self.local_socks_port.parse().unwrap_or(10808);
                let http_p = socks_p.saturating_add(1);
                match crate::v2ray::parse_and_generate_xray_config(&self.v2ray_uri_or_json, socks_p, http_p, self.enable_tun) {
                    Ok(json_cfg) => {
                        match storage::save_active_xray_json(&json_cfg) {
                            Ok(path) => {
                                self.push_log(
                                    "Storage",
                                    format!("Exported active Xray config to {}", path.display()),
                                    LogLevel::Success,
                                );
                            }
                            Err(e) => {
                                self.push_log("Storage", format!("Failed to export Xray config: {}", e), LogLevel::Error);
                            }
                        }
                    }
                    Err(e) => {
                        self.push_log("Storage", format!("Config error: {}", e), LogLevel::Error);
                    }
                }
                Task::none()
            }

            Message::ToggleTunMode(v) => {
                self.enable_tun = v;
                let _ = self.save_current_state();
                self.push_log(
                    "Config",
                    if v { "TUN Mode enabled (virtual network interface)." } else { "TUN Mode disabled." },
                    LogLevel::Info,
                );
                Task::none()
            }

            Message::ToggleSysProxy(v) => {
                self.enable_sysproxy = v;
                let _ = self.save_current_state();
                self.push_log(
                    "Config",
                    if v { "GNOME System Proxy enabled." } else { "GNOME System Proxy disabled." },
                    LogLevel::Info,
                );
                Task::none()
            }

            Message::LoggerReady(sender) => {
                self.log_sender = Some(sender);
                Task::none()
            }

            Message::NewLog(entry) => {
                // Automatically flip UI back to DISCONNECTED if supervisor reports offline or fatal engine error
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
                    // Stop the running tunnel task
                    if let Some(stop_tx) = self.stop_signal.take() {
                        let _ = stop_tx.send(());
                    }
                    self.is_running = false;
                    self.push_log("Supervisor", "Tunnel stopped by user.", LogLevel::Warn);
                    Task::none()
                } else {
                    // Start the background tunnel task and stream logs to the UI
                    self.is_running = true;
                    self.active_tab = ActiveTab::Logs; // Switch to logs automatically like Netray

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

    // ========================================================================
    // 4. REAL-TIME LOG STREAM SUBSCRIPTION
    // ========================================================================

    fn subscription(&self) -> Subscription<Message> {
        Subscription::run( move || {
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
    // 5. ICED VIEW LAYOUT
    // ========================================================================

    fn view(&self) -> Element<'_, Message> {
        let sidebar = self.view_sidebar();

        let main_content = match self.active_tab {
            ActiveTab::Home => self.view_home_tab(),
            ActiveTab::PayloadSni => self.view_payload_tab(),
            ActiveTab::SshConfig => self.view_ssh_tab(),
            ActiveTab::V2RayConfig => self.view_v2ray_tab(),
            ActiveTab::Logs => self.view_logs_tab(),
        };

        row![
            sidebar,
            container(main_content)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(20)
        ]
        .into()
    }

    fn view_sidebar(&self) -> Element<'_, Message> {
        let status_color = if self.is_running {
            Color::from_rgb(0.2, 0.85, 0.4)
        } else {
            Color::from_rgb(0.9, 0.3, 0.3)
        };

        let start_btn_label = if self.is_running { "STOP TUNNEL" } else { "START TUNNEL" };

        column![
            text("NETRAY LINUX").size(22),
            text(if self.is_running { "● CONNECTED" } else { "● DISCONNECTED" })
                .color(status_color)
                .size(13),
            Space::with_height(10),
            pick_list(
                [
                    TunnelMode::SshPayload,
                    TunnelMode::SshSslSni,
                    TunnelMode::V2RayXray,
                    TunnelMode::SlowDns,
                ],
                Some(self.tunnel_mode),
                Message::TunnelModeChanged
            )
            .width(Length::Fill),
            Space::with_height(5),
            button(
                container(text("📋 Paste Clipboard").size(13))
                    .width(Length::Fill)
                    .align_x(Alignment::Center)
            )
            .on_press(Message::RequestClipboardImport)
            .width(Length::Fill),
            Space::with_height(8),
            checkbox("TUN Mode", self.enable_tun)
                .on_toggle(Message::ToggleTunMode)
                .size(13),
            checkbox("System Proxy", self.enable_sysproxy)
                .on_toggle(Message::ToggleSysProxy)
                .size(13),
            Space::with_height(10),
            button(text(format!("Profiles ({})", self.v2ray_profiles.len())).width(Length::Fill))
                .on_press(Message::TabSelected(ActiveTab::Home))
                .width(Length::Fill),
            button(text("Payload / SNI").width(Length::Fill))
                .on_press(Message::TabSelected(ActiveTab::PayloadSni))
                .width(Length::Fill),
            button(text("SSH / Proxy").width(Length::Fill))
                .on_press(Message::TabSelected(ActiveTab::SshConfig))
                .width(Length::Fill),
            button(text("V2Ray / Xray").width(Length::Fill))
                .on_press(Message::TabSelected(ActiveTab::V2RayConfig))
                .width(Length::Fill),
            button(text(format!("Logs ({})", self.logs.len())).width(Length::Fill))
                .on_press(Message::TabSelected(ActiveTab::Logs))
                .width(Length::Fill),
            Space::with_height(Length::Fill),
            button(
                container(text(start_btn_label).size(16))
                    .width(Length::Fill)
                    .align_x(Alignment::Center)
                    .padding(8)
            )
            .on_press(Message::ToggleTunnel)
            .width(Length::Fill),
        ]
        .spacing(8)
        .padding(16)
        .width(220)
        .into()
    }

    fn view_payload_tab(&self) -> Element<'_, Message> {
        column![
            text("HTTP Payload & SNI Injector").size(20),
            text("Configure custom HTTP headers ([crlf], [split], [host_port]) and TLS SNI bug hosts.").size(13),
            Space::with_height(10),
            text("Payload Template:"),
            text_input("Enter HTTP payload...", &self.payload_text)
                .on_input(Message::PayloadChanged)
                .padding(10),
            Space::with_height(10),
            row![
                column![
                    text("Bug Host / CDN Host:"),
                    text_input("cdn.host.com", &self.bug_host)
                        .on_input(Message::BugHostChanged)
                        .padding(8),
                ]
                .spacing(5)
                .width(Length::Fill),
                column![
                    text("TLS SNI (Server Name Indication):"),
                    text_input("sni.host.com", &self.sni_host)
                        .on_input(Message::SniChanged)
                        .padding(8),
                ]
                .spacing(5)
                .width(Length::Fill),
            ]
            .spacing(15),
            Space::with_height(10),
            text("Quick Payload Builder:"),
            row![
                checkbox("Keep-Alive", self.gen_keep_alive).on_toggle(Message::ToggleKeepAlive),
                checkbox("X-Online-Host", self.gen_online_host).on_toggle(Message::ToggleOnlineHost),
                checkbox("Use [split]", self.gen_split).on_toggle(Message::ToggleSplit),
                button("Generate Payload").on_press(Message::GeneratePayload),
            ]
            .spacing(20)
            .align_y(Alignment::Center),
        ]
        .spacing(10)
        .into()
    }

    fn view_ssh_tab(&self) -> Element<'_, Message> {
        column![
            text("SSH & Remote Proxy Configuration").size(20),
            row![
                column![
                    text("SSH Server Host:"),
                    text_input("sg.server.com", &self.ssh_host).on_input(Message::SshHostChanged).padding(8),
                ].width(Length::FillPortion(3)),
                column![
                    text("SSH Port:"),
                    text_input("22", &self.ssh_port).on_input(Message::SshPortChanged).padding(8),
                ].width(Length::FillPortion(1)),
            ].spacing(15),
            row![
                column![
                    text("Username:"),
                    text_input("user", &self.ssh_user).on_input(Message::SshUserChanged).padding(8),
                ].width(Length::Fill),
                column![
                    text("Password:"),
                    text_input("password", &self.ssh_pass).on_input(Message::SshPassChanged).secure(true).padding(8),
                ].width(Length::Fill),
            ].spacing(15),
            Space::with_height(10),
            text("HTTP Proxy / CDN Fronting IP:").size(16),
            row![
                column![
                    text("Proxy / Cloudflare IP:"),
                    text_input("104.16.x.x", &self.proxy_ip).on_input(Message::ProxyIpChanged).padding(8),
                ].width(Length::FillPortion(3)),
                column![
                    text("Proxy Port:"),
                    text_input("80", &self.proxy_port).on_input(Message::ProxyPortChanged).padding(8),
                ].width(Length::FillPortion(1)),
            ].spacing(15),
        ]
        .spacing(12)
        .into()
    }

    fn view_v2ray_tab(&self) -> Element<'_, Message> {
        let http_port = self
            .local_socks_port
            .parse::<u16>()
            .unwrap_or(10808)
            .saturating_add(1);

        let node_name = crate::v2ray::extract_node_name(&self.v2ray_uri_or_json);
        let config_path_display = storage::get_config_path().display().to_string();

        column![
            text("V2Ray / Xray Configuration").size(20),
            text(format!("Configuration auto-saved to: {}", config_path_display))
                .size(12)
                .color(Color::from_rgb(0.6, 0.6, 0.6)),
            Space::with_height(5),
            row![
                text(format!("Active Node: {}", node_name))
                    .size(14)
                    .color(Color::from_rgb(0.35, 0.85, 0.95)),
                Space::with_width(15),
                text(format!("({} profiles stored)", self.v2ray_profiles.len()))
                    .size(12)
                    .color(Color::from_rgb(0.5, 0.5, 0.5)),
            ].align_y(Alignment::Center),
            Space::with_height(5),
            text("Configuration URI / Raw Xray JSON:"),
            text_input("vless://... or vmess://... or Xray JSON", &self.v2ray_uri_or_json)
                .on_input(Message::V2RayConfigChanged)
                .padding(10),
            Space::with_height(10),
            row![
                column![
                    text("Local SOCKS5 Inbound Port:"),
                    text_input("10808", &self.local_socks_port)
                        .on_input(Message::LocalSocksPortChanged)
                        .padding(8),
                ]
                .width(Length::FillPortion(1)),
                column![
                    text("Local HTTP Inbound Port:"),
                    text(format!("127.0.0.1:{}", http_port))
                        .size(14)
                        .color(Color::from_rgb(0.3, 0.85, 0.95)),
                ]
                .width(Length::FillPortion(1)),
            ]
            .spacing(20)
            .align_y(Alignment::Center),
            Space::with_height(10),
            text("Routing Modes:").size(15),
            row![
                checkbox("TUN Mode (Virtual Network Interface / VPN)", self.enable_tun)
                    .on_toggle(Message::ToggleTunMode)
                    .size(14),
                checkbox("GNOME System Proxy (Direct App & Browser Routing)", self.enable_sysproxy)
                    .on_toggle(Message::ToggleSysProxy)
                    .size(14),
            ]
            .spacing(25),
            Space::with_height(15),
            row![
                button("📋 Import Clipboard").on_press(Message::RequestClipboardImport),
                button("💾 Save to Profiles").on_press(Message::SaveCurrentProfile),
                button("📂 Export Xray JSON").on_press(Message::ExportActiveXrayJson),
                button("✓ Validate Config").on_press(Message::ValidateV2RayConfig),
                button("Load Sample Node").on_press(Message::LoadSampleV2Ray),
            ]
            .spacing(12),
        ]
        .spacing(12)
        .into()
    }

    fn view_home_tab(&self) -> Element<'_, Message> {
        let profiles_count = self.v2ray_profiles.len();
        let config_path_display = storage::get_config_path().display().to_string();

        let profile_list: Vec<Element<Message>> = self
            .v2ray_profiles
            .iter()
            .map(|p| {
                let is_active = self.active_profile_id.as_deref() == Some(&p.id)
                    || self.v2ray_uri_or_json == p.config;
                let active_badge = if is_active {
                    text("● ACTIVE").size(12).color(Color::from_rgb(0.2, 0.9, 0.4))
                } else {
                    text("IDLE").size(12).color(Color::from_rgb(0.5, 0.5, 0.5))
                };

                let proto_color = if p.config.starts_with("vless://") {
                    Color::from_rgb(0.3, 0.8, 0.95)
                } else if p.config.starts_with("vmess://") {
                    Color::from_rgb(0.9, 0.5, 0.9)
                } else if p.config.starts_with("trojan://") {
                    Color::from_rgb(0.95, 0.7, 0.3)
                } else {
                    Color::from_rgb(0.5, 0.8, 0.5)
                };

                let proto_name = if p.config.starts_with("vless://") {
                    "VLESS"
                } else if p.config.starts_with("vmess://") {
                    "VMESS"
                } else if p.config.starts_with("trojan://") {
                    "TROJAN"
                } else if p.config.starts_with("ss://") {
                    "SHADOWSOCKS"
                } else {
                    "JSON"
                };

                let id_clone_1 = p.id.clone();
                let id_clone_2 = p.id.clone();

                container(
                    row![
                        column![
                            row![
                                text(&p.name).size(16).color(Color::WHITE),
                                Space::with_width(10),
                                text(format!("[{}]", proto_name)).size(12).color(proto_color),
                                Space::with_width(10),
                                active_badge,
                            ].align_y(Alignment::Center),
                            Space::with_height(3),
                            text(&p.created_at).size(11).color(Color::from_rgb(0.5, 0.5, 0.5)),
                        ].width(Length::Fill),
                        row![
                            button("▶ Activate").on_press(Message::SelectProfile(id_clone_1)),
                            button("🗑 Delete").on_press(Message::DeleteProfile(id_clone_2)),
                        ].spacing(10),
                    ]
                    .align_y(Alignment::Center)
                    .padding(10),
                )
                .width(Length::Fill)
                .into()
            })
            .collect();

        column![
            text("Saved V2Ray / Xray Profiles").size(22),
            text(format!("Configuration file: {}", config_path_display))
                .size(12)
                .color(Color::from_rgb(0.6, 0.6, 0.6)),
            Space::with_height(10),
            row![
                button("📋 Import from Clipboard").on_press(Message::RequestClipboardImport),
                button("💾 Save Current Config as Profile").on_press(Message::SaveCurrentProfile),
                button("📂 Export Active Xray JSON").on_press(Message::ExportActiveXrayJson),
            ].spacing(12),
            Space::with_height(15),
            text(format!("Stored Profiles ({})", profiles_count)).size(15),
            if profile_list.is_empty() {
                Element::from(text("No saved profiles. Click 'Import from Clipboard' or paste a link to save your first profile.").color(Color::from_rgb(0.6, 0.6, 0.6)))
            } else {
                scrollable(column(profile_list).spacing(8)).height(Length::Fill).into()
            }
        ]
        .spacing(10)
        .into()
    }

    fn view_logs_tab(&self) -> Element<'_, Message> {
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
                        .size(13)
                        .color(Color::from_rgb(0.5, 0.5, 0.5)),
                    text(format!("[{}]", entry.source))
                        .size(13)
                        .color(Color::from_rgb(0.4, 0.75, 0.95)),
                    text(&entry.message).size(13).color(color),
                ]
                .spacing(8)
                .into()
            })
            .collect();

        column![
            row![
                text("Real-Time Tunnel Logs").size(20),
                Space::with_width(Length::Fill),
                button("Clear Logs").on_press(Message::ClearLogs),
            ]
            .align_y(Alignment::Center),
            scrollable(
                column(log_rows)
                    .spacing(4)
                    .width(Length::Fill)
                    .padding(10)
            )
            .id(self.log_scroll_id.clone())
            .height(Length::Fill)
            .width(Length::Fill),
        ]
        .spacing(12)
        .into()
    }
}

// ============================================================================
// 6. ASYNC BACKGROUND TUNNEL WORKER (FEEDS LIVE LOGS TO UI)
// ============================================================================

pub async fn send_log(
    tx: &mpsc::UnboundedSender<LogEntry>,
    source: &str,
    msg: impl Into<String>,
    level: LogLevel,
) {
    let _ = tx.send(LogEntry {
        timestamp: Local::now().format("%H:%M:%S").to_string(),
        source: source.to_string(),
        message: msg.into(),
        level,
    });
}