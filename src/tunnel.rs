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