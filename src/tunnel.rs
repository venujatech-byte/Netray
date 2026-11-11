use crate::{send_log, LogEntry, LogLevel, TunnelMode};
use async_trait::async_trait;
use russh::client;
use russh_keys::key;
use rustls_pki_types::ServerName;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

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

// Balance accounting on early I/O errors and cancelled relay futures too.
struct ActiveConnection;
impl Drop for ActiveConnection {
    fn drop(&mut self) {
        record_connection_end();
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

#[allow(dead_code)]
pub fn reset_tunnel_metrics() {
    METRICS.active_connections.store(0, Ordering::Relaxed);
    METRICS.total_proxied_connections.store(0, Ordering::Relaxed);
    METRICS.total_bypassed_connections.store(0, Ordering::Relaxed);
}
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_rustls::{rustls, TlsConnector};

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
    pub allow_insecure_tls: bool,
    pub ipv6_enabled: bool,
    pub bypass_lan: bool,
    pub dns_servers: String,
}

// Minimal russh client handler (accepts server host keys)
struct SshClientHandler;

#[async_trait]
impl client::Handler for SshClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}





// ============================================================================
// MASTER WORKER ENTRY POINT
// ============================================================================

pub async fn run_real_tunnel_worker(
    log_tx: mpsc::Sender<LogEntry>,
    mut stop_rx: tokio::sync::oneshot::Receiver<()>,
    cfg: TunnelConfig,
) {
    send_log(
        &log_tx,
        "Supervisor",
        format!("Starting engine in [{}] mode...", cfg.mode),
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
        TunnelMode::SlowDns => Err("SlowDNS binary wrapper not yet configured.".into()),
    };

    if let Err(e) = result {
        send_log(&log_tx, "Error", format!("Engine failed: {}", e), LogLevel::Error).await;
    }

    if cfg.enable_sysproxy {
        crate::sysproxy::disable_system_proxy().await;
        send_log(&log_tx, "SystemProxy", "System proxy disabled. Network restored.", LogLevel::Info).await;
    }

    if cfg.enable_tun {
        let _ = Command::new("ip").args(["link", "delete", "netray0"]).output().await;
    }

    reset_tunnel_metrics();
    send_log(&log_tx, "Supervisor", "Tunnel is offline.", LogLevel::Info).await;
}






// ============================================================================
// ENGINE 1: PAYLOAD INJECTOR + TLS SNI + RUSSH + LOCAL SOCKS5 SERVER
// ============================================================================

async fn run_ssh_payload_engine(
    log_tx: &mpsc::Sender<LogEntry>,
    cfg: &TunnelConfig,
    use_tls_sni: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // 1. Connect to Proxy / CDN IP (or directly to SSH host if proxy is empty)
    let target_tcp = if cfg.proxy_addr.trim().is_empty() || cfg.proxy_addr.starts_with(':') {
        format!("{}:{}", cfg.ssh_host, cfg.ssh_port)
    } else {
        cfg.proxy_addr.clone()
    };

    send_log(log_tx, "Injector", format!("Connecting TCP to {}...", target_tcp), LogLevel::Info).await;
    let mut stream = TcpStream::connect(&target_tcp).await?;

    // 2. Inject Custom HTTP Payload if provided
    if !cfg.payload_template.trim().is_empty() {
        let ssh_target = format!("{}:{}", cfg.ssh_host, cfg.ssh_port);
        let formatted_payload = cfg
            .payload_template
            .replace("[host_port]", &ssh_target)
            .replace("[host]", &cfg.bug_host)
            .replace("[sni]", &cfg.sni_host)
            .replace("[crlf]", "\r\n")
            .replace("[protocol]", "HTTP/1.1");

        send_log(
            log_tx,
            "Injector",
            format!("Sending Payload (Bug Host: {})...", cfg.bug_host),
            LogLevel::Info,
        )
        .await;

        for (i, part) in formatted_payload.split("[split]").enumerate() {
            if i > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            }
            stream.write_all(part.as_bytes()).await?;
            stream.flush().await?;
        }

        // Read HTTP proxy / CDN status response
        let mut buf = [0u8; 1024];
        let n = stream.read(&mut buf).await?;
        let response = String::from_utf8_lossy(&buf[..n]);
        let status_line = response.lines().next().unwrap_or("Empty response");

        send_log(log_tx, "Injector", format!("Server replied: {}", status_line), LogLevel::Success).await;
    }

    // 3. Hand stream over to russh (Plain TCP or TLS-wrapped with custom SNI)
    let ssh_config = Arc::new(client::Config::default());
    send_log(log_tx, "SSH", "Starting SSH handshake...", LogLevel::Info).await;

    let mut ssh_session = if use_tls_sni {
        send_log(
            log_tx,
            "SSL/SNI",
            format!("Wrapping stream in TLS with SNI: {}", cfg.sni_host),
            LogLevel::Info,
        )
        .await;

        let root_store = rustls::RootCertStore::empty();
        // In production, load webpki-roots or a custom insecure verifier for self-signed VPS certs
        let tls_cfg = rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        let connector = TlsConnector::from(Arc::new(tls_cfg));
        let domain = ServerName::try_from(cfg.sni_host.clone())?;
        let tls_stream = connector.connect(domain, stream).await?;

        client::connect_stream(ssh_config, tls_stream, SshClientHandler).await?
    } else {
        client::connect_stream(ssh_config, stream, SshClientHandler).await?
    };

    // 4. Authenticate SSH User/Pass
    let auth_ok = ssh_session
        .authenticate_password(&cfg.ssh_user, &cfg.ssh_pass)
        .await?;

    if !auth_ok {
        return Err("SSH Password Authentication failed!".into());
    }
    send_log(log_tx, "SSH", "SSH Authenticated successfully!", LogLevel::Success).await;

    let shared_ssh = Arc::new(ssh_session);

    // 5. Start Local SOCKS5 Listener
    let socks_bind = format!("127.0.0.1:{}", cfg.local_socks_port);
    let listener = TcpListener::bind(&socks_bind).await?;
    send_log(
        log_tx,
        "SOCKS5",
        format!("Listening on {} -> Tunneling via SSH", socks_bind),
        LogLevel::Success,
    )
    .await;

    // Own client tasks so stopping the tunnel also releases their sockets and SSH handles.
    let mut clients = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (client_socket, client_addr) = accepted?;
                let ssh_handle = Arc::clone(&shared_ssh);
                let log_tx_clone = log_tx.clone();
                let log_tx_err = log_tx.clone();
                clients.spawn(async move {
                    if let Err(e) = handle_socks5_client(client_socket, client_addr, ssh_handle, log_tx_clone).await {
                        // ignore normal client closes
                        let err_str = e.to_string();
                        if !err_str.contains("Broken pipe") && !err_str.contains("Connection reset") {
                            send_log(&log_tx_err, "SOCKS5", format!("Stream closed: {}", err_str), LogLevel::Warn).await;
                        }
                    }
                });
            }
            Some(_) = clients.join_next(), if !clients.is_empty() => {}
        }
    }
}

/// Minimal RFC 1928 SOCKS5 server that pipes each request into a russh `direct-tcpip` channel
async fn handle_socks5_client(
    mut sock: TcpStream,
    client_addr: std::net::SocketAddr,
    ssh: Arc<client::Handle<SshClientHandler>>,
    log_tx: mpsc::Sender<LogEntry>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut buf = [0u8; 512];

    // 1. Greeting: Client sends [VER=0x05, NMETHODS, METHODS...]
    sock.read_exact(&mut buf[..2]).await?;
    let nmethods = buf[1] as usize;
    sock.read_exact(&mut buf[..nmethods]).await?;
    // Reply: [VER=0x05, METHOD=0x00 (No Auth)]
    sock.write_all(&[0x05, 0x00]).await?;

    // 2. Request Header: [VER=0x05, CMD=0x01 (CONNECT), RSV=0x00, ATYP]
    sock.read_exact(&mut buf[..4]).await?;
    if buf[1] != 0x01 {
        return Err("Only SOCKS5 CONNECT is supported".into());
    }

    let target_host = match buf[3] {
        0x01 => {
            // IPv4
            sock.read_exact(&mut buf[..4]).await?;
            format!("{}.{}.{}.{}", buf[0], buf[1], buf[2], buf[3])
        }
        0x03 => {
            // Domain Name
            sock.read_exact(&mut buf[..1]).await?;
            let len = buf[0] as usize;
            sock.read_exact(&mut buf[..len]).await?;
            String::from_utf8_lossy(&buf[..len]).to_string()
        }
        _ => return Err("Unsupported address type".into()),
    };

    sock.read_exact(&mut buf[..2]).await?;
    let target_port = u16::from_be_bytes([buf[0], buf[1]]);

    let app_name = if crate::log_activity::detailed() {
        let port = client_addr.port();
        tokio::task::spawn_blocking(move || crate::apps::resolve_socket_process_name(port))
            .await.ok().flatten()
    } else { None };
    let app_display = app_name.as_deref().unwrap_or("app");

    record_connection_start(false);
    let _active_connection = ActiveConnection;

    crate::send_connection_log(
        &log_tx,
        "SOCKS5",
        format!("Connecting: [{}] -> {}:{} via SSH", app_display, target_host, target_port),
        LogLevel::Info,
        &target_host,
        target_port,
        app_name.clone(),
        Some("Tunnel".into()),
    )
    .await;

    // 3. Open SSH `direct-tcpip` channel to destination
    let channel_result = ssh
        .channel_open_direct_tcpip(&target_host, target_port as u32, "127.0.0.1", 10808)
        .await;

    let channel = match channel_result {
        Ok(c) => c,
        Err(e) => {
            crate::send_connection_log(
                &log_tx,
                "SOCKS5",
                format!("Failed: [{}] -> {}:{} ({})", app_display, target_host, target_port, e),
                LogLevel::Error,
                &target_host,
                target_port,
                app_name,
                Some("Failed".into()),
            )
            .await;
            return Err(e.into());
        }
    };

    // 4. Reply SOCKS5 Success
    sock.write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0]).await?;

    // 5. Bi-directional zero-copy relay between local app and SSH channel
    let mut ssh_stream = channel.into_stream();
    let relay_res = tokio::io::copy_bidirectional(&mut sock, &mut ssh_stream).await;

    if let Ok((upload, download)) = relay_res {
        crate::storage::record_usage(upload, download);
        crate::send_connection_log(
            &log_tx,
            "SOCKS5",
            format!(
                "Closed: [{}] -> {}:{} (sent {:.1} KB, received {:.1} KB)",
                app_display,
                target_host,
                target_port,
                (upload as f64) / 1024.0,
                (download as f64) / 1024.0
            ),
            LogLevel::Info,
            &target_host,
            target_port,
            app_name,
            Some("Tunnel".into()),
        )
        .await;
    }

    Ok(())
}





// ============================================================================
// ENGINE 2: XRAY / V2RAY CORE SUPERVISOR
// ============================================================================

fn is_running_as_root() -> bool {
    unsafe {
        extern "C" {
            fn geteuid() -> u32;
        }
        geteuid() == 0
    }
}

#[allow(dead_code)]
fn binary_has_cap_net_admin(bin: &Path) -> bool {
    if let Ok(output) = std::process::Command::new("getcap").arg(bin).output() {
        let text = String::from_utf8_lossy(&output.stdout);
        return text.contains("cap_net_admin");
    }
    false
}

fn classify_xray_log(line: &str) -> LogLevel {
    if line.contains("[Warning]") {
        LogLevel::Warn
    } else if line.contains("[Error]") {
        LogLevel::Error
    } else if line.contains("failed") || line.contains("Failed") {
        LogLevel::Warn
    } else if line.contains("Configuration OK") || line.contains("started") {
        LogLevel::Success
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
        let route = right.rsplit_once('[').map(|(_, route)| route.trim_end_matches(']'))
            .unwrap_or("proxy").rsplit("->").next().unwrap_or("proxy").trim().to_owned();

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
            if !crate::log_activity::enabled()
                && !line.contains("[Error]")
                && !line.contains("proxy/vless/outbound: tunneling request to")
            { continue; }
            let lvl = classify_xray_log(&line);
            let cleaned = strip_xray_log_prefix(&line);
            if let Some(conn) = parse_xray_connection(cleaned) {
                let app_name = if let Some(port) = conn.client_port.filter(|_| crate::log_activity::detailed()) {
                    tokio::task::spawn_blocking(move || crate::apps::resolve_socket_process_name(port)).await.ok().flatten()
                } else { None };
                let is_direct = conn.route.eq_ignore_ascii_case("direct") || conn.route.eq_ignore_ascii_case("bypass");
                record_connection_start(is_direct);
                crate::send_connection_log(
                    &tx,
                    "Xray",
                    cleaned,
                    lvl,
                    &conn.target_host,
                    conn.target_port,
                    app_name,
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
    // Each run owns its Xray child; avoid killing another Netray session.
    if cfg.enable_tun {
        let _ = Command::new("ip").args(["link", "delete", "netray0"]).output().await;
    }

    // 1. Locate xray / v2ray binary
    let xray_bin = match crate::v2ray::find_xray_binary() {
        Ok(bin) => bin,
        Err(e) => {
            send_log(log_tx, "Xray", &e, LogLevel::Error).await;
            return Err(e.into());
        }
    };
    send_log(
        log_tx,
        "Xray",
        format!("Found core binary at: {}", xray_bin.display()),
        LogLevel::Info,
    )
    .await;

    // 2. Parse URI / JSON and generate valid Xray configuration
    let http_port = cfg.local_socks_port.saturating_add(1);
    let config_json = match crate::v2ray::parse_and_generate_xray_config_with_whitelist(
        &cfg.v2ray_config,
        cfg.local_socks_port,
        http_port,
        cfg.enable_tun,
        &cfg.whitelist_apps,
        &cfg.whitelist_urls,
    ) {
        Ok(json_str) => json_str,
        Err(e) => {
            send_log(log_tx, "Xray", format!("Config parse error: {}", e), LogLevel::Error).await;
            return Err(e.into());
        }
    };

    let mut logging_config: serde_json::Value = serde_json::from_str(&config_json)?;
    crate::v2ray::apply_tls_policy(&mut logging_config, cfg.allow_insecure_tls);
    crate::v2ray::apply_dns_policy(&mut logging_config, &cfg.dns_servers, cfg.ipv6_enabled)?;
    crate::v2ray::apply_lan_policy(&mut logging_config, cfg.bypass_lan);
    crate::v2ray::apply_ipv6_policy(&mut logging_config, cfg.ipv6_enabled);
    logging_config["log"] = serde_json::json!({"loglevel": if crate::log_activity::enabled() { "info" } else { "error" }, "access": "", "error": ""});
    logging_config["stats"] = serde_json::json!({});
    if !logging_config["api"].is_object() {
        logging_config["api"] = serde_json::json!({});
    }
    if logging_config["api"]["tag"].as_str().is_none() {
        logging_config["api"]["tag"] = serde_json::json!("netray-api");
    }
    logging_config["api"]["listen"] = serde_json::json!(format!(
        "127.0.0.1:{}",
        crate::v2ray::xray_stats_port()
    ));
    if !logging_config["api"]["services"].is_array() {
        logging_config["api"]["services"] = serde_json::json!([]);
    }
    if let Some(services) = logging_config["api"]["services"].as_array_mut() {
        if !services.iter().any(|service| service == "StatsService") {
            services.push(serde_json::json!("StatsService"));
        }
    }
    logging_config["policy"]["system"]["statsOutboundUplink"] = serde_json::json!(true);
    logging_config["policy"]["system"]["statsOutboundDownlink"] = serde_json::json!(true);
    let config_json = serde_json::to_string(&logging_config)?;

    let config_json = if cfg.enable_tun {
        let mut config: serde_json::Value = serde_json::from_str(&config_json)?;
        if let Err(error) = crate::v2ray::pin_proxy_dns(&mut config).await {
            send_log(log_tx, "DNS", &error, LogLevel::Error).await;
            return Err(error.into());
        }
        serde_json::to_string(&config)?
    } else { config_json };

    // 3. Test & validate config with the core binary
    send_log(log_tx, "Xray", "Validating configuration with core...", LogLevel::Info).await;
    if let Err(e) = crate::v2ray::test_xray_config(&xray_bin, &config_json).await {
        send_log(log_tx, "Xray", format!("Config validation failed:\n{}", e), LogLevel::Error).await;
        return Err(e.into());
    }
    send_log(
        log_tx,
        "Xray",
        format!(
            "Configuration OK. Local SOCKS5: 127.0.0.1:{}, HTTP: 127.0.0.1:{}",
            cfg.local_socks_port, http_port
        ),
        LogLevel::Success,
    )
    .await;

    // 4. Write temporary configuration
    let config_path = format!("/tmp/netray_xray_{}.json", std::process::id());
    // Never follow a pre-existing /tmp symlink or expose server credentials to other users.
    let mut config_file = tokio::fs::OpenOptions::new()
        .write(true).create_new(true).mode(0o600).open(&config_path).await?;
    if let Err(error) = config_file.write_all(config_json.as_bytes()).await {
        let _ = tokio::fs::remove_file(&config_path).await;
        return Err(error.into());
    }
    drop(config_file);

    // 5. Enable System Proxy if requested
    if cfg.enable_sysproxy {
        send_log(log_tx, "SystemProxy", "Configuring GNOME system-wide proxy...", LogLevel::Info).await;
        if let Err(e) = crate::sysproxy::enable_system_proxy(cfg.local_socks_port, http_port).await {
            send_log(log_tx, "SystemProxy", format!("Warning: failed to set system proxy: {}", e), LogLevel::Warn).await;
        } else {
            send_log(
                log_tx,
                "SystemProxy",
                format!("System-wide proxy active -> SOCKS5 127.0.0.1:{}, HTTP 127.0.0.1:{}", cfg.local_socks_port, http_port),
                LogLevel::Success,
            ).await;
        }
    }

    let is_root = is_running_as_root();
    let use_pkexec = cfg.enable_tun && !is_root && Path::new("/usr/bin/pkexec").exists();

    if cfg.enable_tun {
        if is_root {
            send_log(
                log_tx,
                "TUN",
                "TUN mode active (running as root; creating netray0 virtual interface)...",
                LogLevel::Success,
            )
            .await;
        } else if use_pkexec {
            send_log(
                log_tx,
                "TUN",
                "TUN mode requires root privileges to create virtual interface 'netray0'. Requesting authorization via pkexec...",
                LogLevel::Info,
            )
            .await;
        } else {
            send_log(
                log_tx,
                "TUN",
                "Warning: TUN mode requires root privileges. Please ensure pkexec is available.",
                LogLevel::Warn,
            )
            .await;
        }
    }

    send_log(log_tx, "Xray", "Spawning core process...", LogLevel::Info).await;

    let supervisor_script = r#"
xray_bin="$1"
cfg="$2"

cleanup() {
    trap - TERM INT HUP EXIT
    kill "$WATCHER_PID" 2>/dev/null
    if [ -n "$XRAY_PID" ]; then
        kill -TERM "$XRAY_PID" 2>/dev/null
        sleep 1
        kill -KILL "$XRAY_PID" 2>/dev/null
        wait "$XRAY_PID" 2>/dev/null
    fi
    ip link delete netray0 2>/dev/null
    rm -f "$cfg"
}

trap cleanup TERM INT HUP EXIT

# Remove any standalone capabilities so Xray runs consistently under the elevated supervisor
setcap -r "$xray_bin" 2>/dev/null || true

# Preserve stdin before POSIX shells redirect background jobs to /dev/null.
exec 3<&0
"$xray_bin" run -c "$cfg" 3<&- &
XRAY_PID=$!

# Watch parent EOF without polling, while the supervisor waits for the actual core.
supervisor_pid=$$
(while read -r line; do :; done; kill -TERM "$supervisor_pid") <&3 3<&- &
WATCHER_PID=$!
exec 3<&-
wait "$XRAY_PID"
status=$?
# On a signal, cleanup still owns the live core; after natural exit it is reaped.
if ! kill -0 "$XRAY_PID" 2>/dev/null; then XRAY_PID=; fi
exit "$status"
"#;

    let xray_bin_str = xray_bin.to_string_lossy().to_string();
    let mut cmd = if use_pkexec {
        let mut c = Command::new("pkexec");
        c.args(["sh", "-c", supervisor_script, "_", &xray_bin_str, &config_path]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", supervisor_script, "_", &xray_bin_str, &config_path]);
        c
    };

    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let _ = tokio::fs::remove_file(&config_path).await;
            return Err(e.into());
        }
    };

    let mut stdin = child.stdin.take();

    if let Some(stdout) = child.stdout.take() {
        spawn_xray_log_pipe(stdout, log_tx.clone());
    }

    if let Some(stderr) = child.stderr.take() {
        spawn_xray_log_pipe(stderr, log_tx.clone());
    }

    send_log(
        log_tx,
        "Xray",
        format!("Checking http://www.gstatic.com/generate_204 through HTTP proxy 127.0.0.1:{http_port}; connection requires HTTP 204."),
        LogLevel::Info,
    )
    .await;
    let ping_ms = tokio::select! {
        // Verification may fail during an outage; keep the same core and routes alive.
        ms = async {
            loop {
                match crate::v2ray::wait_for_http_proxy(http_port).await {
                    Ok(ms) => break ms,
                    Err(error) => {
                        send_log(log_tx, "Supervisor", format!("Connection not yet verified: {error}. Keeping Xray running; retrying in 30 seconds."), LogLevel::Warn).await;
                        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                    }
                }
            }
        } => Some(ms),
        status = child.wait() => {
            match status {
                Ok(status) => send_log(log_tx, "Error", format!("Xray exited before connection verification: {status}"), LogLevel::Error).await,
                Err(error) => send_log(log_tx, "Error", format!("Could not wait for Xray during connection verification: {error}"), LogLevel::Error).await,
            }
            None
        },
        _ = &mut *stop_rx => {
            send_log(log_tx, "Supervisor", "Connection check cancelled; stopping Xray.", LogLevel::Warn).await;
            drop(stdin.take()); let _ = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await;
            None
        }
    };
    if let Some(ms) = ping_ms {
        send_log(log_tx, "Supervisor", format!("Tunnel ready · HTTP 204 through proxy in {ms} ms."), LogLevel::Success).await;
    }

    let status_opt = if ping_ms.is_some() { tokio::select! {
        res = child.wait() => {
            match res {
                Ok(status) => Some(status),
                Err(error) => {
                    send_log(log_tx, "Error", format!("Could not wait for Xray: {error}"), LogLevel::Error).await;
                    None
                }
            }
        }
        _ = &mut *stop_rx => {
            send_log(log_tx, "Supervisor", "Stopping Xray core and tearing down TUN interface...", LogLevel::Info).await;
            // Dropping stdin triggers the supervisor's cleanup trap immediately
            drop(stdin.take());
            // Give up to 1 second for graceful exit, then ensure process is dead
            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await;
            let _ = child.kill().await;
            None
        }
    }} else {
        if stdin.is_some() { drop(stdin.take()); let _ = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await; }
        None
    };

    if let Some(status) = status_opt {
        if !status.success() {
            if use_pkexec && status.code() == Some(126) {
                send_log(
                    log_tx,
                    "TUN",
                    "TUN authorization was dismissed. Tip: Enable GNOME System Proxy for rootless routing, or grant capability: sudo setcap cap_net_admin,cap_net_bind_service=+ep /usr/local/bin/xray",
                    LogLevel::Warn,
                )
                .await;
            } else {
                send_log(log_tx, "Xray", format!("Core process exited with: {}", status), LogLevel::Warn).await;
            }
        } else {
            send_log(log_tx, "Xray", format!("Core process exited cleanly: {}", status), LogLevel::Info).await;
        }
    }

    let _ = tokio::fs::remove_file(&config_path).await;
    let _ = Command::new("pkill").args(["-f", &config_path]).output().await;
    if cfg.enable_tun {
        let _ = Command::new("ip").args(["link", "delete", "netray0"]).output().await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tunnel_metrics_lifecycle() {
        reset_tunnel_metrics();
        assert_eq!(get_tunnel_metrics(), (0, 0, 0));

        record_connection_start(false);
        assert_eq!(get_tunnel_metrics(), (1, 1, 0));

        record_connection_start(true);
        assert_eq!(get_tunnel_metrics(), (2, 1, 1));

        drop(ActiveConnection);
        assert_eq!(get_tunnel_metrics(), (1, 1, 1));

        reset_tunnel_metrics();
        assert_eq!(get_tunnel_metrics(), (0, 0, 0));
    }
}
