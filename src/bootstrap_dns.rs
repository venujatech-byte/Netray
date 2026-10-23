use serde_json::{json, Value};
use std::{net::IpAddr, time::Duration};
use tokio::net::lookup_host;

pub async fn pin_proxy_address(config: &mut Value) -> Result<(), String> {
    let outbounds = config["outbounds"]
        .as_array()
        .ok_or("Xray config has no outbounds")?;
    let proxy = outbounds
        .iter()
        .find(|outbound| outbound["tag"] == "proxy")
        .or_else(|| {
            outbounds.iter().find(|outbound| {
                !matches!(
                    outbound["protocol"].as_str(),
                    Some("dns" | "freedom" | "blackhole") | None
                )
            })
        })
        .ok_or("Xray config has no proxy outbound")?;
    let endpoint = proxy["settings"]["vnext"]
        .as_array()
        .and_then(|servers| servers.first())
        .or_else(|| {
            proxy["settings"]["servers"]
                .as_array()
                .and_then(|servers| servers.first())
        })
        .ok_or("Proxy outbound has no server address")?;
    let host = endpoint["address"]
        .as_str()
        .filter(|host| !host.is_empty())
        .ok_or("Proxy server address is missing")?
        .to_owned();
    if host.parse::<IpAddr>().is_ok() {
        return Ok(());
    }
    let port = endpoint["port"].as_u64().unwrap_or(443) as u16;
    let addresses =
        tokio::time::timeout(Duration::from_secs(5), lookup_host((host.as_str(), port)))
            .await
            .map_err(|_| format!("System DNS timed out resolving proxy server '{host}'."))?
            .map_err(|error| {
                format!("System DNS could not resolve proxy server '{host}': {error}")
            })?
            .map(|address| address.ip().to_string())
            .collect::<Vec<_>>();
    if addresses.is_empty() {
        return Err(format!(
            "System DNS returned no addresses for proxy server '{host}'."
        ));
    }
    let hosts = config["dns"]
        .as_object_mut()
        .ok_or("TUN DNS configuration is missing")?
        .entry("hosts")
        .or_insert_with(|| json!({}));
    let hosts = hosts
        .as_object_mut()
        .ok_or("Xray DNS hosts must be an object")?;
    hosts.entry(host).or_insert_with(|| json!(addresses));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn maps_proxy_hostname_before_tun_dns_starts() {
        let mut config = json!({"outbounds": [{"tag":"proxy","protocol":"vless","settings":{
            "vnext":[{"address":"localhost","port":443}]
        }}], "dns":{"servers":["https://1.1.1.1/dns-query"]}});
        pin_proxy_address(&mut config).await.unwrap();
        assert!(!config["dns"]["hosts"]["localhost"]
            .as_array()
            .unwrap()
            .is_empty());
        assert_eq!(config["dns"]["servers"][0], "https://1.1.1.1/dns-query");
    }

    #[tokio::test]
    async fn preserves_a_user_override_for_the_proxy_host() {
        let mut config = json!({"outbounds": [{"tag":"proxy","protocol":"vless","settings":{
            "vnext":[{"address":"localhost","port":443}]
        }}], "dns":{"hosts":{"localhost":"192.0.2.10"}}});
        pin_proxy_address(&mut config).await.unwrap();
        assert_eq!(config["dns"]["hosts"]["localhost"], "192.0.2.10");
    }

    #[tokio::test]
    async fn leaves_ip_endpoints_and_existing_dns_hosts_alone() {
        let mut config = json!({"outbounds": [{"tag":"proxy","protocol":"vless","settings":{
            "vnext":[{"address":"203.0.113.1","port":443}]
        }}], "dns":{"hosts":{"existing.test":"192.0.2.1"}}});
        pin_proxy_address(&mut config).await.unwrap();
        assert_eq!(config["dns"]["hosts"]["existing.test"], "192.0.2.1");
    }
}
