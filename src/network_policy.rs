use serde_json::{json, Value};

pub fn apply_ipv6_policy(config: &mut Value, enabled: bool) {
    if enabled { return; }
    // Keep IPv6 captured by TUN so disabled traffic cannot escape over the physical NIC.
    config["dns"]["queryStrategy"] = json!("UseIPv4");
    if config["outbounds"].is_array() {
        config["outbounds"].as_array_mut().unwrap().push(json!({"tag":"netray-ipv6-block", "protocol":"blackhole"}));
    }
    if config["routing"]["rules"].is_null() { config["routing"]["rules"] = json!([]); }
    if let Some(rules) = config["routing"]["rules"].as_array_mut() {
        rules.insert(0, json!({"type":"field", "ip":["::/0"], "outboundTag":"netray-ipv6-block"}));
    }
}

pub fn apply_lan_policy(config: &mut Value, enabled: bool) {
    if !enabled { return; }
    config["outbounds"].as_array_mut().unwrap().push(json!({"tag":"netray-lan-direct", "protocol":"freedom"}));
    if config["routing"]["rules"].is_null() { config["routing"]["rules"] = json!([]); }
    if let Some(rules) = config["routing"]["rules"].as_array_mut() {
        rules.insert(0, json!({"type":"field", "ip":["10.0.0.0/8", "100.64.0.0/10", "127.0.0.0/8", "169.254.0.0/16", "172.16.0.0/12", "192.168.0.0/16", "fc00::/7", "fe80::/10"], "outboundTag":"netray-lan-direct"}));
    }
}

pub fn apply_dns_policy(config: &mut Value, raw: &str, ipv6: bool) -> Result<(), String> {
    if !config.is_object() { return Err("Xray config must be an object.".into()); }
    if !config["outbounds"].is_array() { return Err("Xray outbounds must be an array.".into()); }
    for key in ["dns", "routing"] {
        if config[key].is_null() { config[key] = json!({}); }
        if !config[key].is_object() { return Err(format!("Xray {key} must be an object.")); }
    }
    if config["routing"]["rules"].is_null() { config["routing"]["rules"] = json!([]); }
    if !config["routing"]["rules"].is_array() { return Err("Xray routing rules must be an array.".into()); }
    let mut servers = Vec::new();
    for part in raw.split(|c: char| c == ',' || c.is_whitespace()).filter(|s| !s.is_empty()) {
        let address: std::net::IpAddr = part.parse().map_err(|_| format!("Invalid DNS server IP address: {part}"))?;
        if address.is_unspecified() || address.is_multicast() { return Err(format!("Invalid DNS server address: {part}")); }
        if !ipv6 && address.is_ipv6() { continue; }
        let value = address.to_string();
        if !servers.contains(&value) { servers.push(value); }
    }
    if servers.is_empty() { return Err("Enter at least one usable DNS server IP address.".into()); }
    config["dns"]["servers"] = json!(servers);
    config["dns"]["queryStrategy"] = json!(if ipv6 { "UseIP" } else { "UseIPv4" });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disabled_ipv6_stays_blocked_ahead_of_lan_bypass() {
        let mut config = json!({"outbounds":[],"routing":{"rules":[]}});
        apply_lan_policy(&mut config, true);
        apply_ipv6_policy(&mut config, false);
        assert_eq!(config["routing"]["rules"][0]["ip"][0], "::/0");
        assert_eq!(config["routing"]["rules"][1]["outboundTag"], "netray-lan-direct");
    }
    #[test]
    fn dns_validation_and_ipv6_filtering() {
        let mut config = json!({"outbounds":[]});
        assert!(apply_dns_policy(&mut config, "invalid", true).is_err());
        assert!(apply_dns_policy(&mut config, "2606:4700:4700::1111", false).is_err());
        apply_dns_policy(&mut config, "1.1.1.1, 1.1.1.1, 2606:4700:4700::1111", false).unwrap();
        assert_eq!(config["dns"]["servers"], json!(["1.1.1.1"]));
        assert_eq!(config["dns"]["queryStrategy"], "UseIPv4");
    }
}
