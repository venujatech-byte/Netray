use base64::prelude::*;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;
use tokio::net::TcpStream;
use tokio::time::{timeout, Duration};
use url::Url;

/// Locate xray.exe or v2ray.exe on Windows
pub fn find_xray_binary() -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if let Ok(exe) = std::env::current_exe() {
        if let Some(path) = exe.parent().map(|dir| dir.join("xray.exe")) {
            if path.is_file() {
                return Ok(path);
            }
        }
    }

    let candidates = [
        cwd.join("xray.exe"),
        cwd.join("v2ray.exe"),
        cwd.join("bin").join("xray.exe"),
        cwd.join("bin").join("v2ray.exe"),
        PathBuf::from(r#"C:\Program Files\xray\xray.exe"#),
        PathBuf::from(r#"C:\Program Files\v2ray\v2ray.exe"#),
        PathBuf::from(r#"C:\xray\xray.exe"#),
        PathBuf::from(r#"C:\v2ray\v2ray.exe"#),
    ];

    for path in &candidates {
        if path.is_file() {
            return Ok(path.clone());
        }
    }

    // Check system PATH
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let exe = dir.join("xray.exe");
            if exe.is_file() {
                return Ok(exe);
            }
        }
    }

    Err("Xray core was not found. Install xray.exe or place it beside Netray, in its bin folder, or on PATH.".to_string())
}

/// Extract human-readable node name or remark from link
pub fn extract_node_name(raw_input: &str) -> String {
    let trimmed = raw_input.trim();
    if let Some((_, frag)) = trimmed.split_once('#') {
        if let Ok(decoded) = percent_encoding::percent_decode_str(frag).decode_utf8() {
            let res = decoded.trim();
            if !res.is_empty() {
                return res.to_string();
            }
        }
    }

    if trimmed.starts_with("vmess://") {
        let b64 = trimmed.trim_start_matches("vmess://").trim();
        if let Ok(bytes) = BASE64_STANDARD
            .decode(b64)
            .or_else(|_| BASE64_URL_SAFE.decode(b64))
        {
            if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
                if let Some(ps) = v.get("ps").and_then(|p| p.as_str()) {
                    if !ps.trim().is_empty() {
                        return ps.trim().to_string();
                    }
                }
            }
        }
    }

    "Node Profile".to_string()
}

/// Helper to extract protocol tag string like "(VLESS + TCP + TLS)"
pub fn extract_protocol_tag(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with("vless://") {
        if let Ok(url) = Url::parse(trimmed) {
            let mut net = "TCP".to_string();
            let mut sec = "NONE".to_string();
            for (k, v) in url.query_pairs() {
                if k == "type" || k == "net" {
                    net = v.to_uppercase();
                } else if k == "security" {
                    sec = v.to_uppercase();
                }
            }
            if sec != "NONE" {
                return format!("(VLESS + {} + {})", net, sec);
            }
            return format!("(VLESS + {})", net);
        }
        "(VLESS)".to_string()
    } else if trimmed.starts_with("vmess://") {
        "(VMESS + WS + TLS)".to_string()
    } else if trimmed.starts_with("trojan://") {
        if let Ok(url) = Url::parse(trimmed) {
            let mut net = "TCP".to_string();
            for (k, v) in url.query_pairs() {
                if k == "type" || k == "net" {
                    net = v.to_uppercase();
                }
            }
            return format!("(TROJAN + {} + TLS)", net);
        }
        "(TROJAN)".to_string()
    } else if trimmed.starts_with("ss://") {
        "(SHADOWSOCKS)".to_string()
    } else {
        "(CUSTOM JSON)".to_string()
    }
}

/// Helper to parse node address and port for display (e.g. "example.com:443")
pub fn extract_node_address(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Ok(url) = Url::parse(trimmed) {
        if let Some(host) = url.host_str() {
            return format!("{}:{}", host, url.port().unwrap_or(443));
        }
    }
    if trimmed.starts_with("vmess://") {
        let b64 = trimmed.trim_start_matches("vmess://").trim();
        if let Ok(bytes) = BASE64_STANDARD
            .decode(b64)
            .or_else(|_| BASE64_URL_SAFE.decode(b64))
        {
            if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
                if let (Some(add), Some(port)) = (
                    v.get("add").and_then(|p| p.as_str()),
                    v.get("port").and_then(|p| p.as_u64()),
                ) {
                    return format!("{}:{}", add, port);
                }
            }
        }
    }
    "127.0.0.1:443".to_string()
}

pub async fn ping_profile(raw: &str) -> Result<u128, String> {
    let normalized = parse_and_generate_xray_config(raw, 10808, 10809, false)?;
    let config: Value =
        serde_json::from_str(&normalized).map_err(|e| format!("Invalid Xray config: {e}"))?;
    let outbounds = config
        .get("outbounds")
        .and_then(Value::as_array)
        .ok_or_else(|| "No outbound server found in config.".to_string())?;
    let server = outbounds
        .iter()
        .find_map(|outbound| {
            let settings = outbound.get("settings")?;
            settings
                .get("vnext")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .or_else(|| {
                    settings
                        .get("servers")
                        .and_then(Value::as_array)
                        .and_then(|items| items.first())
                })
        })
        .ok_or_else(|| "No remote server endpoint found in config.".to_string())?;
    let host = server
        .get("address")
        .and_then(Value::as_str)
        .filter(|host| !host.is_empty())
        .ok_or_else(|| "Remote server address is missing.".to_string())?;
    let port = server
        .get("port")
        .and_then(Value::as_u64)
        .or_else(|| {
            server
                .get("port")
                .and_then(Value::as_str)
                .and_then(|p| p.parse().ok())
        })
        .filter(|p| (1..=65535).contains(p))
        .ok_or_else(|| "Remote server port is invalid.".to_string())? as u16;

    let started = Instant::now();
    match timeout(Duration::from_secs(5), TcpStream::connect((host, port))).await {
        Ok(Ok(_)) => Ok(started.elapsed().as_millis()),
        Ok(Err(error)) => Err(format!("TCP connection failed: {error}")),
        Err(_) => Err("TCP connection timed out after 5 seconds.".to_string()),
    }
}

#[derive(Debug, Clone, Default)]
pub struct NodeEditState {
    pub id: String,
    pub remarks: String,
    pub address: String,
    pub port: String,
    pub encrypt_method: String,
    pub flow: String,
    pub user_id: String,
    pub transport_protocol: String,
    pub header_type: String,
    pub tls_type: String,
    pub sni: String,
    pub fingerprint: String,
    pub alpn: String,
}

impl NodeEditState {
    pub fn from_profile(id: &str, name: &str, raw_uri: &str) -> Self {
        let trimmed = raw_uri.trim();
        if let Ok(url) = Url::parse(trimmed) {
            let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
            let remarks = if !name.is_empty() {
                name.to_string()
            } else {
                extract_node_name(trimmed)
            };
            return Self {
                id: id.to_string(),
                remarks,
                address: url.host_str().unwrap_or("").to_string(),
                port: url.port().unwrap_or(443).to_string(),
                encrypt_method: query
                    .get("encryption")
                    .cloned()
                    .unwrap_or_else(|| "none".into()),
                flow: query.get("flow").cloned().unwrap_or_else(|| "none".into()),
                user_id: url.username().to_string(),
                transport_protocol: query
                    .get("type")
                    .or_else(|| query.get("net"))
                    .cloned()
                    .unwrap_or_else(|| "tcp".into()),
                header_type: query
                    .get("headerType")
                    .cloned()
                    .unwrap_or_else(|| "none".into()),
                tls_type: query
                    .get("security")
                    .cloned()
                    .unwrap_or_else(|| "tls".into()),
                sni: query
                    .get("sni")
                    .or_else(|| query.get("peer"))
                    .cloned()
                    .unwrap_or_default(),
                fingerprint: query.get("fp").cloned().unwrap_or_else(|| "chrome".into()),
                alpn: query.get("alpn").cloned().unwrap_or_else(|| "none".into()),
            };
        }

        Self {
            id: id.to_string(),
            remarks: if name.is_empty() {
                "New VLess Node".into()
            } else {
                name.to_string()
            },
            address: "example.com".into(),
            port: "443".into(),
            encrypt_method: "none".into(),
            flow: "none".into(),
            user_id: "".into(),
            transport_protocol: "tcp".into(),
            header_type: "none".into(),
            tls_type: "tls".into(),
            sni: "".into(),
            fingerprint: "chrome".into(),
            alpn: "none".into(),
        }
    }

    pub fn to_vless_uri(&self) -> String {
        let mut u = format!(
            "vless://{}@{}:{}?encryption={}&security={}&type={}",
            self.user_id.trim(),
            self.address.trim(),
            self.port.trim(),
            if self.encrypt_method.is_empty() {
                "none"
            } else {
                self.encrypt_method.trim()
            },
            if self.tls_type.is_empty() {
                "none"
            } else {
                self.tls_type.trim()
            },
            if self.transport_protocol.is_empty() {
                "tcp"
            } else {
                self.transport_protocol.trim()
            }
        );
        if !self.flow.is_empty() && self.flow != "none" {
            u.push_str(&format!("&flow={}", self.flow.trim()));
        }
        if !self.header_type.is_empty() && self.header_type != "none" {
            u.push_str(&format!("&headerType={}", self.header_type.trim()));
        }
        if !self.sni.is_empty() {
            u.push_str(&format!("&sni={}", self.sni.trim()));
        }
        if !self.fingerprint.is_empty() && self.fingerprint != "none" {
            u.push_str(&format!("&fp={}", self.fingerprint.trim()));
        }
        if !self.alpn.is_empty() && self.alpn != "none" {
            u.push_str(&format!("&alpn={}", self.alpn.trim()));
        }
        if !self.remarks.is_empty() {
            let encoded = percent_encoding::percent_encode(
                self.remarks.as_bytes(),
                percent_encoding::NON_ALPHANUMERIC,
            );
            u.push_str(&format!("#{}", encoded));
        }
        u
    }
}

/// Convert input link or JSON into complete Xray config JSON with Windows WinTun support
pub fn parse_and_generate_xray_config(
    input: &str,
    socks_port: u16,
    http_port: u16,
    enable_tun: bool,
) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Configuration or URI is empty.".into());
    }

    if trimmed.starts_with('{') {
        let mut val: Value =
            serde_json::from_str(trimmed).map_err(|e| format!("Invalid JSON config: {}", e))?;
        inject_inbounds(&mut val, socks_port, http_port, enable_tun)?;
        if enable_tun {
            configure_tun(&mut val)?;
        }
        return serde_json::to_string_pretty(&val)
            .map_err(|e| format!("Serialization error: {}", e));
    }

    let outbound = if trimmed.starts_with("vless://") {
        parse_vless_uri(trimmed)?
    } else if trimmed.starts_with("vmess://") {
        parse_vmess_uri(trimmed)?
    } else if trimmed.starts_with("trojan://") {
        parse_trojan_uri(trimmed)?
    } else if trimmed.starts_with("ss://") {
        parse_shadowsocks_uri(trimmed)?
    } else {
        return Err(
            "Unsupported format. Please provide a VLESS, VMess, Trojan, Shadowsocks link, or JSON."
                .into(),
        );
    };

    let inbounds = vec![
        json!({
            "tag": "socks-in",
            "port": socks_port,
            "listen": "127.0.0.1",
            "protocol": "socks",
            "sniffing": {
                "enabled": true,
                "destOverride": ["http", "tls"]
            },
            "settings": {
                "auth": "noauth",
                "udp": true
            }
        }),
        json!({
            "tag": "http-in",
            "port": http_port,
            "listen": "127.0.0.1",
            "protocol": "http",
            "sniffing": {
                "enabled": true,
                "destOverride": ["http", "tls"]
            },
            "settings": {
                "allowTransparent": false
            }
        }),
    ];

    let mut root_cfg = json!({
        "log": {
            "loglevel": "warning"
        },
        "inbounds": inbounds,
        "outbounds": [
            outbound,
            {
                "tag": "direct",
                "protocol": "freedom"
            },
            {
                "tag": "block",
                "protocol": "blackhole"
            }
        ],
        "routing": {
            "domainStrategy": "AsIs",
            "rules": [
                {
                    "type": "field",
                    "inboundTag": ["socks-in", "http-in", "tun-in"],
                    "outboundTag": "proxy"
                }
            ]
        }
    });
    if enable_tun {
        configure_tun(&mut root_cfg)?;
    }

    serde_json::to_string_pretty(&root_cfg)
        .map_err(|e| format!("Failed to format configuration: {}", e))
}

fn inject_inbounds(
    val: &mut Value,
    socks_port: u16,
    http_port: u16,
    enable_tun: bool,
) -> Result<(), String> {
    let obj = val
        .as_object_mut()
        .ok_or("Xray JSON config must be an object.")?;
    let inbounds = obj.entry("inbounds").or_insert_with(|| json!([]));
    let arr = inbounds
        .as_array_mut()
        .ok_or("Xray JSON inbounds must be an array.")?;
    arr.retain(|i| {
        if let Some(p) = i.get("port").and_then(|p| p.as_u64()) {
            p != socks_port as u64 && p != http_port as u64
        } else if let Some(proto) = i.get("protocol").and_then(|p| p.as_str()) {
            proto != "tun"
        } else {
            true
        }
    });
    arr.push(json!({
            "tag": "socks-in",
            "port": socks_port,
            "listen": "127.0.0.1",
            "protocol": "socks",
            "sniffing": {
                "enabled": true,
                "destOverride": ["http", "tls"]
            },
            "settings": { "auth": "noauth", "udp": true }
        }));
    arr.push(json!({
            "tag": "http-in",
            "port": http_port,
            "listen": "127.0.0.1",
            "protocol": "http",
            "sniffing": {
                "enabled": true,
                "destOverride": ["http", "tls"]
            },
            "settings": { "allowTransparent": false }
        }));
    if enable_tun {
        arr.push(json!({
            "tag": "tun-in",
            "protocol": "tun",
            "settings": { "name": "wintun", "mtu": 1500 }
        }));
    }
    Ok(())
}

fn configure_tun(config: &mut Value) -> Result<(), String> {
    let inbounds = config
        .get_mut("inbounds")
        .and_then(Value::as_array_mut)
        .ok_or("Xray config inbounds must be an array for TUN mode.")?;
    let index = inbounds
        .iter()
        .position(|inbound| inbound["protocol"] == "tun")
        .unwrap_or_else(|| {
            inbounds.push(json!({ "tag": "tun-in", "protocol": "tun" }));
            inbounds.len() - 1
        });
    let inbound = &mut inbounds[index];
    let tag = inbound["tag"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            inbound["tag"] = json!("tun-in");
            "tun-in".to_owned()
        });
    inbound["settings"] = json!({
        "name": "wintun",
        "desc": "Netray",
        "mtu": 1500,
        "gateway": ["198.18.0.1/30", "fc00::1/64"],
        "dns": ["1.1.1.1", "8.8.8.8"],
        "autoSystemRoutingTable": ["0.0.0.0/0", "::/0"],
        "autoOutboundsInterface": "auto",
        "autoSystemWfpBlockLeak": ["dns", "misconfigtun"]
    });

    let dns = config
        .as_object_mut()
        .ok_or("Xray config must be a JSON object.")?
        .entry("dns")
        .or_insert_with(|| json!({ "servers": ["https://1.1.1.1/dns-query", "https://8.8.8.8/dns-query"] }));
    if !dns.is_object() {
        return Err("Xray DNS config must be an object for TUN mode.".to_owned());
    }
    if dns.get("servers").is_some_and(|servers| !servers.is_array()) {
        return Err("Xray DNS servers must be an array for TUN mode.".to_owned());
    }
    if dns["servers"].as_array().map_or(true, Vec::is_empty) {
        dns["servers"] = json!(["https://1.1.1.1/dns-query", "https://8.8.8.8/dns-query"]);
    }

    let outbounds = config
        .get_mut("outbounds")
        .and_then(Value::as_array_mut)
        .ok_or("Xray config outbounds must be an array for TUN mode.")?;
    if outbounds.is_empty() {
        return Err("Xray config needs at least one outbound for TUN mode.".to_owned());
    }
    let primary = outbounds
        .iter()
        .position(|outbound| outbound["tag"] == "proxy")
        .or_else(|| {
            outbounds.iter().position(|outbound| {
                !matches!(outbound["protocol"].as_str(), Some("dns" | "freedom" | "blackhole"))
            })
        })
        .unwrap_or(0);
    if outbounds[primary]["tag"].as_str().is_none() {
        outbounds[primary]["tag"] = json!("netray-primary");
    }
    let primary_tag = outbounds[primary]["tag"].as_str().unwrap().to_owned();
    let dns_tag = if let Some(outbound) = outbounds
        .iter_mut()
        .find(|outbound| outbound["protocol"] == "dns")
    {
        if outbound["tag"].as_str().is_none() {
            outbound["tag"] = json!("netray-dns");
        }
        outbound["tag"].as_str().unwrap().to_owned()
    } else {
        outbounds.push(json!({ "tag": "netray-dns", "protocol": "dns" }));
        "netray-dns".to_owned()
    };

    let routing = config
        .as_object_mut()
        .ok_or("Xray config must be a JSON object.")?
        .entry("routing")
        .or_insert_with(|| json!({ "rules": [] }));
    let routing = routing
        .as_object_mut()
        .ok_or("Xray routing config must be an object for TUN mode.")?;
    let rules = routing
        .entry("rules")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or("Xray routing rules must be an array for TUN mode.")?;
    rules.insert(0, json!({
        "type": "field", "inboundTag": [tag], "port": 53, "outboundTag": dns_tag
    }));
    rules.insert(1, json!({
        "type": "field", "inboundTag": [tag], "outboundTag": primary_tag
    }));
    Ok(())
}

fn parse_vless_uri(uri_str: &str) -> Result<Value, String> {
    let url = Url::parse(uri_str).map_err(|e| format!("Invalid VLESS URL: {}", e))?;
    let host = url.host_str().ok_or("VLESS URL missing host")?;
    let port = url.port().unwrap_or(443);
    let uuid = url.username();
    if uuid.is_empty() {
        return Err("VLESS URL missing UUID".into());
    }

    let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
    let encryption = query
        .get("encryption")
        .cloned()
        .unwrap_or_else(|| "none".into());
    let flow = query.get("flow").cloned().unwrap_or_default();
    let stream_settings = extract_stream_settings(&query, host)?;

    let mut user_obj = json!({
        "id": uuid,
        "encryption": encryption,
        "level": 0
    });
    if !flow.is_empty() && flow != "none" {
        user_obj["flow"] = json!(flow);
    }

    Ok(json!({
        "tag": "proxy",
        "protocol": "vless",
        "settings": {
            "vnext": [
                {
                    "address": host,
                    "port": port,
                    "users": [user_obj]
                }
            ]
        },
        "streamSettings": stream_settings
    }))
}

fn parse_vmess_uri(uri_str: &str) -> Result<Value, String> {
    let raw = uri_str.trim_start_matches("vmess://").trim();
    let bytes = BASE64_STANDARD
        .decode(raw)
        .or_else(|_| BASE64_URL_SAFE.decode(raw))
        .map_err(|e| format!("Invalid base64 in VMess URI: {}", e))?;
    let json_val: Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("Invalid JSON in VMess URI: {}", e))?;

    let host = json_val
        .get("add")
        .and_then(|v| v.as_str())
        .ok_or("VMess missing add")?;
    let port = json_val
        .get("port")
        .and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        })
        .ok_or("VMess missing port")? as u16;
    let uuid = json_val
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or("VMess missing id")?;
    let net = json_val
        .get("net")
        .and_then(|v| v.as_str())
        .unwrap_or("tcp");
    let tls = json_val
        .get("tls")
        .and_then(|v| v.as_str())
        .unwrap_or("none");
    let sni = json_val.get("sni").and_then(|v| v.as_str()).unwrap_or("");
    let host_header = json_val.get("host").and_then(|v| v.as_str()).unwrap_or("");
    let path = json_val.get("path").and_then(|v| v.as_str()).unwrap_or("/");

    let stream_settings = build_stream_settings(
        net,
        tls,
        sni,
        host_header,
        path,
        "",
        "",
        "chrome",
        "",
        "",
        "",
        "",
    )?;

    Ok(json!({
        "tag": "proxy",
        "protocol": "vmess",
        "settings": {
            "vnext": [
                {
                    "address": host,
                    "port": port,
                    "users": [
                        {
                            "id": uuid,
                            "alterId": 0,
                            "security": "auto",
                            "level": 0
                        }
                    ]
                }
            ]
        },
        "streamSettings": stream_settings
    }))
}

fn parse_trojan_uri(uri_str: &str) -> Result<Value, String> {
    let url = Url::parse(uri_str).map_err(|e| format!("Invalid Trojan URL: {}", e))?;
    let host = url.host_str().ok_or("Trojan URL missing host")?;
    let port = url.port().unwrap_or(443);
    let password = url.username();
    if password.is_empty() {
        return Err("Trojan URL missing password".into());
    }

    let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
    let stream_settings = extract_stream_settings(&query, host)?;

    Ok(json!({
        "tag": "proxy",
        "protocol": "trojan",
        "settings": {
            "servers": [
                {
                    "address": host,
                    "port": port,
                    "password": password,
                    "level": 0
                }
            ]
        },
        "streamSettings": stream_settings
    }))
}

fn parse_shadowsocks_uri(uri_str: &str) -> Result<Value, String> {
    let stripped = uri_str.trim_start_matches("ss://");
    let (body, _) = stripped.split_once('#').unwrap_or((stripped, ""));

    let (method, password, host, port) = if let Some((user_info, host_port)) = body.split_once('@')
    {
        let (h, p_str) = host_port.split_once(':').ok_or("SS URI missing port")?;
        let p = p_str.parse::<u16>().map_err(|_| "Invalid SS port")?;

        let decoded_user = if let Ok(bytes) = BASE64_STANDARD
            .decode(user_info)
            .or_else(|_| BASE64_URL_SAFE.decode(user_info))
        {
            String::from_utf8_lossy(&bytes).to_string()
        } else {
            user_info.to_string()
        };

        let (m, pass) = decoded_user
            .split_once(':')
            .ok_or("SS URI missing method:password")?;
        (m.to_string(), pass.to_string(), h.to_string(), p)
    } else {
        let decoded = BASE64_STANDARD
            .decode(body)
            .or_else(|_| BASE64_URL_SAFE.decode(body))
            .map_err(|e| format!("Invalid base64 in SS URI: {}", e))?;
        let decoded_str = String::from_utf8_lossy(&decoded).to_string();
        let (method_pass, host_port) = decoded_str
            .split_once('@')
            .ok_or("Invalid decoded SS format")?;
        let (m, pass) = method_pass
            .split_once(':')
            .ok_or("Missing method:password")?;
        let (h, p_str) = host_port.split_once(':').ok_or("Missing host:port")?;
        let p = p_str.parse::<u16>().map_err(|_| "Invalid port")?;
        (m.to_string(), pass.to_string(), h.to_string(), p)
    };

    Ok(json!({
        "tag": "proxy",
        "protocol": "shadowsocks",
        "settings": {
            "servers": [
                {
                    "address": host,
                    "port": port,
                    "method": method,
                    "password": password
                }
            ]
        }
    }))
}

fn extract_stream_settings(query: &HashMap<String, String>, host: &str) -> Result<Value, String> {
    let network = query
        .get("type")
        .or_else(|| query.get("net"))
        .map(|s| s.as_str())
        .unwrap_or("tcp");
    let security = query.get("security").map(|s| s.as_str()).unwrap_or("none");
    let sni = query
        .get("sni")
        .or_else(|| query.get("peer"))
        .cloned()
        .unwrap_or_default();
    let header_host = query.get("host").cloned().unwrap_or_default();
    let path = query.get("path").cloned().unwrap_or_else(|| "/".into());
    let service_name = query.get("serviceName").cloned().unwrap_or_default();
    let header_type = query.get("headerType").cloned().unwrap_or_default();
    let fp = query.get("fp").cloned().unwrap_or_else(|| "chrome".into());
    let pbk = query.get("pbk").map(|s| s.as_str()).unwrap_or("");
    let sid = query.get("sid").map(|s| s.as_str()).unwrap_or("");

    let vcn = query
        .get("vcn")
        .or_else(|| query.get("verifyPeerCertByName"))
        .cloned()
        .or_else(|| {
            if !sni.is_empty() && sni != host && host.parse::<std::net::IpAddr>().is_err() {
                Some(host.to_string())
            } else {
                None
            }
        })
        .unwrap_or_default();

    let pinned_cert = query
        .get("pcs")
        .or_else(|| query.get("pinnedPeerCertSha256"))
        .cloned()
        .unwrap_or_default();

    build_stream_settings(
        network,
        security,
        &sni,
        &header_host,
        &path,
        &service_name,
        &header_type,
        &fp,
        pbk,
        sid,
        &vcn,
        &pinned_cert,
    )
}

fn build_stream_settings(
    network: &str,
    security: &str,
    sni: &str,
    host_header: &str,
    path: &str,
    service_name: &str,
    header_type: &str,
    fp: &str,
    pbk: &str,
    sid: &str,
    vcn: &str,
    pinned_cert: &str,
) -> Result<Value, String> {
    let mut settings = json!({
        "network": network,
    });

    match network {
        "ws" => {
            let mut ws_obj = json!({ "path": path });
            if !host_header.is_empty() {
                ws_obj["headers"] = json!({ "Host": host_header });
            }
            settings["wsSettings"] = ws_obj;
        }
        "grpc" => {
            let mut grpc_obj = json!({ "multiMode": false });
            if !service_name.is_empty() {
                grpc_obj["serviceName"] = json!(service_name);
            }
            settings["grpcSettings"] = grpc_obj;
        }
        "h2" | "http" => {
            let mut h2_obj = json!({ "path": path });
            if !host_header.is_empty() {
                h2_obj["host"] = json!([host_header]);
            }
            settings["httpSettings"] = h2_obj;
        }
        "tcp" => {
            if header_type.eq_ignore_ascii_case("http") {
                let mut http_obj = json!({ "type": "http" });
                if !host_header.is_empty() {
                    http_obj["request"] = json!({ "headers": { "Host": [host_header] } });
                }
                settings["tcpSettings"] = json!({ "header": http_obj });
            }
        }
        _ => {}
    }

    if security == "tls" {
        settings["security"] = json!("tls");
        let mut tls_settings = json!({ "fingerprint": fp });
        if !sni.is_empty() {
            tls_settings["serverName"] = json!(sni);
        }
        if !vcn.is_empty() {
            tls_settings["verifyPeerCertByName"] = json!(vcn);
        }
        if !pinned_cert.is_empty() {
            tls_settings["pinnedPeerCertSha256"] = json!(pinned_cert);
        }
        settings["tlsSettings"] = tls_settings;
    } else if security == "reality" {
        settings["security"] = json!("reality");
        let mut reality_settings = json!({
            "fingerprint": fp,
            "publicKey": pbk,
            "shortId": sid,
        });
        if !sni.is_empty() {
            reality_settings["serverName"] = json!(sni);
        }
        settings["realitySettings"] = reality_settings;
    } else {
        settings["security"] = json!("none");
    }

    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vless_generation_wintun() {
        let uri = "vless://b831381d-6324-4d53-ad4f-8cda48b30811@example.com:443?encryption=none&security=tls&sni=example.com&type=ws&path=%2F#Sample-Node";
        let config =
            parse_and_generate_xray_config(uri, 10808, 10809, true).expect("Should parse with TUN");
        let json_val: Value = serde_json::from_str(&config).expect("Must be valid JSON");
        let inbounds = json_val["inbounds"]
            .as_array()
            .expect("inbounds must be array");
        assert_eq!(inbounds.len(), 3);
        assert_eq!(inbounds[2]["protocol"], "tun");
        assert_eq!(inbounds[2]["settings"]["name"], "wintun");
        assert_eq!(
            inbounds[2]["settings"]["autoSystemRoutingTable"],
            json!(["0.0.0.0/0", "::/0"])
        );
        assert_eq!(
            inbounds[2]["settings"]["autoSystemWfpBlockLeak"],
            json!(["dns", "misconfigtun"])
        );
        assert_eq!(json_val["routing"]["rules"][0]["outboundTag"], "netray-dns");
        assert_eq!(json_val["routing"]["rules"][1]["outboundTag"], "proxy");
    }

    #[test]
    fn test_full_json_gets_a_routed_tun_inbound() {
        let raw = r#"{"inbounds":[],"outbounds":[{"tag":"proxy","protocol":"freedom"}]}"#;
        let config = parse_and_generate_xray_config(raw, 10808, 10809, true).unwrap();
        let config: Value = serde_json::from_str(&config).unwrap();
        assert_eq!(config["inbounds"][2]["protocol"], "tun");
        assert_eq!(config["routing"]["rules"][1]["outboundTag"], "proxy");
    }

    #[test]
    fn test_extract_helpers() {
        let uri = "vless://b831381d-6324-4d53-ad4f-8cda48b30811@example.com:443?encryption=none&security=tls&sni=example.com&type=tcp#Sample-Node";
        assert_eq!(extract_node_address(uri), "example.com:443");
        assert_eq!(extract_protocol_tag(uri), "(VLESS + TCP + TLS)");
    }
}
