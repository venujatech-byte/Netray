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
