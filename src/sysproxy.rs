use tokio::process::Command;

/// Enable GNOME and Linux system-wide proxy routing to local Netray ports
pub async fn enable_system_proxy(socks_port: u16, http_port: u16) -> Result<(), String> {
    let socks_port_str = socks_port.to_string();
    let http_port_str = http_port.to_string();

    let settings = [
        ("org.gnome.system.proxy", "mode", "manual"),
        ("org.gnome.system.proxy.socks", "host", "127.0.0.1"),
        ("org.gnome.system.proxy.socks", "port", &socks_port_str),
        ("org.gnome.system.proxy.http", "host", "127.0.0.1"),
        ("org.gnome.system.proxy.http", "port", &http_port_str),
        ("org.gnome.system.proxy.https", "host", "127.0.0.1"),
        ("org.gnome.system.proxy.https", "port", &http_port_str),
    ];

    for (schema, key, val) in settings {
        let _ = Command::new("gsettings")
            .args(["set", schema, key, val])
            .output()
            .await;
    }

    Ok(())
}

/// Disable system proxy and restore direct network connectivity
pub async fn disable_system_proxy() {
    let _ = Command::new("gsettings")
        .args(["set", "org.gnome.system.proxy", "mode", "none"])
        .output()
        .await;
}
