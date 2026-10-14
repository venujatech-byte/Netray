mod tunnel;
use tunnel::{run_real_tunnel_worker, TunnelConfig};
use chrono::Local;
use iced::futures::SinkExt;
use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, text, text_input, Space,
};
use iced::{Alignment, Color, Element, Length, Subscription, Task, Theme};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

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

    // Real-time Logger Events
    LoggerReady(mpsc::UnboundedSender<LogEntry>),
    NewLog(LogEntry),
    ClearLogs,
}

impl NetrayApp {
    fn new() -> (Self, Task<Message>) {
        let app = Self {
            active_tab: ActiveTab::PayloadSni,
            is_running: false,
            tunnel_mode: TunnelMode::SshPayload,

            payload_text: "GET / HTTP/1.1[crlf]Host: [host][crlf]Upgrade: websocket[crlf][crlf]"
                .to_string(),
            bug_host: "cdn.bug-example.com".to_string(),
            sni_host: "cdn.bug-example.com".to_string(),
            gen_keep_alive: true,
            gen_online_host: true,
            gen_split: false,

            ssh_host: "sg1.myserver.com".to_string(),
            ssh_port: "22".to_string(),
            ssh_user: "netray".to_string(),
            ssh_pass: "".to_string(),
            proxy_ip: "104.16.51.111".to_string(),
            proxy_port: "80".to_string(),
            local_socks_port: "10808".to_string(),

            v2ray_uri_or_json: "vless://uuid@server:443?security=tls&sni=bug.com#LinuxNode"
                .to_string(),

            logs: vec![LogEntry {
                timestamp: Local::now().format("%H:%M:%S").to_string(),
                source: "System".into(),
                message: "Netray Linux initialized. Ready to start tunnel.".into(),
                level: LogLevel::Info,
            }],
            log_sender: None,
            stop_signal: None,
            log_scroll_id: scrollable::Id::unique(),
        };

        (app, Task::none())
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

            Message::LoggerReady(sender) => {
                self.log_sender = Some(sender);
                Task::none()
            }

            Message::NewLog(entry) => {
                self.logs.push(entry);
                // Keep log buffer capped at 1000 lines for low memory usage
                if self.logs.len() > 1000 {
                    self.logs.remove(0);
                }
                // Auto-scroll to bottom of the log viewer
                scrollable::snap_to(self.log_scroll_id.clone(), scrollable::RelativeOffset::END)
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

    fn view(&self) -> Element<Message> {
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

    fn view_sidebar(&self) -> Element<Message> {
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
            Space::with_height(15),
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
            Space::with_height(15),
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
        .spacing(10)
        .padding(16)
        .width(220)
        .into()
    }

    fn view_payload_tab(&self) -> Element<Message> {
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

    fn view_ssh_tab(&self) -> Element<Message> {
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

    fn view_v2ray_tab(&self) -> Element<Message> {
        column![
            text("V2Ray / Xray Configuration").size(20),
            text("Paste a vmess://, vless://, trojan:// URI or raw Xray JSON config:"),
            text_input("vless://...", &self.v2ray_uri_or_json)
                .on_input(Message::V2RayConfigChanged)
                .padding(10),
        ]
        .spacing(12)
        .into()
    }

    fn view_home_tab(&self) -> Element<Message> {
        column![text("Profiles").size(20)].into()
    }

    fn view_logs_tab(&self) -> Element<Message> {
        let log_rows: Vec<Element<Message>> = self
            .logs
            .iter()
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

async fn send_log(
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

async fn run_tunnel_worker(
    log_tx: mpsc::UnboundedSender<LogEntry>,
    mut stop_rx: tokio::sync::oneshot::Receiver<()>,
    mode: TunnelMode,
    proxy_addr: String,
    ssh_target: String,
    bug_host: String,
    socks_port: String,
) {
    send_log(&log_tx, "Supervisor", format!("Starting engine in [{}] mode...", mode), LogLevel::Info).await;

    // Step-by-step lifecycle simulation (replace with your TcpStream / russh / xray Command calls)
    let work = async {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        send_log(&log_tx, "Injector", format!("Connecting to remote proxy {}...", proxy_addr), LogLevel::Info).await;

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        send_log(&log_tx, "Injector", format!("Injecting HTTP Payload (Host: {})...", bug_host), LogLevel::Info).await;

        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        send_log(&log_tx, "Injector", "HTTP/1.1 101 Switching Protocols received!", LogLevel::Success).await;

        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        send_log(&log_tx, "SSH", format!("Starting SSH handshake to {} over injected stream...", ssh_target), LogLevel::Info).await;

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        send_log(&log_tx, "SOCKS5", format!("Local SOCKS5 listening on 127.0.0.1:{}", socks_port), LogLevel::Success).await;

        // Keep alive loop until stopped
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            send_log(&log_tx, "KeepAlive", "SSH tunnel heartbeat OK (ping: 42ms)", LogLevel::Info).await;
        }
    };

    tokio::select! {
        _ = work => {}
        _ = &mut stop_rx => {
            send_log(&log_tx, "Supervisor", "Aborted active sockets and cleaned up ports.", LogLevel::Warn).await;
        }
    }
}