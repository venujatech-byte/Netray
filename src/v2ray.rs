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
    parse_and_generate_xray_config_with_whitelist(
        raw_input,
        socks_port,
        http_port,
        enable_tun,
        "",
        "",
    )
}

/// Converts raw user configuration into a full Xray JSON configuration with optional
/// split-tunneling whitelist rules for processes and domains/URLs.
pub fn parse_and_generate_xray_config_with_whitelist(
    raw_input: &str,
    socks_port: u16,
    http_port: u16,
    enable_tun: bool,
    whitelist_apps: &str,
    whitelist_urls: &str,
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
        return handle_json_config(
            trimmed,
            socks_port,
            http_port,
            enable_tun,
            whitelist_apps,
            whitelist_urls,
        );
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
    } else if trimmed.starts_with("socks://") || trimmed.starts_with("socks5://") {
        parse_socks_uri(trimmed)?
    } else if trimmed.starts_with("wireguard://") || trimmed.starts_with("wg://") {
        parse_wireguard_uri(trimmed)?
    } else if trimmed.starts_with("hy2://") || trimmed.starts_with("hysteria2://") {
        parse_hysteria2_uri(trimmed)?
    } else if trimmed.starts_with("ssr://") {
        parse_shadowsocksr_uri(trimmed)?
    } else {
        return Err(format!(
            "Unsupported configuration scheme: '{}'. Expected vless://, vmess://, trojan://, ss:// or JSON.",
            trimmed.chars().take(20).collect::<String>()
        ));
    };

    let full_config = assemble_xray_config(
        outbound,
        socks_port,
        http_port,
        enable_tun,
        whitelist_apps,
        whitelist_urls,
    );
    serde_json::to_string_pretty(&full_config)
        .map_err(|e| format!("Failed to serialize Xray config: {}", e))
}

/// Extracts clean domain and IP rule matchers from user-specified URLs and domains.
/// Supports full URLs (e.g. `https://mywork.com/login`), wildcards (`*.internal.net`),
/// standard domains (`github.com`), IP addresses (`192.168.1.1`), and CIDRs (`10.0.0.0/8`).
pub fn parse_whitelist_domains_and_ips(raw: &str) -> (Vec<String>, Vec<String>) {
    let mut domains = Vec::new();
    let mut ips = Vec::new();

    for token in raw.split(|c| c == ',' || c == ';' || c == '\n' || c == '\r' || c == '\t') {
        let trimmed = token.trim().trim_matches(&['"', '\'', '`'][..]).trim();
        if trimmed.is_empty() {
            continue;
        }

        // Case 1: Full URL (http://... or https://...)
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            if let Ok(parsed_url) = url::Url::parse(trimmed) {
                if let Some(host) = parsed_url.host_str() {
                    let host_clean = host.trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
                    if host_clean.parse::<std::net::IpAddr>().is_ok() {
                        if !ips.contains(&host_clean) {
                            ips.push(host_clean);
                        }
                    } else if !domains.iter().any(|d| d == &host_clean || d == &format!("domain:{host_clean}")) {
                        domains.push(format!("domain:{host_clean}"));
                    }
                    continue;
                }
            }
        }

        // Case 2: Direct IP address or CIDR
        if trimmed.parse::<std::net::IpAddr>().is_ok()
            || (trimmed.contains('/')
                && trimmed
                    .split('/')
                    .next()
                    .and_then(|ip| ip.parse::<std::net::IpAddr>().ok())
                    .is_some())
        {
            if !ips.contains(&trimmed.to_string()) {
                ips.push(trimmed.to_string());
            }
            continue;
        }

        // Case 3: GeoIP or special Xray IP matcher
        if trimmed.starts_with("geoip:") {
            if !ips.contains(&trimmed.to_string()) {
                ips.push(trimmed.to_string());
            }
            continue;
        }

        // Case 4: Xray domain match prefixes (domain:, full:, regexp:, keyword:, geosite:)
        if trimmed.starts_with("domain:")
            || trimmed.starts_with("full:")
            || trimmed.starts_with("regexp:")
            || trimmed.starts_with("keyword:")
            || trimmed.starts_with("geosite:")
        {
            let normalized = trimmed.to_ascii_lowercase();
            if !domains.contains(&normalized) {
                domains.push(normalized);
            }
            continue;
        }

        // Case 5: Wildcard domain (*.example.com -> domain:example.com)
        let domain_clean = if let Some(stripped) = trimmed.strip_prefix("*.") {
            format!("domain:{}", stripped.to_ascii_lowercase())
        } else {
            format!("domain:{}", trimmed.to_ascii_lowercase())
        };

        if !domains.contains(&domain_clean) {
            domains.push(domain_clean);
        }
    }

    (domains, ips)
}

/// Extracts clean process executable names from user-specified app list.
/// Handles file paths (e.g. `/usr/bin/curl` -> `curl`, `C:\...\app.exe` -> `app.exe`),
/// whitespace, commas, semicolons, quotes, and newlines.
pub fn parse_whitelist_processes(raw: &str) -> Vec<String> {
    let mut processes = Vec::new();

    for token in raw.split(|c| c == ',' || c == ';' || c == '\n' || c == '\r' || c == '\t') {
        let trimmed = token.trim().trim_matches(&['"', '\'', '`'][..]).trim();
        if trimmed.is_empty() {
            continue;
        }

        // Strip file path components if user entered a full path
        let file_name = if let Some((_, name)) = trimmed.rsplit_once('/') {
            name
        } else if let Some((_, name)) = trimmed.rsplit_once('\\') {
            name
        } else {
            trimmed
        };

        let file_name = file_name.trim();
        if !file_name.is_empty() && !processes.contains(&file_name.to_string()) {
            processes.push(file_name.to_string());
        }
    }

    processes
}

/// Builds routing rules targeting direct outbound for whitelisted domains, URLs, and IPs
pub fn build_whitelist_domain_and_ip_rules(whitelist_urls: &str) -> Vec<Value> {
    let mut rules = Vec::new();
    let (domains, ips) = parse_whitelist_domains_and_ips(whitelist_urls);

    if !domains.is_empty() {
        rules.push(json!({
            "type": "field",
            "outboundTag": "direct",
            "domain": domains
        }));
    }

    if !ips.is_empty() {
        rules.push(json!({
            "type": "field",
            "outboundTag": "direct",
            "ip": ips
        }));
    }

    rules
}

/// Builds routing rules targeting direct outbound for whitelisted applications, domains, URLs, and IPs
pub fn build_whitelist_routing_rules(
    whitelist_apps: &str,
    whitelist_urls: &str,
) -> Vec<Value> {
    let mut rules = build_whitelist_domain_and_ip_rules(whitelist_urls);
    let processes = parse_whitelist_processes(whitelist_apps);

    if !processes.is_empty() {
        rules.push(json!({
            "type": "field",
            "outboundTag": "direct",
            "process": processes
        }));
    }

    rules
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

/// Generate a shareable protocol URI link from a profile config and remark.
/// If the config is already a URI, returns it (adding the remark if missing).
/// If the config is a raw JSON configuration, extracts the outbound parameters
/// and generates the appropriate protocol link (VLESS, VMess, Trojan, Shadowsocks, Socks5, WireGuard, Hysteria2).
pub fn generate_share_link(config: &str, remark: &str) -> String {
    let trimmed = config.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let encoded_remark = percent_encoding::utf8_percent_encode(
        if remark.trim().is_empty() { "Netray-Node" } else { remark.trim() },
        percent_encoding::NON_ALPHANUMERIC,
    )
    .to_string();

    // 1. If it's already a standard protocol URI
    let known_schemes = [
        "vless://", "vmess://", "trojan://", "ss://", "socks://", "socks5://",
        "wireguard://", "wg://", "hy2://", "hysteria2://", "ssr://", "ssh://",
        "dnstt+ssh://", "http://", "https://",
    ];

    for scheme in known_schemes {
        if trimmed.starts_with(scheme) {
            if !trimmed.contains('#') {
                return format!("{}#{}", trimmed, encoded_remark);
            }
            return trimmed.to_string();
        }
    }

    // 2. If it's a JSON configuration, parse outbounds and construct the appropriate link
    if trimmed.starts_with('{') {
        if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
            if let Some(link) = extract_link_from_json(&v, remark, &encoded_remark) {
                return link;
            }
            // Fallback for custom raw JSON: generate compact base64 v2ray URI
            let b64 = BASE64_STANDARD.encode(trimmed.as_bytes());
            return format!("v2ray://config?data={}#{}", b64, encoded_remark);
        }
    }

    // 3. Fallback for host:port or custom text
    format!("vless://{}#{}", trimmed, encoded_remark)
}

fn extract_link_from_json(v: &Value, remark: &str, encoded_remark: &str) -> Option<String> {
    let outbounds = v
        .get("outbounds")
        .and_then(|o| o.as_array())
        .or_else(|| v.get("outbound").and_then(|o| o.as_array()))?;

    // Find the primary proxy outbound (exclude freedom, blackhole, dns)
    let outbound = outbounds
        .iter()
        .find(|o| {
            let proto = o.get("protocol").and_then(|p| p.as_str()).unwrap_or("");
            proto != "freedom" && proto != "blackhole" && proto != "dns"
        })
        .or_else(|| outbounds.first())?;

    let proto = outbound.get("protocol").and_then(|p| p.as_str())?;
    let settings = outbound.get("settings").unwrap_or(&Value::Null);
    let stream = outbound.get("streamSettings").unwrap_or(&Value::Null);

    let network = stream.get("network").and_then(|n| n.as_str()).unwrap_or("tcp");
    let security = stream.get("security").and_then(|s| s.as_str()).unwrap_or("none");

    match proto {
        "vless" => {
            let vnext = settings.get("vnext").and_then(|vn| vn.as_array())?.first()?;
            let address = vnext.get("address").and_then(|a| a.as_str()).unwrap_or("127.0.0.1");
            let port = vnext.get("port").and_then(|p| p.as_u64()).unwrap_or(443);
            let user = vnext.get("users").and_then(|u| u.as_array())?.first()?;
            let uuid = user.get("id").and_then(|i| i.as_str()).unwrap_or("00000000-0000-0000-0000-000000000000");
            let flow = user.get("flow").and_then(|f| f.as_str()).unwrap_or("");

            let mut query_params = Vec::new();
            if network != "tcp" {
                query_params.push(format!("type={}", network));
            }
            if security != "none" {
                query_params.push(format!("security={}", security));
            }
            if !flow.is_empty() {
                query_params.push(format!("flow={}", flow));
            }
            if let Some(sni) = stream.get("tlsSettings").and_then(|t| t.get("serverName")).and_then(|s| s.as_str()) {
                query_params.push(format!("sni={}", sni));
            } else if let Some(sni) = stream.get("realitySettings").and_then(|r| r.get("serverName")).and_then(|s| s.as_str()) {
                query_params.push(format!("sni={}", sni));
            }
            if let Some(pbk) = stream.get("realitySettings").and_then(|r| r.get("publicKey")).and_then(|s| s.as_str()) {
                query_params.push(format!("pbk={}", pbk));
            }
            if let Some(sid) = stream.get("realitySettings").and_then(|r| r.get("shortId")).and_then(|s| s.as_str()) {
                query_params.push(format!("sid={}", sid));
            }
            if let Some(ws_path) = stream.get("wsSettings").and_then(|w| w.get("path")).and_then(|s| s.as_str()) {
                query_params.push(format!("path={}", percent_encoding::utf8_percent_encode(ws_path, percent_encoding::NON_ALPHANUMERIC)));
            }
            let query_str = if query_params.is_empty() {
                String::new()
            } else {
                format!("?{}", query_params.join("&"))
            };
            Some(format!("vless://{}@{}:{}{}#{}", uuid, address, port, query_str, encoded_remark))
        }
        "vmess" => {
            let vnext = settings.get("vnext").and_then(|vn| vn.as_array())?.first()?;
            let address = vnext.get("address").and_then(|a| a.as_str()).unwrap_or("127.0.0.1");
            let port = vnext.get("port").and_then(|p| p.as_u64()).unwrap_or(443);
            let user = vnext.get("users").and_then(|u| u.as_array())?.first()?;
            let uuid = user.get("id").and_then(|i| i.as_str()).unwrap_or("00000000-0000-0000-0000-000000000000");
            let alter_id = user.get("alterId").and_then(|a| a.as_u64()).unwrap_or(0);
            let sni = stream.get("tlsSettings").and_then(|t| t.get("serverName")).and_then(|s| s.as_str()).unwrap_or("");
            let path = stream.get("wsSettings").and_then(|w| w.get("path")).and_then(|s| s.as_str()).unwrap_or("/");

            let vmess_obj = json!({
                "v": "2",
                "ps": remark,
                "add": address,
                "port": port,
                "id": uuid,
                "aid": alter_id,
                "scy": "auto",
                "net": network,
                "type": "none",
                "host": sni,
                "path": path,
                "tls": if security == "tls" { "tls" } else { "" },
                "sni": sni,
            });
            let b64 = BASE64_STANDARD.encode(vmess_obj.to_string().as_bytes());
            Some(format!("vmess://{}", b64))
        }
        "trojan" => {
            let server = settings.get("servers").and_then(|s| s.as_array())?.first()?;
            let address = server.get("address").and_then(|a| a.as_str()).unwrap_or("127.0.0.1");
            let port = server.get("port").and_then(|p| p.as_u64()).unwrap_or(443);
            let password = server.get("password").and_then(|p| p.as_str()).unwrap_or("");
            let sni = stream.get("tlsSettings").and_then(|t| t.get("serverName")).and_then(|s| s.as_str()).unwrap_or("");

            Some(format!("trojan://{}@{}:{}?security=tls&sni={}#{}", password, address, port, sni, encoded_remark))
        }
        "shadowsocks" => {
            let server = settings.get("servers").and_then(|s| s.as_array())?.first()?;
            let address = server.get("address").and_then(|a| a.as_str()).unwrap_or("127.0.0.1");
            let port = server.get("port").and_then(|p| p.as_u64()).unwrap_or(8388);
            let method = server.get("method").and_then(|m| m.as_str()).unwrap_or("aes-128-gcm");
            let password = server.get("password").and_then(|p| p.as_str()).unwrap_or("");
            let auth = BASE64_STANDARD.encode(format!("{}:{}", method, password).as_bytes());

            Some(format!("ss://{}@{}:{}#{}", auth, address, port, encoded_remark))
        }
        "socks" => {
            let server = settings.get("servers").and_then(|s| s.as_array())?.first()?;
            let address = server.get("address").and_then(|a| a.as_str()).unwrap_or("127.0.0.1");
            let port = server.get("port").and_then(|p| p.as_u64()).unwrap_or(1080);
            Some(format!("socks5://{}:{}#{}", address, port, encoded_remark))
        }
        "wireguard" => {
            let secret_key = settings.get("secretKey").and_then(|s| s.as_str()).unwrap_or("");
            let address = settings.get("address").and_then(|a| a.as_str()).unwrap_or("127.0.0.1");
            let port = settings.get("port").and_then(|p| p.as_u64()).unwrap_or(51820);
            Some(format!("wireguard://{}@{}:{}#{}", secret_key, address, port, encoded_remark))
        }
        "hysteria2" | "hy2" => {
            let server = settings.get("servers").and_then(|s| s.as_array())?.first()?;
            let address = server.get("address").and_then(|a| a.as_str()).unwrap_or("127.0.0.1");
            let port = server.get("port").and_then(|p| p.as_u64()).unwrap_or(443);
            let password = server.get("password").and_then(|p| p.as_str()).unwrap_or("");
            Some(format!("hy2://{}@{}:{}#{}", password, address, port, encoded_remark))
        }
        _ => None,
    }
}

/// Queries the current external public IP address using `curl ifconfig.me`
pub async fn check_public_ip() -> Result<String, String> {
    tokio::task::spawn_blocking(|| {
        let output = std::process::Command::new("curl")
            .args(["-s", "-m", "5", "https://ifconfig.me"])
            .output()
            .or_else(|_| {
                std::process::Command::new("curl")
                    .args(["-s", "-m", "5", "http://ifconfig.me"])
                    .output()
            })
            .map_err(|e| format!("curl execution failed: {e}"))?;

        if !output.status.success() {
            return Err(format!("curl exited with code {:?}", output.status.code()));
        }

        let raw = String::from_utf8_lossy(&output.stdout);
        parse_ifconfig_output(&raw)
    })
    .await
    .unwrap_or_else(|e| Err(format!("Task execution error: {e}")))
}

/// Parses and validates the IP address returned by ifconfig.me
pub fn parse_ifconfig_output(raw: &str) -> Result<String, String> {
    let clean = raw.trim();
    if clean.is_empty() {
        return Err("Empty response from ifconfig.me".to_string());
    }
    // Validate if it is a valid IPv4 or IPv6 address
    if clean.parse::<std::net::IpAddr>().is_ok() {
        Ok(clean.to_string())
    } else if clean.len() <= 45 && !clean.contains('\n') && !clean.contains(' ') {
        Ok(clean.to_string())
    } else {
        Err(format!("Invalid IP format received: {clean}"))
    }
}

/// Assemble full Xray configuration containing inbounds (SOCKS5, HTTP, optional TUN), outbounds, and logging
fn assemble_xray_config(
    primary_outbound: Value,
    socks_port: u16,
    http_port: u16,
    enable_tun: bool,
    whitelist_apps: &str,
    whitelist_urls: &str,
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

    let primary_tag = primary_outbound["tag"]
        .as_str()
        .unwrap_or("proxy")
        .to_string();
    let mut routing_rules = build_whitelist_routing_rules(whitelist_apps, whitelist_urls);

    if !routing_rules.is_empty() && !enable_tun {
        routing_rules.push(json!({
            "type": "field",
            "inboundTag": ["socks-in", "http-in"],
            "outboundTag": primary_tag
        }));
    }

    let mut config = json!({
        "log": {
            "loglevel": "info",
            "access": ""
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

    if !routing_rules.is_empty() && !enable_tun {
        config["routing"] = json!({
            "domainStrategy": "IPIfNonMatch",
            "rules": routing_rules
        });
    }

    if enable_tun {
        configure_tun(&mut config, whitelist_apps, whitelist_urls).expect("generated Xray config supports TUN");
    }
    config
}

fn configure_tun(
    config: &mut Value,
    whitelist_apps: &str,
    whitelist_urls: &str,
) -> Result<(), String> {
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
    // 1. DNS routing rule (port 53 always goes to DNS outbound)
    rules.insert(
        0,
        json!({
            "type": "field", "inboundTag": [tag.clone()], "port": 53, "outboundTag": dns_tag
        }),
    );

    // 2. Direct whitelist routing rules (for bypassed apps, domains, and IPs)
    let whitelist_rules = build_whitelist_routing_rules(whitelist_apps, whitelist_urls);
    let mut insert_idx = 1;
    for rule in whitelist_rules {
        rules.insert(insert_idx, rule);
        insert_idx += 1;
    }

    // 3. Fallback: all remaining traffic from TUN goes to primary proxy outbound
    rules.insert(
        insert_idx,
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
    whitelist_apps: &str,
    whitelist_urls: &str,
) -> Result<String, String> {
    let mut parsed: Value =
        serde_json::from_str(raw_json).map_err(|e| format!("Invalid JSON config: {}", e))?;

    // Ensure direct outbound exists
    if let Some(outbounds) = parsed.get_mut("outbounds").and_then(Value::as_array_mut) {
        if !outbounds.iter().any(|o| o["tag"] == "direct") {
            outbounds.push(json!({ "tag": "direct", "protocol": "freedom" }));
        }
    }

    // If it already has inbounds and outbounds, ensure loglevel is present
    if parsed.get("inbounds").is_some() && parsed.get("outbounds").is_some() {
        if parsed.get("log").is_none() {
            parsed["log"] = json!({ "loglevel": "info", "access": "" });
        }
        let whitelist_rules = build_whitelist_routing_rules(whitelist_apps, whitelist_urls);
        if !whitelist_rules.is_empty() {
            let routing = parsed
                .as_object_mut()
                .ok_or("Xray config must be an object")?
                .entry("routing")
                .or_insert_with(|| json!({ "rules": [] }));
            if let Some(rules) = routing.get_mut("rules").and_then(Value::as_array_mut) {
                for (i, r) in whitelist_rules.into_iter().enumerate() {
                    rules.insert(i, r);
                }
            }
        }
        if enable_tun {
            configure_tun(&mut parsed, whitelist_apps, whitelist_urls)?;
        }
        return serde_json::to_string_pretty(&parsed)
            .map_err(|e| format!("Serialization error: {}", e));
    }

    // If it has outbounds but no inbounds, add standard inbounds
    if let Some(outbounds) = parsed.get("outbounds") {
        let mut full = assemble_xray_config(
            json!({}),
            socks_port,
            http_port,
            false,
            whitelist_apps,
            whitelist_urls,
        );
        full["outbounds"] = outbounds.clone();
        if enable_tun {
            configure_tun(&mut full, whitelist_apps, whitelist_urls)?;
        }
        return serde_json::to_string_pretty(&full)
            .map_err(|e| format!("Serialization error: {}", e));
    }

    // If it's a single outbound object (has "protocol")
    if parsed.get("protocol").is_some() {
        let full = assemble_xray_config(
            parsed,
            socks_port,
            http_port,
            enable_tun,
            whitelist_apps,
            whitelist_urls,
        );
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

/// Parse ShadowsocksR URI: ssr://base64
fn parse_shadowsocksr_uri(uri_str: &str) -> Result<Value, String> {
    let body = uri_str.trim_start_matches("ssr://").trim();
    let decoded_str = if let Ok(bytes) = BASE64_URL_SAFE
        .decode(body)
        .or_else(|_| BASE64_STANDARD.decode(body))
        .or_else(|_| BASE64_STANDARD_NO_PAD.decode(body))
    {
        String::from_utf8_lossy(&bytes).to_string()
    } else {
        body.to_string()
    };

    let (main_part, _query) = decoded_str.split_once("/?").unwrap_or((&decoded_str, ""));
    let parts: Vec<&str> = main_part.split(':').collect();
    if parts.len() < 6 {
        return Err("Invalid SSR format: expected host:port:protocol:method:obfs:password".to_string());
    }

    let host = parts[0];
    let port = parts[1].parse::<u16>().map_err(|_| "Invalid SSR port")?;
    let _protocol = parts[2];
    let method = parts[3];
    let _obfs = parts[4];
    let raw_pass = parts[5];

    let password = if let Ok(bytes) = BASE64_URL_SAFE
        .decode(raw_pass)
        .or_else(|_| BASE64_STANDARD.decode(raw_pass))
        .or_else(|_| BASE64_STANDARD_NO_PAD.decode(raw_pass))
    {
        String::from_utf8_lossy(&bytes).to_string()
    } else {
        raw_pass.to_string()
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

/// Parse Socks URI: socks://user:password@host:port#Name or socks5://host:port#Name
fn parse_socks_uri(uri_str: &str) -> Result<Value, String> {
    let clean_uri = if let Some(stripped) = uri_str.strip_prefix("socks5://") {
        format!("socks://{}", stripped)
    } else {
        uri_str.to_string()
    };
    let url = Url::parse(&clean_uri).map_err(|e| format!("Invalid Socks URL: {}", e))?;
    let host = url.host_str().ok_or("Socks URL missing host")?;
    let port = url.port().unwrap_or(1080);

    let mut users = Vec::new();
    let user = url.username();
    if !user.is_empty() {
        let pass = url.password().unwrap_or("");
        users.push(json!({
            "user": user,
            "pass": pass,
            "level": 0
        }));
    }

    let mut server = json!({
        "address": host,
        "port": port
    });
    if !users.is_empty() {
        server["users"] = Value::Array(users);
    }

    Ok(json!({
        "tag": "proxy",
        "protocol": "socks",
        "settings": {
            "servers": [server]
        }
    }))
}

/// Parse WireGuard URI: wireguard://private_key@endpoint:port/?public_key=...&address=10.0.0.2/32#Name
fn parse_wireguard_uri(uri_str: &str) -> Result<Value, String> {
    let clean_uri = if let Some(stripped) = uri_str.strip_prefix("wireguard://") {
        format!("wg://{}", stripped)
    } else {
        uri_str.to_string()
    };
    let url = Url::parse(&clean_uri).map_err(|e| format!("Invalid WireGuard URL: {}", e))?;
    let raw_secret = url.username();
    if raw_secret.is_empty() {
        return Err("WireGuard URL missing secret key (private key)".to_string());
    }
    let secret_key = percent_encoding::percent_decode_str(raw_secret)
        .decode_utf8_lossy()
        .to_string();
    let host = url.host_str().ok_or("WireGuard URL missing host")?;
    let port = url.port().unwrap_or(51820);
    let mut public_key = String::new();
    let mut address = vec!["10.0.0.2/32".to_string()];
    for (k, v) in url.query_pairs() {
        if k == "public_key" || k == "pubkey" || k == "peer_public_key" {
            public_key = v.to_string();
        } else if k == "address" || k == "ip" {
            address = v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        }
    }
    if public_key.is_empty() {
        return Err("WireGuard URL missing public_key query parameter".to_string());
    }

    Ok(json!({
        "tag": "proxy",
        "protocol": "wireguard",
        "settings": {
            "secretKey": secret_key,
            "address": address,
            "peers": [
                {
                    "publicKey": public_key,
                    "endpoint": format!("{}:{}", host, port)
                }
            ]
        }
    }))
}

/// Parse Hysteria2 URI: hy2://password@host:port/?insecure=1&sni=example.com#Name
fn parse_hysteria2_uri(uri_str: &str) -> Result<Value, String> {
    let clean_uri = if let Some(stripped) = uri_str.strip_prefix("hy2://") {
        format!("hysteria2://{}", stripped)
    } else {
        uri_str.to_string()
    };
    let url = Url::parse(&clean_uri).map_err(|e| format!("Invalid Hysteria2 URL: {}", e))?;
    let password = url.username();
    let host = url.host_str().ok_or("Hysteria2 URL missing host")?;
    let port = url.port().unwrap_or(443);

    let mut sni = host.to_string();
    let mut insecure = false;
    for (k, v) in url.query_pairs() {
        if k == "sni" {
            sni = v.to_string();
        } else if k == "insecure" && (v == "1" || v == "true") {
            insecure = true;
        }
    }

    Ok(json!({
        "tag": "proxy",
        "protocol": "hysteria2",
        "settings": {
            "servers": [
                {
                    "address": host,
                    "port": port,
                    "password": password
                }
            ]
        },
        "streamSettings": {
            "network": "udp",
            "security": "tls",
            "tlsSettings": {
                "serverName": sni,
                "allowInsecure": insecure
            }
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
        if trimmed.to_lowercase().contains("2022-") {
            "(SHADOWSOCKS-2022)".to_string()
        } else {
            "(SHADOWSOCKS)".to_string()
        }
    } else if trimmed.starts_with("socks://") || trimmed.starts_with("socks5://") {
        "(SOCKS5)".to_string()
    } else if trimmed.starts_with("wireguard://") || trimmed.starts_with("wg://") {
        "(WIREGUARD)".to_string()
    } else if trimmed.starts_with("hy2://") || trimmed.starts_with("hysteria2://") {
        "(HYSTERIA2)".to_string()
    } else if trimmed.starts_with("ssr://") {
        "(SHADOWSOCKSR)".to_string()
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
    pub protocol: String,
    pub is_new: bool,
    pub path: String,
    pub public_key: String,
    pub short_id: String,
}

impl NodeEditState {
    pub fn new_for_setup(id: &str, protocol: &str, name: &str, config: &str) -> Self {
        let mut state = Self::from_profile(id, name, config);
        state.is_new = true;
        if !protocol.is_empty() {
            state.protocol = protocol.to_lowercase();
        }
        state
    }

    pub fn from_profile(id: &str, name: &str, raw_uri: &str) -> Self {
        let trimmed = raw_uri.trim();

        if trimmed.starts_with("vmess://") {
            let b64 = trimmed.trim_start_matches("vmess://").trim();
            if let Ok(bytes) = base64::prelude::BASE64_STANDARD.decode(b64) {
                if let Ok(val) = serde_json::from_slice::<Value>(&bytes) {
                    let remarks = if !name.is_empty() {
                        name.to_string()
                    } else {
                        val.get("ps")
                            .and_then(|v| v.as_str())
                            .unwrap_or("New VMess Node")
                            .to_string()
                    };
                    return Self {
                        id: id.to_string(),
                        remarks,
                        address: val.get("add").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        port: val
                            .get("port")
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "443".into()),
                        encrypt_method: "auto".into(),
                        flow: "none".into(),
                        user_id: val.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        transport_protocol: val
                            .get("net")
                            .and_then(|v| v.as_str())
                            .unwrap_or("ws")
                            .to_string(),
                        header_type: val
                            .get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("none")
                            .to_string(),
                        tls_type: val
                            .get("tls")
                            .and_then(|v| v.as_str())
                            .unwrap_or("tls")
                            .to_string(),
                        sni: val.get("host").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        fingerprint: "chrome".into(),
                        alpn: "none".into(),
                        protocol: "vmess".into(),
                        is_new: false,
                        path: val.get("path").and_then(|v| v.as_str()).unwrap_or("/").to_string(),
                        public_key: "".into(),
                        short_id: "".into(),
                    };
                }
            }
        }

        if trimmed.starts_with("ss://") {
            if let Some((content, tag)) = trimmed.trim_start_matches("ss://").split_once('#') {
                let remarks = if !name.is_empty() {
                    name.to_string()
                } else {
                    percent_encoding::percent_decode_str(tag)
                        .decode_utf8_lossy()
                        .to_string()
                };
                if let Some((user_info, host_port)) = content.split_once('@') {
                    if let Some((h, p)) = host_port.split_once(':') {
                        let (method, pass) = if let Ok(decoded) =
                            base64::prelude::BASE64_STANDARD.decode(user_info)
                        {
                            let s = String::from_utf8_lossy(&decoded).to_string();
                            s.split_once(':')
                                .map(|(m, p)| (m.to_string(), p.to_string()))
                                .unwrap_or(("chacha20-poly1305".into(), user_info.to_string()))
                        } else {
                            user_info
                                .split_once(':')
                                .map(|(m, p)| (m.to_string(), p.to_string()))
                                .unwrap_or(("chacha20-poly1305".into(), user_info.to_string()))
                        };
                        return Self {
                            id: id.to_string(),
                            remarks,
                            address: h.to_string(),
                            port: p.to_string(),
                            encrypt_method: method,
                            flow: "none".into(),
                            user_id: pass,
                            transport_protocol: "tcp".into(),
                            header_type: "none".into(),
                            tls_type: "none".into(),
                            sni: "".into(),
                            fingerprint: "none".into(),
                            alpn: "none".into(),
                            protocol: "shadowsocks".into(),
                            is_new: false,
                            path: "".into(),
                            public_key: "".into(),
                            short_id: "".into(),
                        };
                    }
                }
            }
        }

        let detected_proto = if trimmed.starts_with("vless://") {
            "vless".to_string()
        } else if trimmed.starts_with("vmess://") {
            "vmess".to_string()
        } else if trimmed.starts_with("trojan://") {
            "trojan".to_string()
        } else if trimmed.starts_with("socks://") {
            "socks".to_string()
        } else if trimmed.starts_with("ss://") {
            "shadowsocks".to_string()
        } else if trimmed.starts_with("ssr://") {
            "shadowsocksr".to_string()
        } else if trimmed.starts_with("hy2://") {
            "hysteria2".to_string()
        } else if trimmed.starts_with('{') {
            if trimmed.contains("\"wireguard\"") {
                "wireguard".to_string()
            } else {
                "raw".to_string()
            }
        } else {
            "vless".to_string()
        };

        if let Ok(url) = Url::parse(trimmed) {
            let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
            let remarks = if !name.is_empty() {
                name.to_string()
            } else {
                extract_node_name(trimmed)
            };
            let proto = if !detected_proto.is_empty() && detected_proto != "vless" {
                detected_proto
            } else {
                url.scheme().to_lowercase()
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
                protocol: proto,
                is_new: false,
                path: query.get("path").cloned().unwrap_or_default(),
                public_key: query.get("pbk").cloned().unwrap_or_default(),
                short_id: query.get("sid").cloned().unwrap_or_default(),
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
            protocol: detected_proto,
            is_new: false,
            path: "".into(),
            public_key: "".into(),
            short_id: "".into(),
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
        if self.tls_type == "reality" {
            if !self.public_key.is_empty() {
                u.push_str(&format!("&pbk={}", self.public_key.trim()));
            }
            if !self.short_id.is_empty() {
                u.push_str(&format!("&sid={}", self.short_id.trim()));
            }
        }
        if !self.path.is_empty() {
            let p = self.path.trim();
            let enc = percent_encoding::percent_encode(p.as_bytes(), percent_encoding::NON_ALPHANUMERIC);
            u.push_str(&format!("&path={}", enc));
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

    pub fn to_config(&self) -> String {
        match self.protocol.to_lowercase().as_str() {
            "vmess" => {
                let vmess_json = serde_json::json!({
                    "v": "2",
                    "ps": self.remarks.trim(),
                    "add": self.address.trim(),
                    "port": self.port.trim().parse::<u16>().unwrap_or(443),
                    "id": self.user_id.trim(),
                    "aid": 0,
                    "net": if self.transport_protocol.is_empty() { "ws" } else { self.transport_protocol.trim() },
                    "type": if self.header_type.is_empty() { "none" } else { self.header_type.trim() },
                    "host": if self.sni.is_empty() { self.address.trim() } else { self.sni.trim() },
                    "path": if self.path.is_empty() { "/" } else { self.path.trim() },
                    "tls": if self.tls_type.is_empty() { "tls" } else { self.tls_type.trim() }
                });
                let encoded = base64::prelude::BASE64_STANDARD.encode(vmess_json.to_string().as_bytes());
                format!("vmess://{}", encoded)
            }
            "trojan" => {
                let mut u = format!(
                    "trojan://{}@{}:{}?security={}",
                    self.user_id.trim(),
                    self.address.trim(),
                    self.port.trim(),
                    if self.tls_type.is_empty() { "tls" } else { self.tls_type.trim() }
                );
                if !self.sni.is_empty() {
                    u.push_str(&format!("&sni={}", self.sni.trim()));
                }
                if !self.transport_protocol.is_empty() && self.transport_protocol != "tcp" {
                    u.push_str(&format!("&type={}", self.transport_protocol.trim()));
                }
                if !self.path.is_empty() {
                    u.push_str(&format!("&path={}", self.path.trim()));
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
            "socks" => {
                let mut u = if !self.user_id.is_empty() {
                    format!("socks://{}@{}:{}", self.user_id.trim(), self.address.trim(), self.port.trim())
                } else {
                    format!("socks://{}:{}", self.address.trim(), self.port.trim())
                };
                if !self.remarks.is_empty() {
                    let encoded = percent_encoding::percent_encode(
                        self.remarks.as_bytes(),
                        percent_encoding::NON_ALPHANUMERIC,
                    );
                    u.push_str(&format!("#{}", encoded));
                }
                u
            }
            "hysteria2" | "hy2" => {
                let mut u = format!(
                    "hy2://{}@{}:{}?",
                    self.user_id.trim(),
                    self.address.trim(),
                    self.port.trim()
                );
                if !self.sni.is_empty() {
                    u.push_str(&format!("sni={}&", self.sni.trim()));
                }
                if self.tls_type == "none" {
                    u.push_str("insecure=1&");
                }
                let clean = u.trim_end_matches('&').trim_end_matches('?');
                let mut res = clean.to_string();
                if !self.remarks.is_empty() {
                    let encoded = percent_encoding::percent_encode(
                        self.remarks.as_bytes(),
                        percent_encoding::NON_ALPHANUMERIC,
                    );
                    res.push_str(&format!("#{}", encoded));
                }
                res
            }
            "shadowsocks" | "ss" => {
                let method = if self.encrypt_method.is_empty() { "chacha20-poly1305" } else { self.encrypt_method.trim() };
                let auth = format!("{}:{}", method, self.user_id.trim());
                let encoded = base64::prelude::BASE64_STANDARD.encode(auth.as_bytes());
                let mut u = format!("ss://{}@{}:{}", encoded, self.address.trim(), self.port.trim());
                if !self.remarks.is_empty() {
                    let encoded_rem = percent_encoding::percent_encode(
                        self.remarks.as_bytes(),
                        percent_encoding::NON_ALPHANUMERIC,
                    );
                    u.push_str(&format!("#{}", encoded_rem));
                }
                u
            }
            _ => self.to_vless_uri(),
        }
    }
}

/// Prompts the user with a native file picker dialog to select a config file,
/// then reads its contents. Returns Some((file_name, content)) if selected.
pub async fn pick_and_read_config_file() -> Option<(String, String)> {
    let path = pick_config_file_path().await?;
    let path_obj = Path::new(&path);
    let file_name = path_obj
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("imported_config")
        .to_string();

    match tokio::fs::read_to_string(&path).await {
        Ok(content) => Some((file_name, content)),
        Err(_) => None,
    }
}

/// Launches a native file picker dialog (zenity or kdialog) on Linux.
pub async fn pick_config_file_path() -> Option<String> {
    tokio::task::spawn_blocking(|| {
        if let Ok(output) = std::process::Command::new("zenity")
            .args([
                "--file-selection",
                "--title=Select Config File to Import",
                "--file-filter=Config files (*.json *.txt *.yaml *.conf *.v2ray *.xray) | *.json *.txt *.yaml *.conf *.v2ray *.xray",
                "--file-filter=All files | *",
            ])
            .output()
        {
            if output.status.success() {
                let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path_str.is_empty() {
                    return Some(path_str);
                }
            }
        }

        if let Ok(output) = std::process::Command::new("kdialog")
            .args([
                "--getopenfilename",
                ".",
                "*.json *.txt *.yaml *.conf *.v2ray *.xray|Config files\n*|All files",
                "--title",
                "Select Config File to Import",
            ])
            .output()
        {
            if output.status.success() {
                let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path_str.is_empty() {
                    return Some(path_str);
                }
            }
        }

        None
    })
    .await
    .unwrap_or(None)
}

/// Parses config file content into a list of (node_name, config_string).
/// Supports single/multi-line protocol links, base64 subscriptions, raw JSON configs, and JSON arrays.
pub fn parse_imported_configs(content: &str, file_name: Option<&str>) -> Vec<(String, String)> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let default_name = file_name
        .map(|f| {
            Path::new(f)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(f)
        })
        .unwrap_or("Imported Config");

    let mut results = Vec::new();

    // 1. Try decoding as pure Base64 (subscription format)
    let text_to_parse = if !trimmed.starts_with('{') && !trimmed.starts_with('[') && !trimmed.contains("://") {
        if let Ok(decoded_bytes) = BASE64_STANDARD.decode(trimmed).or_else(|_| BASE64_URL_SAFE.decode(trimmed)) {
            if let Ok(decoded_str) = String::from_utf8(decoded_bytes) {
                decoded_str
            } else {
                trimmed.to_string()
            }
        } else {
            trimmed.to_string()
        }
    } else {
        trimmed.to_string()
    };

    let trimmed_text = text_to_parse.trim();

    // 2. Check if it's a JSON array of configs or profiles: e.g. [{"name": "...", "config": "..."}, ...] or outbounds list
    if trimmed_text.starts_with('[') {
        if let Ok(Value::Array(items)) = serde_json::from_str::<Value>(trimmed_text) {
            for (idx, item) in items.iter().enumerate() {
                if let Some(cfg_str) = item.get("config").and_then(|c| c.as_str()) {
                    let name = item.get("name").and_then(|n| n.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| extract_node_name(cfg_str));
                    results.push((name, cfg_str.to_string()));
                } else if item.is_object() {
                    let serialized = serde_json::to_string_pretty(item).unwrap_or_default();
                    let name = item.get("tag").and_then(|t| t.as_str())
                        .or_else(|| item.get("remarks").and_then(|r| r.as_str()))
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| format!("{} #{}", default_name, idx + 1));
                    results.push((name, serialized));
                }
            }
            if !results.is_empty() {
                return results;
            }
        }
    }

    // 3. Check if it's a JSON object: full Xray config or single outbound
    if trimmed_text.starts_with('{') {
        if let Ok(val) = serde_json::from_str::<Value>(trimmed_text) {
            let name = val.get("tag").and_then(|t| t.as_str())
                .or_else(|| val.get("remarks").and_then(|r| r.as_str()))
                .or_else(|| {
                    val.get("outbounds")
                        .and_then(|o| o.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|first| first.get("tag"))
                        .and_then(|t| t.as_str())
                })
                .map(|s| s.to_string())
                .unwrap_or_else(|| default_name.to_string());
            results.push((name, trimmed_text.to_string()));
            return results;
        }
    }

    // 4. Split by lines to find URI protocol links
    let known_prefixes = [
        "vless://", "vmess://", "trojan://", "ss://", "ssr://", "hy2://",
        "hysteria2://", "hysteria://", "wireguard://", "wg://", "socks://",
        "socks5://", "http://", "https://", "tuic://", "vnet://",
    ];

    for line in trimmed_text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') || l.starts_with("//") {
            continue;
        }
        let is_known = known_prefixes.iter().any(|prefix| l.starts_with(prefix));
        if is_known {
            let name = extract_node_name(l);
            results.push((name, l.to_string()));
        } else if l.starts_with('{') {
            if let Ok(val) = serde_json::from_str::<Value>(l) {
                let name = val.get("tag").and_then(|t| t.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("{} #{}", default_name, results.len() + 1));
                results.push((name, l.to_string()));
            }
        }
    }

    // 5. Fallback: if no line matched known prefix, but the content has something non-empty
    if results.is_empty() && !trimmed_text.is_empty() {
        let name = extract_node_name(trimmed_text);
        let final_name = if name == "Custom Node" && !default_name.is_empty() {
            default_name.to_string()
        } else {
            name
        };
        results.push((final_name, trimmed_text.to_string()));
    }

    results
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

    #[test]
    fn test_whitelist_domain_and_ip_parsing() {
        let input = "https://MyWork.com/login; \"*.internal.net\"\t'192.168.1.1', 10.0.0.0/8; geoip:private\n`EXAMPLE.ORG`";
        let (domains, ips) = parse_whitelist_domains_and_ips(input);
        assert!(domains.contains(&"domain:mywork.com".to_string()));
        assert!(domains.contains(&"domain:internal.net".to_string()));
        assert!(domains.contains(&"domain:example.org".to_string()));
        assert!(ips.contains(&"192.168.1.1".to_string()));
        assert!(ips.contains(&"10.0.0.0/8".to_string()));
        assert!(ips.contains(&"geoip:private".to_string()));

        let proc_input = "\"curl\"; '/usr/bin/firefox'\t`discord.exe`\nslack";
        let procs = parse_whitelist_processes(proc_input);
        assert_eq!(procs, vec!["curl", "firefox", "discord.exe", "slack"]);
    }

    #[test]
    fn test_whitelist_routing_rules_proxy_mode() {
        let uri = "vless://b831381d-6324-4d53-ad4f-8cda48b30811@example.com:443?encryption=none&security=tls&sni=example.com&type=tcp#TestNode";
        let config_str = parse_and_generate_xray_config_with_whitelist(
            uri,
            10808,
            10809,
            false,
            "",
            "https://bypass.example.org, 10.0.0.1",
        )
        .expect("Should generate config");
        let val: Value = serde_json::from_str(&config_str).unwrap();

        assert_eq!(val["routing"]["domainStrategy"], "IPIfNonMatch");
        let rules = val["routing"]["rules"].as_array().unwrap();
        assert!(rules.iter().any(|r| {
            r["outboundTag"] == "direct"
                && r["domain"].as_array().map_or(false, |d| {
                    d.iter().any(|item| item == "domain:bypass.example.org")
                })
        }));
        assert!(rules.iter().any(|r| {
            r["outboundTag"] == "direct"
                && r["ip"].as_array().map_or(false, |i| {
                    i.iter().any(|item| item == "10.0.0.1")
                })
        }));
    }

    #[test]
    fn test_whitelist_processes_parsing() {
        let input = "curl, /usr/bin/firefox, C:\\Program Files\\App\\discord.exe\nspotify";
        let procs = parse_whitelist_processes(input);
        assert_eq!(procs, vec!["curl", "firefox", "discord.exe", "spotify"]);
    }

    #[test]
    fn test_whitelist_routing_rules_tun_mode() {
        let uri = "vless://b831381d-6324-4d53-ad4f-8cda48b30811@example.com:443?encryption=none&security=tls&sni=example.com&type=tcp#TunTestNode";
        let config_str = parse_and_generate_xray_config_with_whitelist(
            uri,
            10808,
            10809,
            true,
            "curl, chrome.exe",
            "https://internal.site, 192.168.0.0/16",
        )
        .expect("Should generate TUN config");
        let val: Value = serde_json::from_str(&config_str).unwrap();

        assert!(val["inbounds"].as_array().unwrap().iter().any(|i| i["protocol"] == "tun"));
        let rules = val["routing"]["rules"].as_array().unwrap();

        // Rule 0 is DNS routing to dns tag
        assert_eq!(rules[0]["port"], 53);
        assert_eq!(rules[0]["outboundTag"], "netray-dns");

        // Subsequent rules include direct whitelist rules
        assert!(rules.iter().any(|r| {
            r["outboundTag"] == "direct"
                && r["process"].as_array().map_or(false, |p| {
                    p.iter().any(|item| item == "curl") && p.iter().any(|item| item == "chrome.exe")
                })
        }));
        assert!(rules.iter().any(|r| {
            r["outboundTag"] == "direct"
                && r["domain"].as_array().map_or(false, |d| {
                    d.iter().any(|item| item == "domain:internal.site")
                })
        }));

        // Final rule routes tun-in traffic to primary proxy tag
        let last_rule = rules.last().unwrap();
        assert_eq!(last_rule["inboundTag"][0], "tun-in");
        assert_eq!(last_rule["outboundTag"], "proxy");
    }

    #[tokio::test]
    async fn test_whitelist_config_validation_with_xray() {
        let uri = "vless://b831381d-6324-4d53-ad4f-8cda48b30811@server.example.com:443?encryption=none&security=tls&sni=example.com&type=tcp#SampleNode";
        let config = parse_and_generate_xray_config_with_whitelist(
            uri,
            10808,
            10809,
            false,
            "curl, firefox, discord",
            "https://mywork.com, *.internal.net, 10.0.0.0/8, 192.168.1.1",
        )
        .expect("Should generate whitelist config");

        if let Ok(bin) = find_xray_binary() {
            let res = test_xray_config(&bin, &config).await;
            assert!(
                res.is_ok(),
                "Xray validation failed for whitelist routing config: {:?}",
                res
            );
        }
    }

    #[test]
    fn test_parse_ifconfig_output() {
        assert_eq!(
            parse_ifconfig_output("13.214.47.207\n"),
            Ok("13.214.47.207".to_string())
        );
        assert_eq!(
            parse_ifconfig_output("2606:4700:4700::1111\n"),
            Ok("2606:4700:4700::1111".to_string())
        );
        assert!(parse_ifconfig_output("").is_err());
        assert!(parse_ifconfig_output("   \n\t").is_err());
        assert!(parse_ifconfig_output("<html>502 Bad Gateway</html>").is_err());
    }

    #[tokio::test]
    async fn test_wireguard_generation() {
        let uri = "wireguard://AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=@127.0.0.1:51820/?public_key=BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB=&address=10.0.0.2/32#Test-WG";
        let config_str = parse_and_generate_xray_config(uri, 10808, 10809, false)
            .expect("Should generate WireGuard config");
        let val: Value = serde_json::from_str(&config_str).unwrap();
        assert_eq!(val["outbounds"][0]["protocol"], "wireguard");
        assert_eq!(
            val["outbounds"][0]["settings"]["secretKey"],
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
        );
        assert_eq!(
            val["outbounds"][0]["settings"]["peers"][0]["publicKey"],
            "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB="
        );
        assert_eq!(extract_protocol_tag(uri), "(WIREGUARD)");
    }

    #[tokio::test]
    async fn test_shadowsocks2022_generation() {
        let uri = "ss://2022-blake3-aes-128-gcm:7sN6q9U1jB3V0vYw8Z1A2Q==@127.0.0.1:8388#Test-SS2022";
        let config_str = parse_and_generate_xray_config(uri, 10808, 10809, false)
            .expect("Should generate SS2022 config");
        let val: Value = serde_json::from_str(&config_str).unwrap();
        assert_eq!(val["outbounds"][0]["protocol"], "shadowsocks");
        assert_eq!(
            val["outbounds"][0]["settings"]["servers"][0]["method"],
            "2022-blake3-aes-128-gcm"
        );
        assert_eq!(extract_protocol_tag(uri), "(SHADOWSOCKS-2022)");
    }

    #[tokio::test]
    async fn test_socks_generation() {
        let uri = "socks://user:pass@127.0.0.1:1080#Test-Socks";
        let config_str = parse_and_generate_xray_config(uri, 10808, 10809, false)
            .expect("Should generate Socks config");
        let val: Value = serde_json::from_str(&config_str).unwrap();
        assert_eq!(val["outbounds"][0]["protocol"], "socks");
        assert_eq!(
            val["outbounds"][0]["settings"]["servers"][0]["address"],
            "127.0.0.1"
        );
        assert_eq!(
            val["outbounds"][0]["settings"]["servers"][0]["port"],
            1080
        );
        assert_eq!(extract_protocol_tag(uri), "(SOCKS5)");
    }

    #[tokio::test]
    async fn test_hysteria2_generation() {
        let uri = "hy2://password123@127.0.0.1:443?insecure=1&sni=example.com#Test-Hy2";
        let config_str = parse_and_generate_xray_config(uri, 10808, 10809, false)
            .expect("Should generate Hysteria2 config");
        let val: Value = serde_json::from_str(&config_str).unwrap();
        assert_eq!(val["outbounds"][0]["protocol"], "hysteria2");
        assert_eq!(
            val["outbounds"][0]["settings"]["servers"][0]["password"],
            "password123"
        );
        assert_eq!(
            val["outbounds"][0]["streamSettings"]["tlsSettings"]["serverName"],
            "example.com"
        );
        assert_eq!(extract_protocol_tag(uri), "(HYSTERIA2)");
    }

    #[tokio::test]
    async fn test_shadowsocksr_generation() {
        let uri = "ssr://MTI3LjAuMC4xOjgzODg6b3JpZ2luOmFlcy0yNTYtY2ZiOnBsYWluOmNHRnpjM2R2Y21ReE1qTT0vP3JlbWFya3M9VG1WM0xWTlRVUzFPYkdRPQ==";
        let config_str = parse_and_generate_xray_config(uri, 10808, 10809, false)
            .expect("Should generate SSR config");
        let val: Value = serde_json::from_str(&config_str).unwrap();
        assert_eq!(val["outbounds"][0]["protocol"], "shadowsocks");
        assert_eq!(
            val["outbounds"][0]["settings"]["servers"][0]["method"],
            "aes-256-cfb"
        );
        assert_eq!(
            val["outbounds"][0]["settings"]["servers"][0]["password"],
            "password123"
        );
        assert_eq!(extract_protocol_tag(uri), "(SHADOWSOCKSR)");
    }

    #[tokio::test]
    async fn test_all_eleven_templates_produce_valid_outbounds() {
        for proto in crate::protocols::ProfileType::ALL {
            let (remarks, config) = proto.generate_template();
            assert!(!remarks.is_empty());
            assert!(!config.is_empty());

            match proto.badge() {
                crate::protocols::BadgeKind::Xray | crate::protocols::BadgeKind::New => {
                    let result = parse_and_generate_xray_config(&config, 10808, 10809, false);
                    assert!(
                        result.is_ok(),
                        "Failed to generate config for {:?}: {:?}",
                        proto,
                        result.err()
                    );
                    let parsed: Value = serde_json::from_str(&result.unwrap()).unwrap();
                    assert!(parsed.get("outbounds").is_some());
                }
                crate::protocols::BadgeKind::Native => {
                    assert!(config.starts_with("ssh://") || config.starts_with("dnstt+ssh://"));
                }
            }
        }
    }

    #[test]
    fn test_generate_share_link_preserves_existing_uris() {
        let uri = "vless://uuid@1.1.1.1:443?security=tls#ExistingNode";
        let link = generate_share_link(uri, "ExistingNode");
        assert_eq!(link, uri);

        let uri_no_frag = "vless://uuid@1.1.1.1:443?security=tls";
        let link_added = generate_share_link(uri_no_frag, "AddedRemark");
        assert!(link_added.ends_with("#AddedRemark"));
    }

    #[test]
    fn test_generate_share_link_from_raw_json_vless() {
        let json_config = r#"{
            "outbounds": [{
                "protocol": "vless",
                "settings": {
                    "vnext": [{
                        "address": "fast.node.com",
                        "port": 443,
                        "users": [{
                            "id": "11111111-2222-3333-4444-555555555555",
                            "flow": "xtls-rprx-vision"
                        }]
                    }]
                },
                "streamSettings": {
                    "network": "tcp",
                    "security": "reality",
                    "realitySettings": {
                        "serverName": "reality.sni.com",
                        "publicKey": "testpbk",
                        "shortId": "testsid"
                    }
                }
            }]
        }"#;

        let link = generate_share_link(json_config, "GeneratedVLess");
        assert!(link.starts_with("vless://11111111-2222-3333-4444-555555555555@fast.node.com:443?"));
        assert!(link.contains("security=reality"));
        assert!(link.contains("sni=reality.sni.com"));
        assert!(link.contains("pbk=testpbk"));
        assert!(link.contains("sid=testsid"));
        assert!(link.contains("flow=xtls-rprx-vision"));
        assert!(link.ends_with("#GeneratedVLess"));
    }

    #[test]
    fn test_generate_share_link_from_raw_json_vmess() {
        let json_config = r#"{
            "outbounds": [{
                "protocol": "vmess",
                "settings": {
                    "vnext": [{
                        "address": "vmess.server.net",
                        "port": 8443,
                        "users": [{
                            "id": "aaaa-bbbb-cccc-dddd",
                            "alterId": 0
                        }]
                    }]
                },
                "streamSettings": {
                    "network": "ws",
                    "security": "tls",
                    "wsSettings": {
                        "path": "/websocket"
                    }
                }
            }]
        }"#;

        let link = generate_share_link(json_config, "GeneratedVMess");
        assert!(link.starts_with("vmess://"));
        let b64 = link.trim_start_matches("vmess://");
        let decoded = BASE64_STANDARD.decode(b64).expect("Valid base64");
        let val: Value = serde_json::from_slice(&decoded).expect("Valid json");
        assert_eq!(val["ps"], "GeneratedVMess");
        assert_eq!(val["add"], "vmess.server.net");
        assert_eq!(val["port"], 8443);
        assert_eq!(val["id"], "aaaa-bbbb-cccc-dddd");
        assert_eq!(val["net"], "ws");
        assert_eq!(val["path"], "/websocket");
    }

    #[test]
    fn test_generate_share_link_from_raw_json_trojan_and_shadowsocks() {
        let trojan_json = r#"{
            "outbounds": [{
                "protocol": "trojan",
                "settings": {
                    "servers": [{
                        "address": "trojan.host.com",
                        "port": 443,
                        "password": "secretpassword"
                    }]
                },
                "streamSettings": {
                    "tlsSettings": {
                        "serverName": "trojan.host.com"
                    }
                }
            }]
        }"#;
        let trojan_link = generate_share_link(trojan_json, "TrojanNode");
        assert!(trojan_link.starts_with("trojan://secretpassword@trojan.host.com:443?"));
        assert!(trojan_link.contains("sni=trojan.host.com"));
        assert!(trojan_link.ends_with("#TrojanNode"));

        let ss_json = r#"{
            "outbounds": [{
                "protocol": "shadowsocks",
                "settings": {
                    "servers": [{
                        "address": "ss.server.org",
                        "port": 9000,
                        "method": "aes-128-gcm",
                        "password": "sspassword"
                    }]
                }
            }]
        }"#;
        let ss_link = generate_share_link(ss_json, "SSNode");
        assert!(ss_link.starts_with("ss://"));
        assert!(ss_link.contains("@ss.server.org:9000"));
        assert!(ss_link.ends_with("#SSNode"));
    }

    #[test]
    fn test_xray_config_access_logging_enabled() {
        let uri = "vless://00000000-0000-0000-0000-000000000000@example.com:443?type=tcp&encryption=none#TestNode";
        let config_str = parse_and_generate_xray_config_with_whitelist(uri, 10808, 10809, false, "", "").unwrap();
        let val: serde_json::Value = serde_json::from_str(&config_str).unwrap();
        assert_eq!(val["log"]["loglevel"], "info");
        assert_eq!(val["log"]["access"], "");
    }

    #[test]
    fn test_parse_imported_configs_multi_format() {
        // 1. Single URI
        let single = "vless://00000000-0000-0000-0000-000000000000@1.1.1.1:443?type=ws#FastVless";
        let res = parse_imported_configs(single, Some("test.txt"));
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].0, "FastVless");
        assert_eq!(res[0].1, single);

        // 2. Multiline list with comments
        let multi = "# Subscription Header\nvless://uuid@1.1.1.1:443#Node1\n// Comment\ntrojan://pass@2.2.2.2:443#Node2\n";
        let res = parse_imported_configs(multi, Some("list.conf"));
        assert_eq!(res.len(), 2);
        assert_eq!(res[0].0, "Node1");
        assert_eq!(res[1].0, "Node2");

        // 3. Base64 subscription
        let sub_plain = "vless://uuid@3.3.3.3:443#SubNode1\ntrojan://pass@4.4.4.4:443#SubNode2";
        let sub_b64 = BASE64_STANDARD.encode(sub_plain);
        let res = parse_imported_configs(&sub_b64, Some("sub.txt"));
        assert_eq!(res.len(), 2);
        assert_eq!(res[0].0, "SubNode1");
        assert_eq!(res[1].0, "SubNode2");

        // 4. Raw JSON object
        let raw_json = r#"{"outbounds":[{"protocol":"vless","tag":"MyJsonNode"}]}"#;
        let res = parse_imported_configs(raw_json, Some("config.json"));
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].0, "MyJsonNode");

        // 5. JSON Array of profiles
        let arr_json = r#"[{"name":"ProfileA","config":"vless://..."},{"name":"ProfileB","config":"trojan://..."}]"#;
        let res = parse_imported_configs(arr_json, Some("backup.json"));
        assert_eq!(res.len(), 2);
        assert_eq!(res[0].0, "ProfileA");
        assert_eq!(res[1].0, "ProfileB");
    }
}

