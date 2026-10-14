use crate::{send_log, LogEntry, LogLevel, TunnelMode};
use async_trait::async_trait;
use russh::client;
use russh_keys::key;
use rustls_pki_types::ServerName;
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
}

// Minimal russh client handler (accepts server host keys like NetMod)
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
            send_log(&log_tx, "Supervisor", "Tunnel stopped. All sockets and child processes closed.", LogLevel::Warn).await;
        }
    }
}

