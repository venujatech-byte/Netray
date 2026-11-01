use crate::{send_log, LogEntry, LogLevel, TunnelMode};
use async_trait::async_trait;
use russh::client;
use russh_keys::key;
use rustls_pki_types::ServerName;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_rustls::{rustls, TlsConnector};

#[derive(Debug, Default)]
pub struct TunnelMetrics {
    pub active_connections: AtomicUsize,
    pub total_proxied_connections: AtomicU64,
    pub total_bypassed_connections: AtomicU64,
}

static METRICS: TunnelMetrics = TunnelMetrics {
    active_connections: AtomicUsize::new(0),
    total_proxied_connections: AtomicU64::new(0),
    total_bypassed_connections: AtomicU64::new(0),
};

/// Returns (active_connections, total_proxied, total_bypassed)
pub fn get_tunnel_metrics() -> (usize, u64, u64) {
    (
        METRICS.active_connections.load(Ordering::Relaxed),
        METRICS.total_proxied_connections.load(Ordering::Relaxed),
        METRICS.total_bypassed_connections.load(Ordering::Relaxed),
    )
}

pub fn record_connection_start(is_direct: bool) {
    METRICS.active_connections.fetch_add(1, Ordering::Relaxed);
    if is_direct {
        METRICS.total_bypassed_connections.fetch_add(1, Ordering::Relaxed);
    } else {
        METRICS.total_proxied_connections.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn record_connection_end() {
    let mut current = METRICS.active_connections.load(Ordering::Relaxed);
    while current > 0 {
        match METRICS.active_connections.compare_exchange_weak(
            current,
            current - 1,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(actual) => current = actual,
        }
    }
}

pub fn reset_tunnel_metrics() {
    METRICS.active_connections.store(0, Ordering::Relaxed);
    METRICS.total_proxied_connections.store(0, Ordering::Relaxed);
    METRICS.total_bypassed_connections.store(0, Ordering::Relaxed);
}

#[derive(Clone)]
pub struct TunnelConfig {
    pub mode: TunnelMode,
    pub proxy_addr: String,
    pub ssh_host: String,
    pub ssh_port: u16,
    pub ssh_user: String,
    pub ssh_pass: String,
    pub bug_host: String,
    pub sni_host: String,
    pub payload_template: String,
    pub local_socks_port: u16,
    pub v2ray_config: String,
    pub enable_tun: bool,
    pub enable_sysproxy: bool,
    pub whitelist_apps: String,
    pub whitelist_urls: String,
}

struct SshClientHandler;

#[async_trait]
impl client::Handler for SshClientHandler {
    type Error = russh::Error;
    async fn check_server_key(&mut self, _key: &key::PublicKey) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

pub async fn run_real_tunnel_worker(
    log_tx: mpsc::Sender<LogEntry>,
    mut stop_rx: tokio::sync::oneshot::Receiver<()>,
    cfg: TunnelConfig,
) {
    send_log(
        &log_tx,
        "Supervisor",
        format!("Starting engine in [{}] mode on Windows...", cfg.mode),
        LogLevel::Info,
    )
    .await;

    let result = match cfg.mode {
        TunnelMode::SshPayload => {
            tokio::select! {
                res = run_ssh_payload_engine(&log_tx, &cfg, false) => res,
                _ = &mut stop_rx => {
                    send_log(&log_tx, "Supervisor", "Tunnel stopped by user. All sockets closed.", LogLevel::Warn).await;
                    Ok(())
                }
            }
        }
        TunnelMode::SshSslSni => {
            tokio::select! {
                res = run_ssh_payload_engine(&log_tx, &cfg, true) => res,
                _ = &mut stop_rx => {
                    send_log(&log_tx, "Supervisor", "Tunnel stopped by user. All sockets closed.", LogLevel::Warn).await;
                    Ok(())
                }
            }
        }
        TunnelMode::V2RayXray => run_xray_engine(&log_tx, &cfg, &mut stop_rx).await,
        TunnelMode::SlowDns => Err("SlowDNS wrapper not yet configured on Windows.".into()),
    };

    if let Err(e) = result {
        send_log(&log_tx, "Error", format!("Engine failed: {}", e), LogLevel::Error).await;
    }

    if cfg.enable_sysproxy {
        crate::sysproxy::disable_system_proxy().await;
        send_log(&log_tx, "SystemProxy", "System proxy disabled. Network restored.", LogLevel::Info).await;
    }

    reset_tunnel_metrics();
    send_log(&log_tx, "Supervisor", "Tunnel is offline.", LogLevel::Info).await;
}

async fn run_ssh_payload_engine(
    log_tx: &mpsc::Sender<LogEntry>,
    cfg: &TunnelConfig,
    use_ssl: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let target_tcp = if cfg.proxy_addr.trim().is_empty() {
        format!("{}:{}", cfg.ssh_host, cfg.ssh_port)
    } else {
        cfg.proxy_addr.trim().to_string()
    };

    send_log(log_tx, "Injector", format!("Connecting TCP to {}...", target_tcp), LogLevel::Info).await;
    let mut tcp_stream = TcpStream::connect(&target_tcp).await?;

    if use_ssl {
        send_log(log_tx, "SSL", format!("Initiating TLS handshake (SNI: {})...", cfg.sni_host), LogLevel::Info).await;
        let root_store = rustls::RootCertStore::empty();
        let client_config = rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        let connector = TlsConnector::from(Arc::new(client_config));
        let domain_str = if cfg.sni_host.is_empty() { cfg.ssh_host.clone() } else { cfg.sni_host.clone() };
        let server_name = ServerName::try_from(domain_str.as_str())?.to_owned();
        let mut tls_stream = connector.connect(server_name, tcp_stream).await?;

        if !cfg.payload_template.is_empty() {
            let req = cfg.payload_template
                .replace("[host_port]", &format!("{}:{}", cfg.ssh_host, cfg.ssh_port))
                .replace("[host]", &cfg.bug_host)
                .replace("[crlf]", "\r\n");
            tls_stream.write_all(req.as_bytes()).await?;
            tls_stream.flush().await?;
        }
    } else if !cfg.payload_template.is_empty() {
        let req = cfg.payload_template
            .replace("[host_port]", &format!("{}:{}", cfg.ssh_host, cfg.ssh_port))
            .replace("[host]", &cfg.bug_host)
            .replace("[crlf]", "\r\n");
        tcp_stream.write_all(req.as_bytes()).await?;
        tcp_stream.flush().await?;
        let mut resp_buf = [0u8; 1024];
        let n = tcp_stream.read(&mut resp_buf).await?;
        let status_line = String::from_utf8_lossy(&resp_buf[..n]);
        send_log(log_tx, "Injector", format!("Server replied: {}", status_line.lines().next().unwrap_or("")), LogLevel::Success).await;
    }

    send_log(log_tx, "SSH", "Starting SSH handshake...", LogLevel::Info).await;
    let client_config = Arc::new(client::Config::default());
    let mut session = client::connect(client_config, (&cfg.ssh_host as &str, cfg.ssh_port), SshClientHandler).await?;

    let auth_ok = session.authenticate_password(&cfg.ssh_user, &cfg.ssh_pass).await?;
    if !auth_ok {
        return Err("SSH Authentication failed.".into());
    }
    send_log(log_tx, "SSH", "SSH Authenticated successfully!", LogLevel::Success).await;

    let shared_ssh = Arc::new(session);
    let socks_bind = format!("127.0.0.1:{}", cfg.local_socks_port);
    let listener = TcpListener::bind(&socks_bind).await?;
    send_log(log_tx, "SOCKS5", format!("Listening on {} -> Tunneling via SSH", socks_bind), LogLevel::Success).await;

    // Own client tasks so stopping the tunnel also releases their sockets and SSH handles.
    let mut clients = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (client_socket, _) = accepted?;
                let ssh_handle = Arc::clone(&shared_ssh);
                let log_tx_clone = log_tx.clone();
                clients.spawn(async move {
                    if let Err(e) = handle_socks5_client(client_socket, ssh_handle).await {
                        send_log(&log_tx_clone, "SOCKS5", format!("Stream closed: {}", e), LogLevel::Warn).await;
                    }
                });
            }
            Some(_) = clients.join_next(), if !clients.is_empty() => {}
        }
    }
}

async fn handle_socks5_client(
    mut sock: TcpStream,
    ssh: Arc<client::Handle<SshClientHandler>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut buf = [0u8; 512];
    sock.read_exact(&mut buf[..2]).await?;
    let nmethods = buf[1] as usize;
    sock.read_exact(&mut buf[..nmethods]).await?;
    sock.write_all(&[0x05, 0x00]).await?;

    sock.read_exact(&mut buf[..4]).await?;
    if buf[1] != 0x01 {
        return Err("Only SOCKS5 CONNECT is supported".into());
    }

    let target_host = match buf[3] {
        0x01 => {
            let mut ip_bytes = [0u8; 4];
            sock.read_exact(&mut ip_bytes).await?;
            std::net::Ipv4Addr::from(ip_bytes).to_string()
        }
        0x03 => {
            let mut len_byte = [0u8; 1];
            sock.read_exact(&mut len_byte).await?;
            let len = len_byte[0] as usize;
            let mut domain_buf = vec![0u8; len];
            sock.read_exact(&mut domain_buf).await?;
            String::from_utf8(domain_buf)?
        }
        _ => return Err("Unsupported address type".into()),
    };

    let mut port_bytes = [0u8; 2];
    sock.read_exact(&mut port_bytes).await?;
    let target_port = u16::from_be_bytes(port_bytes);

    let channel = ssh.channel_open_direct_tcpip(
        &target_host,
        target_port as u32,
        "127.0.0.1",
        1080,
    ).await?;


    sock.write_all(&[0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0x04, 0x38]).await?;

    let (mut sock_rx, mut sock_tx) = sock.split();
    let mut chan_stream = channel.into_stream();
    let (mut chan_rx, mut chan_tx) = tokio::io::split(&mut chan_stream);

    let client_to_ssh = tokio::io::copy(&mut sock_rx, &mut chan_tx);
    let ssh_to_client = tokio::io::copy(&mut chan_rx, &mut sock_tx);
    tokio::select! {
        _ = client_to_ssh => {}
        _ = ssh_to_client => {}
    }
    Ok(())
}

fn classify_xray_log(line: &str) -> LogLevel {
    if line.contains("[Warning]") {
        LogLevel::Warn
    } else if line.contains("[Error]") {
        LogLevel::Error
    } else if line.contains("failed") || line.contains("Failed") {
        LogLevel::Warn
    } else {
        LogLevel::Info
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XrayConnectionInfo {
    pub client_port: Option<u16>,
    pub target_host: String,
    pub target_port: u16,
    pub protocol: String,
    pub route: String,
}

pub fn parse_xray_connection(line: &str) -> Option<XrayConnectionInfo> {
    if let Some((left, right)) = line.split_once(" accepted ") {
        let client_port = left.rsplit_once(':').and_then(|(_, p)| p.trim().parse::<u16>().ok());
        let mut tokens = right.split_whitespace();
        let target_token = tokens.next()?;
        let route_raw = tokens.next().unwrap_or("[proxy]");
        let route = route_raw.trim_matches(&['[', ']'][..]).to_string();

        let parts: Vec<&str> = target_token.split(':').collect();
        if parts.len() >= 3 {
            let protocol = parts[0].to_string();
            let port_str = parts[parts.len() - 1];
            let host = parts[1..parts.len() - 1].join(":");
            if let Ok(port) = port_str.parse::<u16>() {
                if !host.is_empty() {
                    return Some(XrayConnectionInfo {
                        client_port,
                        target_host: host,
                        target_port: port,
                        protocol,
                        route,
                    });
                }
            }
        }
    }
    None
}

fn strip_xray_log_prefix(line: &str) -> &str {
    let trimmed = line.trim();
    if trimmed.len() > 20
        && trimmed.as_bytes().get(4) == Some(&b'/')
        && trimmed.as_bytes().get(7) == Some(&b'/')
    {
        if let Some(bracket_end) = trimmed.find("] ") {
            return trimmed[bracket_end + 2..].trim();
        }
    }
    trimmed
}

fn spawn_xray_log_pipe<R: tokio::io::AsyncRead + Unpin + Send + 'static>(
    stream: R,
    tx: mpsc::Sender<LogEntry>,
) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stream).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let lvl = classify_xray_log(&line);
            let cleaned = strip_xray_log_prefix(&line);
            if let Some(conn) = parse_xray_connection(cleaned) {
                let is_direct = conn.route.eq_ignore_ascii_case("direct") || conn.route.eq_ignore_ascii_case("bypass");
                record_connection_start(is_direct);
                crate::send_connection_log(
                    &tx,
                    "Xray",
                    cleaned,
                    lvl,
                    &conn.target_host,
                    conn.target_port,
                    None,
                    Some(conn.route),
                ).await;
            } else {
                send_log(&tx, "Xray", cleaned, lvl).await;
            }
        }
    });
}

async fn run_xray_engine(
    log_tx: &mpsc::Sender<LogEntry>,
    cfg: &TunnelConfig,
    stop_rx: &mut tokio::sync::oneshot::Receiver<()>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // 0. Locate xray.exe
    let xray_bin = match crate::v2ray::find_xray_binary() {
        Ok(bin) => bin,
        Err(e) => {
            send_log(log_tx, "Xray", &e, LogLevel::Error).await;
            return Err(e.into());
        }
    };
    send_log(log_tx, "Xray", format!("Found core binary at: {}", xray_bin.display()), LogLevel::Info).await;

    // 2. Generate configuration
    let http_port = cfg.local_socks_port.saturating_add(1);
    let config_json = crate::v2ray::parse_and_generate_xray_config_with_whitelist(
        &cfg.v2ray_config,
        cfg.local_socks_port,
        http_port,
        cfg.enable_tun,
        &cfg.whitelist_apps,
        &cfg.whitelist_urls,
    )?;

    let config_json = if cfg.enable_tun {
        let mut config: serde_json::Value = serde_json::from_str(&config_json)?;
        if let Err(error) = crate::v2ray::pin_proxy_dns(&mut config).await {
            send_log(log_tx, "DNS", &error, LogLevel::Error).await;
            return Err(error.into());
        }
        serde_json::to_string(&config)?
    } else { config_json };

    // 3. Write temp config
    let config_dir = crate::storage::get_config_dir();
    let config_path = config_dir.join("active_xray.json");
    tokio::fs::write(&config_path, &config_json).await?;

    // 4. Enable System Proxy if requested
    if cfg.enable_sysproxy {
        send_log(log_tx, "SystemProxy", "Configuring Windows system proxy in Registry...", LogLevel::Info).await;
        let _ = crate::sysproxy::enable_system_proxy(cfg.local_socks_port, http_port).await;
        send_log(log_tx, "SystemProxy", format!("Proxy active -> SOCKS5 127.0.0.1:{}, HTTP 127.0.0.1:{}", cfg.local_socks_port, http_port), LogLevel::Success).await;
    }

    send_log(log_tx, "Xray", "Spawning Xray process...", LogLevel::Info).await;

    let mut cmd = Command::new(&xray_bin);
    if let Some(dir) = xray_bin.parent() {
        cmd.current_dir(dir);
    }
    cmd.args(["run", "-c", &config_path.to_string_lossy()])
       .stdout(Stdio::piped())
       .stderr(Stdio::piped())
       .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);

    let mut child = cmd.spawn()?;

    if let Some(stdout) = child.stdout.take() {
        spawn_xray_log_pipe(stdout, log_tx.clone());
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_xray_log_pipe(stderr, log_tx.clone());
    }

    send_log(
        log_tx,
        "Xray",
        format!("Checking https://www.gstatic.com/generate_204 through HTTP proxy 127.0.0.1:{http_port}; connection requires HTTP 204."),
        LogLevel::Info,
    )
    .await;
    let ping_ms = tokio::select! {
        result = crate::v2ray::wait_for_http_proxy(http_port) => match result {
            Ok(ms) => Some(ms),
            Err(error) => {
                send_log(log_tx, "Error", format!("Connection check failed: {error}"), LogLevel::Error).await;
                None
            }
        },
        status = child.wait() => {
            match status {
                Ok(status) => send_log(log_tx, "Error", format!("Xray exited before connection verification: {status}"), LogLevel::Error).await,
                Err(error) => send_log(log_tx, "Error", format!("Could not wait for Xray during connection verification: {error}"), LogLevel::Error).await,
            }
            None
        },
        _ = &mut *stop_rx => {
            send_log(log_tx, "Supervisor", "Connection check cancelled; stopping Xray.", LogLevel::Warn).await;
            let _ = child.kill().await;
            None
        }
    };
    if let Some(ms) = ping_ms {
        send_log(log_tx, "Supervisor", format!("Tunnel ready · HTTP 204 through proxy in {ms} ms."), LogLevel::Success).await;
    }

    if ping_ms.is_some() { tokio::select! {
        res = child.wait() => {
            let status = res?;
            send_log(log_tx, "Xray", format!("Core process exited: {}", status), LogLevel::Warn).await;
        }
        _ = &mut *stop_rx => {
            send_log(log_tx, "Supervisor", "Stopping Xray core and tearing down WinTun interface...", LogLevel::Info).await;
            let _ = child.kill().await;
        }
    }; } else { let _ = child.kill().await; }

    let _ = tokio::fs::remove_file(&config_path).await;
    Ok(())
}
