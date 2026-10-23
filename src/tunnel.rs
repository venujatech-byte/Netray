use crate::{send_log, LogEntry, LogLevel, TunnelMode};
use async_trait::async_trait;
use russh::client;
use russh_keys::key;
use rustls_pki_types::ServerName;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
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
}

// Minimal russh client handler (accepts server host keys like Netray)
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
    log_tx: mpsc::UnboundedSender<LogEntry>,
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

    let engine_task = async {
        let result = match cfg.mode {
            TunnelMode::SshPayload => run_ssh_payload_engine(&log_tx, &cfg, false).await,
            TunnelMode::SshSslSni => run_ssh_payload_engine(&log_tx, &cfg, true).await,
            TunnelMode::V2RayXray => run_xray_engine(&log_tx, &cfg).await,
            TunnelMode::SlowDns => {
                Err("SlowDNS binary wrapper not yet configured.".into())
            }
        };

        if let Err(e) = result {
            send_log(&log_tx, "Error", format!("Engine failed: {}", e), LogLevel::Error).await;
        }
    };

    // Wait for either the engine to exit OR the user to click STOP TUNNEL
    tokio::select! {
        _ = engine_task => {}
        _ = &mut stop_rx => {
            send_log(&log_tx, "Supervisor", "Tunnel stopped by user. All sockets and child processes closed.", LogLevel::Warn).await;
        }
    }

    if cfg.enable_sysproxy || cfg.enable_tun {
        crate::sysproxy::disable_system_proxy().await;
        send_log(&log_tx, "SystemProxy", "System-wide proxy & TUN routing disabled. Network restored.", LogLevel::Info).await;
    }

    send_log(&log_tx, "Supervisor", "Tunnel is offline.", LogLevel::Info).await;
}






// ============================================================================
// ENGINE 1: PAYLOAD INJECTOR + TLS SNI + RUSSH + LOCAL SOCKS5 SERVER
// ============================================================================

async fn run_ssh_payload_engine(
    log_tx: &mpsc::UnboundedSender<LogEntry>,
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

    loop {
        let (client_socket, _) = listener.accept().await?;
        let ssh_handle = Arc::clone(&shared_ssh);
        let log_tx_clone = log_tx.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_socks5_client(client_socket, ssh_handle).await {
                let _ = send_log(&log_tx_clone, "SOCKS5", format!("Stream closed: {}", e), LogLevel::Warn).await;
            }
        });
    }
}

/// Minimal RFC 1928 SOCKS5 server that pipes each request into a russh `direct-tcpip` channel
async fn handle_socks5_client(
    mut sock: TcpStream,
    ssh: Arc<client::Handle<SshClientHandler>>,
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
    let target_port = u16::from_be_bytes([buf[0], buf[1]]) as u32;

    // 3. Open SSH `direct-tcpip` channel to destination
    let channel = ssh
        .channel_open_direct_tcpip(&target_host, target_port, "127.0.0.1", 10808)
        .await?;

    // 4. Reply SOCKS5 Success
    sock.write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0]).await?;

    // 5. Bi-directional zero-copy relay between local app and SSH channel
    let mut ssh_stream = channel.into_stream();
    tokio::io::copy_bidirectional(&mut sock, &mut ssh_stream).await?;

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
    } else if line.contains("[Error]") || line.contains("failed") || line.contains("Failed") {
        LogLevel::Error
    } else if line.contains("Configuration OK") || line.contains("started") {
        LogLevel::Success
    } else {
        LogLevel::Info
    }
}

fn spawn_xray_log_pipe<R: tokio::io::AsyncRead + Unpin + Send + 'static>(
    stream: R,
    tx: mpsc::UnboundedSender<LogEntry>,
) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stream).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let lvl = classify_xray_log(&line);
            send_log(&tx, "Xray", line, lvl).await;
        }
    });
}

async fn run_xray_engine(
    log_tx: &mpsc::UnboundedSender<LogEntry>,
    cfg: &TunnelConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
    let config_json = match crate::v2ray::parse_and_generate_xray_config(
        &cfg.v2ray_config,
        cfg.local_socks_port,
        http_port,
        cfg.enable_tun,
    ) {
        Ok(json_str) => json_str,
        Err(e) => {
            send_log(log_tx, "Xray", format!("Config parse error: {}", e), LogLevel::Error).await;
            return Err(e.into());
        }
    };

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
    tokio::fs::write(&config_path, &config_json).await?;

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
    let has_caps = binary_has_cap_net_admin(&xray_bin);
    let use_pkexec = cfg.enable_tun && !is_root && !has_caps && Path::new("/usr/bin/pkexec").exists();

    if cfg.enable_tun {
        if is_root || has_caps {
            send_log(
                log_tx,
                "TUN",
                "TUN mode active (root/CAP_NET_ADMIN verified; creating tun0 virtual interface)...",
                LogLevel::Success,
            )
            .await;
        } else if use_pkexec {
            send_log(
                log_tx,
                "TUN",
                "TUN mode requires root privileges to create virtual interface 'tun0'. Requesting authorization via pkexec...",
                LogLevel::Info,
            )
            .await;
        } else {
            send_log(
                log_tx,
                "TUN",
                "Warning: TUN mode requires root or CAP_NET_ADMIN privileges to create interface. Run: sudo setcap cap_net_admin,cap_net_bind_service=+ep /usr/local/bin/xray",
                LogLevel::Warn,
            )
            .await;
        }
    }

    send_log(log_tx, "Xray", "Spawning core process...", LogLevel::Info).await;

    let mut cmd = if use_pkexec {
        let mut c = Command::new("pkexec");
        c.arg(&xray_bin)
            .arg("run")
            .arg("-c")
            .arg(&config_path);
        c
    } else {
        let mut c = Command::new(&xray_bin);
        c.arg("run")
            .arg("-c")
            .arg(&config_path);
        c
    };

    let mut child = match cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = tokio::fs::remove_file(&config_path).await;
            if use_pkexec {
                send_log(
                    log_tx,
                    "TUN",
                    format!("Failed to spawn pkexec: {}. Falling back to unprivileged core.", e),
                    LogLevel::Warn,
                )
                .await;
                let mut fallback = Command::new(&xray_bin);
                fallback
                    .arg("run")
                    .arg("-c")
                    .arg(&config_path)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .kill_on_drop(true)
                    .spawn()?
            } else {
                return Err(e.into());
            }
        }
    };

    if let Some(stdout) = child.stdout.take() {
        spawn_xray_log_pipe(stdout, log_tx.clone());
    }

    if let Some(stderr) = child.stderr.take() {
        spawn_xray_log_pipe(stderr, log_tx.clone());
    }

    let status = child.wait().await?;
    let _ = tokio::fs::remove_file(&config_path).await;
    let _ = tokio::process::Command::new("pkill")
        .args(["-f", &config_path])
        .output()
        .await;

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
    Ok(())
}