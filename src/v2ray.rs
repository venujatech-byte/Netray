#[path = "bootstrap_dns.rs"]
mod bootstrap_dns;
#[path = "latency.rs"]
mod latency;
use base64::prelude::*;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use url::Url;

/// Locate the xray or v2ray binary on the local system
pub fn find_xray_binary() -> Result<PathBuf, String> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(path) = exe.parent().map(|dir| dir.join("xray")) {
            if path.is_file() {
                return Ok(path);
            }
        }
    }

    // 1. Check standard environment PATH via `which`
    if let Ok(path) = which::which("xray").or_else(|_| which::which("v2ray")) {
        return Ok(path);
    }

    // 2. Check well-known fixed installation paths
    let candidates = [
        "/usr/local/bin/xray",
        "/usr/bin/xray",
        "/bin/xray",
        "/opt/xray/xray",
        "/opt/netray/xray",
        "/usr/local/bin/v2ray",
        "/usr/bin/v2ray",
        "/bin/v2ray",
        "/opt/v2ray/v2ray",
    ];

    for candidate in &candidates {
        let p = Path::new(candidate);
        if p.is_file() {
            return Ok(p.to_path_buf());
        }
    }

    Err(
        "Neither 'xray' nor 'v2ray' core binary was found. Place xray beside Netray or install it on PATH."
            .to_string(),
    )
}

/// Fallback helper to search PATH without external crate
mod which {
    use std::path::PathBuf;

    pub fn which(binary_name: &str) -> Result<PathBuf, ()> {
        if let Some(paths) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&paths) {
                let candidate = dir.join(binary_name);
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }
        Err(())
    }
}

/// Converts raw user configuration (vless://, vmess://, trojan://, ss://, or JSON)
/// into a full, valid Xray JSON configuration.
pub fn parse_and_generate_xray_config(
    raw_input: &str,
    socks_port: u16,
    http_port: u16,
    enable_tun: bool,
) -> Result<String, String> {
    let trimmed = raw_input.trim();
    if trimmed.is_empty() {
        return Err(
            "Configuration is empty. Paste a vless://, vmess://, trojan:// URI or Xray JSON."
                .to_string(),
        );
    }

    // Case 1: Raw JSON configuration
    if trimmed.starts_with('{') {
        return handle_json_config(trimmed, socks_port, http_port, enable_tun);
    }

    // Case 2: Protocol URIs
    let outbound = if trimmed.starts_with("vless://") {
        parse_vless_uri(trimmed)?
    } else if trimmed.starts_with("vmess://") {
        parse_vmess_uri(trimmed)?
    } else if trimmed.starts_with("trojan://") {
        parse_trojan_uri(trimmed)?
    } else if trimmed.starts_with("ss://") {
        parse_shadowsocks_uri(trimmed)?
    } else {
        return Err(format!(
            "Unsupported configuration scheme: '{}'. Expected vless://, vmess://, trojan://, ss:// or JSON.",
            trimmed.chars().take(20).collect::<String>()
        ));
    };

    let full_config = assemble_xray_config(outbound, socks_port, http_port, enable_tun);
    serde_json::to_string_pretty(&full_config)
        .map_err(|e| format!("Failed to serialize Xray config: {}", e))
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
                    if !ps.is_empty() {
                        return ps.to_string();
                    }
                }
                if let Some(add) = v.get("add").and_then(|p| p.as_str()) {
                    return add.to_string();
                }
            }
        }
    }

    if let Ok(url) = Url::parse(trimmed) {
        if let Some(host) = url.host_str() {
            return format!("{}:{}", host, url.port().unwrap_or(443));
        }
    }

    "Custom Node".to_string()
}

/// Assemble full Xray configuration containing inbounds (SOCKS5, HTTP, optional TUN), outbounds, and logging
fn assemble_xray_config(
    primary_outbound: Value,
    socks_port: u16,
    http_port: u16,
    enable_tun: bool,
) -> Value {
    let inbounds = vec![
        json!({
            "tag": "socks-in",
            "port": socks_port,
            "listen": "127.0.0.1",
            "protocol": "socks",
            "sniffing": {
                "enabled": true,
                "destOverride": ["http", "tls", "quic"]
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
                "destOverride": ["http", "tls", "quic"]
            }
        }),
    ];

    let mut config = json!({
        "log": {
            "loglevel": "warning"
        },
        "inbounds": inbounds,
        "outbounds": [
            primary_outbound,
            {
                "tag": "direct",
                "protocol": "freedom"
            },
            {
                "tag": "blocked",
                "protocol": "blackhole"
            }
        ]
    });
    if enable_tun {
        configure_tun(&mut config).expect("generated Xray config supports TUN");
    }
    config
}

fn configure_tun(config: &mut Value) -> Result<(), String> {
    let tun = json!({
        "tag": "tun-in",
        "protocol": "tun",
        "settings": {
            "name": "netray0",
            "mtu": 1500,
            "gateway": ["198.18.0.1/30", "fc00::1/64"],
            "autoSystemRoutingTable": ["0.0.0.0/0", "::/0"],
            "autoOutboundsInterface": "auto",
            "autoSystemDnsToGateway": true
        },
        "sniffing": { "enabled": true, "destOverride": ["http", "tls", "quic"] }
    });
    let inbounds = config
        .get_mut("inbounds")
        .and_then(Value::as_array_mut)
        .ok_or("Xray config inbounds must be an array for TUN mode.")?;
    let index = inbounds
        .iter()
        .position(|inbound| inbound["protocol"] == "tun")
        .unwrap_or_else(|| {
            inbounds.push(tun);
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
        "name": "netray0",
        "mtu": 1500,
        "gateway": ["198.18.0.1/30", "fc00::1/64"],
        "autoSystemRoutingTable": ["0.0.0.0/0", "::/0"],
        "autoOutboundsInterface": "auto",
        "autoSystemDnsToGateway": true
    });

    let dns = config
        .as_object_mut()
        .ok_or("Xray config must be a JSON object.")?
        .entry("dns")
        .or_insert_with(
            || json!({ "servers": ["https://1.1.1.1/dns-query", "https://8.8.8.8/dns-query"] }),
        );
    if !dns.is_object() {
        return Err("Xray DNS config must be an object for TUN mode.".to_owned());
    }
    if dns
        .get("servers")
        .is_some_and(|servers| !servers.is_array())
    {
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
                !matches!(
                    outbound["protocol"].as_str(),
                    Some("dns" | "freedom" | "blackhole")
                )
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
    rules.insert(
        0,
        json!({
            "type": "field", "inboundTag": [tag], "port": 53, "outboundTag": dns_tag
        }),
    );
    rules.insert(
        1,
        json!({
            "type": "field", "inboundTag": [tag], "outboundTag": primary_tag
        }),
    );
    Ok(())
}

/// Handle raw JSON configurations (preserving inbounds if present, or wrapping in full config)
fn handle_json_config(
    raw_json: &str,
    socks_port: u16,
    http_port: u16,
    enable_tun: bool,
) -> Result<String, String> {
    let mut parsed: Value =
        serde_json::from_str(raw_json).map_err(|e| format!("Invalid JSON config: {}", e))?;

    // If it already has inbounds and outbounds, ensure loglevel is present
    if parsed.get("inbounds").is_some() && parsed.get("outbounds").is_some() {
        if parsed.get("log").is_none() {
            parsed["log"] = json!({ "loglevel": "warning" });
        }
        if enable_tun {
            configure_tun(&mut parsed)?;
        }
        return serde_json::to_string_pretty(&parsed)
            .map_err(|e| format!("Serialization error: {}", e));
    }

    // If it has outbounds but no inbounds, add standard inbounds
    if let Some(outbounds) = parsed.get("outbounds") {
        let mut full = assemble_xray_config(json!({}), socks_port, http_port, false);
        full["outbounds"] = outbounds.clone();
        if enable_tun {
            configure_tun(&mut full)?;
        }
        return serde_json::to_string_pretty(&full)
            .map_err(|e| format!("Serialization error: {}", e));
    }

    // If it's a single outbound object (has "protocol")
    if parsed.get("protocol").is_some() {
        let full = assemble_xray_config(parsed, socks_port, http_port, enable_tun);
        return serde_json::to_string_pretty(&full)
            .map_err(|e| format!("Serialization error: {}", e));
    }

    Err("JSON config must include 'outbounds' array or represent an outbound object.".to_string())
}

/// Parse VLESS URI: vless://uuid@host:port?param1=val1...#remark
fn parse_vless_uri(uri_str: &str) -> Result<Value, String> {
    let url = Url::parse(uri_str).map_err(|e| format!("Invalid VLESS URI: {}", e))?;

    let uuid = url.username();
    if uuid.is_empty() {
        return Err("VLESS URI missing UUID".to_string());
    }

    let host = url
        .host_str()
        .ok_or_else(|| "VLESS URI missing server host".to_string())?;
    let port = url.port().unwrap_or(443);

    let query: HashMap<String, String> = url.query_pairs().into_owned().collect();

    let encryption = query
        .get("encryption")
        .map(|s| s.as_str())
        .unwrap_or("none");
    let flow = query.get("flow").cloned().unwrap_or_default();
    let pbk = query.get("pbk").map(|s| s.as_str()).unwrap_or_default();
    let sid = query.get("sid").map(|s| s.as_str()).unwrap_or_default();

    let mut user_obj = json!({
        "id": uuid,
        "encryption": encryption,
    });
    if !flow.is_empty() {
        user_obj["flow"] = json!(flow);
    }

    let stream_settings = extract_stream_settings(&query, host, "none", pbk, sid)?;

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

/// Parse VMess URI: vmess://BASE64
fn parse_vmess_uri(uri_str: &str) -> Result<Value, String> {
    let b64 = uri_str.trim_start_matches("vmess://").trim();

    // Decode standard or URL-safe base64, with or without padding
    let decoded_bytes = BASE64_STANDARD
        .decode(b64)
        .or_else(|_| BASE64_STANDARD_NO_PAD.decode(b64))
        .or_else(|_| BASE64_URL_SAFE.decode(b64))
        .or_else(|_| BASE64_URL_SAFE_NO_PAD.decode(b64))
        .map_err(|e| format!("Failed to base64-decode vmess payload: {}", e))?;

    let json_val: Value = serde_json::from_slice(&decoded_bytes)
        .map_err(|e| format!("Invalid JSON in vmess payload: {}", e))?;

    let host = json_val
        .get("add")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "VMess config missing 'add' (server host)".to_string())?;

    let port: u16 = match json_val.get("port") {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(443) as u16,
        Some(Value::String(s)) => s.parse::<u16>().unwrap_or(443),
        _ => 443,
    };

    let uuid = json_val
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "VMess config missing 'id' (UUID)".to_string())?;

    let aid: u32 = match json_val.get("aid") {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(0) as u32,
        Some(Value::String(s)) => s.parse::<u32>().unwrap_or(0),
        _ => 0,
    };

    let scy = json_val
        .get("scy")
        .and_then(|v| v.as_str())
        .unwrap_or("auto");

    let network = json_val
        .get("net")
        .and_then(|v| v.as_str())
        .unwrap_or("tcp");

    let tls = json_val.get("tls").and_then(|v| v.as_str()).unwrap_or("");
    let security = if tls == "tls" { "tls" } else { "none" };

    let sni = json_val.get("sni").and_then(|v| v.as_str()).unwrap_or("");
    let host_header = json_val.get("host").and_then(|v| v.as_str()).unwrap_or("");
    let effective_sni = if !sni.is_empty() {
        sni
    } else if !host_header.is_empty() {
        host_header
    } else {
        host
    };

    let path = json_val.get("path").and_then(|v| v.as_str()).unwrap_or("/");

    let header_type = json_val
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("none");

    let vcn = if !effective_sni.is_empty()
        && effective_sni != host
        && host.parse::<std::net::IpAddr>().is_err()
    {
        host.to_string()
    } else {
        "".to_string()
    };

    let stream_settings = build_stream_settings(
        network,
        security,
        effective_sni,
        host_header,
        path,
        "",
        header_type,
        "chrome",
        "",
        "",
        &vcn,
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
                            "alterId": aid,
                            "security": scy
                        }
                    ]
                }
            ]
        },
        "streamSettings": stream_settings
    }))
}

/// Parse Trojan URI: trojan://password@host:port?param1=val1...#remark
fn parse_trojan_uri(uri_str: &str) -> Result<Value, String> {
    let url = Url::parse(uri_str).map_err(|e| format!("Invalid Trojan URI: {}", e))?;

    let password = url.username();
    if password.is_empty() {
        return Err("Trojan URI missing password".to_string());
    }

    let host = url
        .host_str()
        .ok_or_else(|| "Trojan URI missing server host".to_string())?;
    let port = url.port().unwrap_or(443);

    let query: HashMap<String, String> = url.query_pairs().into_owned().collect();

    let stream_settings = extract_stream_settings(&query, host, "tls", "", "")?;

    Ok(json!({
        "tag": "proxy",
        "protocol": "trojan",
        "settings": {
            "servers": [
                {
                    "address": host,
                    "port": port,
                    "password": password
                }
            ]
        },
        "streamSettings": stream_settings
    }))
}

/// Parse Shadowsocks URI: ss://base64@host:port or ss://base64
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
            .or_else(|_| BASE64_STANDARD_NO_PAD.decode(user_info))
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
        // Entire body might be base64
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

fn extract_stream_settings(
    query: &HashMap<String, String>,
    host: &str,
    default_security: &str,
    pbk: &str,
    sid: &str,
) -> Result<Value, String> {
    let network = query
        .get("type")
        .or_else(|| query.get("net"))
        .map(|s| s.as_str())
        .unwrap_or("tcp");
    let security = query
        .get("security")
        .map(|s| s.as_str())
        .unwrap_or(default_security);
    let sni = query
        .get("sni")
        .or_else(|| query.get("peer"))
        .cloned()
        .unwrap_or_else(|| host.to_string());
    let path = query
        .get("path")
        .cloned()
        .unwrap_or_else(|| "/".to_string());
    let header_host = query.get("host").cloned().unwrap_or_else(|| sni.clone());
    let service_name = query.get("serviceName").cloned().unwrap_or_default();
    let header_type = query
        .get("headerType")
        .cloned()
        .unwrap_or_else(|| "none".to_string());
    let fp = query
        .get("fp")
        .cloned()
        .unwrap_or_else(|| "chrome".to_string());

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

/// Helper to build streamSettings JSON for transports (ws, tcp, grpc, h2) and security (tls, reality, none)
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
            let mut ws_obj = json!({
                "path": path,
            });
            if !host_header.is_empty() {
                ws_obj["headers"] = json!({ "Host": host_header });
            }
            settings["wsSettings"] = ws_obj;
        }
        "grpc" => {
            let mut grpc_obj = json!({
                "multiMode": false
            });
            if !service_name.is_empty() {
                grpc_obj["serviceName"] = json!(service_name);
            }
            settings["grpcSettings"] = grpc_obj;
        }
        "h2" | "http" => {
            let mut h2_obj = json!({
                "path": path,
            });
            if !host_header.is_empty() {
                h2_obj["host"] = json!([host_header]);
            }
            settings["httpSettings"] = h2_obj;
        }
        "tcp" => {
            if header_type.eq_ignore_ascii_case("http") {
                let mut http_obj = json!({
                    "type": "http"
                });
                if !host_header.is_empty() {
                    http_obj["request"] = json!({
                        "headers": {
                            "Host": [host_header]
                        }
                    });
                }
                settings["tcpSettings"] = json!({
                    "header": http_obj
                });
            }
        }
        "kcp" => {
            let mut kcp_obj = json!({});
            if !header_type.is_empty() && !header_type.eq_ignore_ascii_case("none") {
                kcp_obj["header"] = json!({ "type": header_type });
            }
            if !path.is_empty() && path != "/" {
                kcp_obj["seed"] = json!(path);
            }
            settings["kcpSettings"] = kcp_obj;
        }
        _ => {}
    }

    if security == "tls" {
        settings["security"] = json!("tls");
        let mut tls_settings = json!({
            "fingerprint": fp,
        });
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

static TEST_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Validate configuration using `xray run -test -c <file>`
pub async fn test_xray_config(xray_bin: &Path, config_json: &str) -> Result<(), String> {
    // If the config contains a TUN inbound, strip it for dry-run validation.
    // Xray attempts to create the real tun interface during -test mode,
    // which fails with EPERM for non-root users.
    let validation_json = if let Ok(mut val) = serde_json::from_str::<Value>(config_json) {
        if let Some(inbounds) = val.get_mut("inbounds").and_then(|i| i.as_array_mut()) {
            inbounds.retain(|ib| ib.get("protocol").and_then(|p| p.as_str()) != Some("tun"));
        }
        serde_json::to_string(&val).unwrap_or_else(|_| config_json.to_string())
    } else {
        config_json.to_string()
    };

    let count = TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temp_test_file = format!(
        "/tmp/netray_xray_test_{}_{}_{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        count
    );
    tokio::fs::write(&temp_test_file, &validation_json)
        .await
        .map_err(|e| format!("Failed to write test config: {}", e))?;

    let output = tokio::process::Command::new(xray_bin)
        .arg("run")
        .arg("-test")
        .arg("-c")
        .arg(&temp_test_file)
        .output()
        .await;

    let _ = tokio::fs::remove_file(&temp_test_file).await;

    match output {
        Ok(out) => {
            if out.status.success() {
                Ok(())
            } else {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                let combined = format!("{}\n{}", stdout, stderr);
                let error_lines: Vec<&str> = combined
                    .lines()
                    .filter(|l| l.contains("failed") || l.contains("error") || l.contains("Error"))
                    .collect();
                if error_lines.is_empty() {
                    Err(combined.trim().to_string())
                } else {
                    Err(error_lines.join("\n"))
                }
            }
        }
        Err(e) => Err(format!("Failed to execute Xray test: {}", e)),
    }
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

pub async fn pin_proxy_dns(config: &mut Value) -> Result<(), String> {
    bootstrap_dns::pin_proxy_address(config).await
}

pub async fn ping_local_http_proxy(port: u16) -> Result<u128, String> {
    latency::ping_local_http_proxy(port).await
}

pub async fn wait_for_http_proxy(port: u16) -> Result<u128, String> {
    latency::wait_for_http_proxy(port).await
}

pub async fn ping_profile(raw: &str) -> Result<u128, String> {
    let normalized = parse_and_generate_xray_config(raw, 10808, 10809, false)?;
    let mut config: Value =
        serde_json::from_str(&normalized).map_err(|e| format!("Invalid Xray config: {e}"))?;
    config["dns"] = json!({
        "servers": ["https://1.1.1.1/dns-query", "https://8.8.8.8/dns-query"],
        "hosts": {}
    });
    pin_proxy_dns(&mut config).await?;
    latency::measure(&find_xray_binary()?, config).await
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires an installed or packaged Xray core"]
    fn test_find_binary() {
        let bin = find_xray_binary();
        assert!(
            bin.is_ok(),
            "Should locate xray or v2ray on system: {:?}",
            bin
        );
        assert!(bin.unwrap().exists());
    }

    #[tokio::test]
    async fn test_vless_generation() {
        let uri = "vless://b831381d-6324-4d53-ad4f-8cda48b30811@example.com:443?encryption=none&security=tls&sni=example.com&type=ws&path=%2F#Sample-Node";
        let config =
            parse_and_generate_xray_config(uri, 10808, 10809, false).expect("Should parse VLESS");

        let json_val: Value = serde_json::from_str(&config).expect("Must be valid JSON");
        assert_eq!(json_val["inbounds"][0]["port"], 10808);
        assert_eq!(json_val["inbounds"][1]["port"], 10809);
        assert_eq!(json_val["outbounds"][0]["protocol"], "vless");

        assert_eq!(extract_node_name(uri), "Sample-Node");

        if let Ok(bin) = find_xray_binary() {
            let res = test_xray_config(&bin, &config).await;
            assert!(res.is_ok(), "Xray test validation failed: {:?}", res);
        }
    }

    #[tokio::test]
    async fn test_vmess_generation() {
        let vmess_raw = json!({
            "v": "2",
            "ps": "test-vmess",
            "add": "example.com",
            "port": 443,
            "id": "b831381d-6324-4d53-ad4f-8cda48b30811",
            "aid": 0,
            "scy": "auto",
            "net": "ws",
            "type": "none",
            "host": "example.com",
            "path": "/ws",
            "tls": "tls",
            "sni": "example.com"
        });
        let b64 = BASE64_STANDARD.encode(serde_json::to_string(&vmess_raw).unwrap());
        let uri = format!("vmess://{}", b64);

        let config =
            parse_and_generate_xray_config(&uri, 10808, 10809, false).expect("Should parse VMess");
        let json_val: Value = serde_json::from_str(&config).expect("Must be valid JSON");
        assert_eq!(json_val["outbounds"][0]["protocol"], "vmess");
        assert_eq!(extract_node_name(&uri), "test-vmess");

        if let Ok(bin) = find_xray_binary() {
            let res = test_xray_config(&bin, &config).await;
            assert!(
                res.is_ok(),
                "Xray test validation failed for vmess: {:?}",
                res
            );
        }
    }

    #[tokio::test]
    async fn test_trojan_generation() {
        let uri = "trojan://secretpassword@example.com:443?security=tls&sni=example.com&type=tcp#Trojan-Node";
        let config =
            parse_and_generate_xray_config(uri, 10808, 10809, false).expect("Should parse Trojan");

        let json_val: Value = serde_json::from_str(&config).expect("Must be valid JSON");
        assert_eq!(json_val["outbounds"][0]["protocol"], "trojan");
        assert_eq!(extract_node_name(uri), "Trojan-Node");

        if let Ok(bin) = find_xray_binary() {
            let res = test_xray_config(&bin, &config).await;
            assert!(
                res.is_ok(),
                "Xray test validation failed for trojan: {:?}",
                res
            );
        }
    }

    #[tokio::test]
    async fn test_raw_json_handling() {
        let raw = r#"{
            "outbounds": [
                {
                    "tag": "proxy",
                    "protocol": "freedom"
                }
            ]
        }"#;
        let config = parse_and_generate_xray_config(raw, 10808, 10809, false)
            .expect("Should handle raw JSON");
        let json_val: Value = serde_json::from_str(&config).expect("Must be valid JSON");
        assert_eq!(json_val["inbounds"][0]["port"], 10808);

        if let Ok(bin) = find_xray_binary() {
            let res = test_xray_config(&bin, &config).await;
            assert!(
                res.is_ok(),
                "Xray test validation failed for raw json: {:?}",
                res
            );
        }
    }

    #[tokio::test]
    async fn test_tun_inbound_generation() {
        let uri = "vless://b831381d-6324-4d53-ad4f-8cda48b30811@example.com:443?encryption=none&security=tls&sni=example.com&type=ws&path=%2F#Sample-Node";
        let config =
            parse_and_generate_xray_config(uri, 10808, 10809, true).expect("Should parse with TUN");
        let json_val: Value = serde_json::from_str(&config).expect("Must be valid JSON");

        let inbounds = json_val["inbounds"]
            .as_array()
            .expect("inbounds must be array");
        assert_eq!(inbounds.len(), 3);
        assert_eq!(inbounds[2]["protocol"], "tun");
        assert_eq!(inbounds[2]["settings"]["name"], "netray0");
        assert_eq!(
            inbounds[2]["settings"]["autoSystemRoutingTable"],
            json!(["0.0.0.0/0", "::/0"])
        );
        assert_eq!(inbounds[2]["settings"]["autoSystemDnsToGateway"], true);
        assert_eq!(json_val["routing"]["rules"][0]["outboundTag"], "netray-dns");
        assert_eq!(json_val["routing"]["rules"][1]["outboundTag"], "proxy");

        if let Ok(bin) = find_xray_binary() {
            let res = test_xray_config(&bin, &config).await;
            assert!(
                res.is_ok(),
                "Xray validation should succeed with TUN: {:?}",
                res
            );
        }
    }

    #[test]
    fn test_full_json_gets_a_routed_tun_inbound() {
        let raw = r#"{"inbounds":[],"outbounds":[{"tag":"proxy","protocol":"freedom"}]}"#;
        let config = parse_and_generate_xray_config(raw, 10808, 10809, true).unwrap();
        let config: Value = serde_json::from_str(&config).unwrap();
        assert_eq!(config["inbounds"][0]["protocol"], "tun");
        assert_eq!(config["routing"]["rules"][1]["outboundTag"], "proxy");
    }

    #[test]
    fn test_vless_tcp_no_fake_http() {
        let uri = "vless://b831381d-6324-4d53-ad4f-8cda48b30811@example.com:443?encryption=none&security=tls&sni=example.com&type=tcp#TcpNode";
        let config =
            parse_and_generate_xray_config(uri, 10808, 10809, false).expect("Should parse");
        let json_val: Value = serde_json::from_str(&config).expect("Must be valid JSON");
        let stream = &json_val["outbounds"][0]["streamSettings"];
        assert_eq!(stream["network"], "tcp");
        assert!(
            stream.get("tcpSettings").is_none(),
            "Standard TCP must NOT have fake HTTP tcpSettings"
        );
    }

    #[test]
    fn test_vless_tcp_with_http_obfuscation() {
        let uri = "vless://b831381d-6324-4d53-ad4f-8cda48b30811@example.com:443?encryption=none&security=tls&sni=example.com&type=tcp&headerType=http&host=example.com#HttpTcpNode";
        let config =
            parse_and_generate_xray_config(uri, 10808, 10809, false).expect("Should parse");
        let json_val: Value = serde_json::from_str(&config).expect("Must be valid JSON");
        let stream = &json_val["outbounds"][0]["streamSettings"];
        assert_eq!(stream["network"], "tcp");
        assert_eq!(stream["tcpSettings"]["header"]["type"], "http");
        assert_eq!(
            stream["tcpSettings"]["header"]["request"]["headers"]["Host"][0],
            "example.com"
        );
    }

    #[tokio::test]
    async fn test_sni_bughost_verify_peer_cert() {
        let uri = "vless://b831381d-6324-4d53-ad4f-8cda48b30811@server.example.com:443?encryption=none&security=tls&sni=example.com&type=ws&path=%2F#BugHostNode";
        let config =
            parse_and_generate_xray_config(uri, 10808, 10809, false).expect("Should parse");
        let json_val: Value = serde_json::from_str(&config).expect("Must be valid JSON");

        let tls_settings = &json_val["outbounds"][0]["streamSettings"]["tlsSettings"];
        assert_eq!(tls_settings["serverName"], "example.com");
        assert_eq!(tls_settings["verifyPeerCertByName"], "server.example.com");

        if let Ok(bin) = find_xray_binary() {
            let res = test_xray_config(&bin, &config).await;
            assert!(
                res.is_ok(),
                "Xray validation failed for bug host: {:?}",
                res
            );
        }
    }

    #[test]
    fn test_extract_helpers() {
        let uri = "vless://b831381d-6324-4d53-ad4f-8cda48b30811@example.com:443?encryption=none&security=tls&sni=example.com&type=tcp#Sample-Node";
        assert_eq!(extract_node_address(uri), "example.com:443");
        assert_eq!(extract_protocol_tag(uri), "(VLESS + TCP + TLS)");

        let ws_uri = "vless://b831381d-6324-4d53-ad4f-8cda48b30811@example.com:80?encryption=none&type=ws#Sample";
        assert_eq!(extract_node_address(ws_uri), "example.com:80");
        assert_eq!(extract_protocol_tag(ws_uri), "(VLESS + WS)");
    }

    #[test]
    fn test_node_edit_state_roundtrip() {
        let uri = "vless://00000000-0000-0000-0000-000000000000@example.com:443?encryption=none&flow=none&security=tls&sni=example.com&type=tcp&fp=chrome#Sample-Node";
        let state = NodeEditState::from_profile("test-id", "Sample-Node", uri);
        assert_eq!(state.remarks, "Sample-Node");
        assert_eq!(state.address, "example.com");
        assert_eq!(state.port, "443");
        assert_eq!(state.user_id, "00000000-0000-0000-0000-000000000000");
        assert_eq!(state.transport_protocol, "tcp");
        assert_eq!(state.tls_type, "tls");
        assert_eq!(state.sni, "example.com");
        assert_eq!(state.fingerprint, "chrome");

        let generated_uri = state.to_vless_uri();
        assert!(generated_uri
            .starts_with("vless://00000000-0000-0000-0000-000000000000@example.com:443"));
        assert!(generated_uri.contains("sni=example.com"));
        assert_eq!(extract_node_name(&generated_uri), "Sample-Node");
    }
}
