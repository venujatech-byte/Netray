use serde_json::{json, Value};
use std::{path::Path, process::Stdio, time::Instant};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    process::Command,
    time::{timeout, Duration},
};

// A TCP handshake can finish locally under TUN; wait for a remote HTTP response.
pub async fn measure(binary: &Path, mut config: Value) -> Result<u128, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let outbounds = config["outbounds"]
        .as_array_mut()
        .ok_or("Missing outbounds")?;
    let proxy = outbounds
        .iter_mut()
        .find(|outbound| {
            !matches!(
                outbound["protocol"].as_str(),
                Some("freedom" | "blackhole" | "dns") | None
            )
        })
        .ok_or("No proxy outbound to test")?;
    if proxy["tag"].as_str().is_none() {
        proxy["tag"] = json!("latency-proxy");
    }
    let tag = proxy["tag"].clone();
    config["inbounds"] =
        json!([{"tag": "latency", "listen": "127.0.0.1", "port": port, "protocol": "http"}]);
    config["routing"] =
        json!({"rules": [{"type": "field", "inboundTag": ["latency"], "outboundTag": tag}]});
    config["log"] = json!({"loglevel": "none"});
    for key in [
        "api",
        "metrics",
        "stats",
        "observatory",
        "burstObservatory",
        "reverse",
    ] {
        config.as_object_mut().ok_or("Invalid config")?.remove(key);
    }
    let mut command = Command::new(binary);
    command
        .args(["run", "-c", "stdin:", "-format", "json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(target_os = "windows")]
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW
                                        // Release the reservation immediately before Xray binds its private probe port.
    drop(listener);
    let mut child = command
        .spawn()
        .map_err(|e| format!("Cannot start latency test: {e}"))?;
    let result = timeout(Duration::from_secs(15), async {
        let mut stdin = child.stdin.take().ok_or("Cannot send probe config")?;
        stdin
            .write_all(config.to_string().as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        drop(stdin);
        let stream = loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                return Err(format!(
                    "Latency test core exited ({status}); check the profile."
                ));
            }
            match TcpStream::connect(("127.0.0.1", port)).await {
                Ok(stream) => break stream,
                Err(_) => tokio::time::sleep(Duration::from_millis(25)).await,
            }
        };
        response_latency(stream).await
    })
    .await
    .unwrap_or_else(|_| Err("Proxy response timed out after 15 seconds.".into()));
    // Reap the probe even after an error; cancellation is covered by kill_on_drop.
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill().await;
    }
    result
}

pub async fn ping_local_http_proxy(port: u16) -> Result<u128, String> {
    let stream = TcpStream::connect(("127.0.0.1", port))
        .await
        .map_err(|error| format!("Local Xray HTTP proxy is unavailable: {error}"))?;
    timeout(Duration::from_secs(10), response_latency(stream))
        .await
        .map_err(|_| "Remote proxy check timed out after 10 seconds.".to_owned())?
}

pub async fn wait_for_http_proxy(port: u16) -> Result<u128, String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    let mut last_error = String::new();
    while tokio::time::Instant::now() < deadline {
        match TcpStream::connect(("127.0.0.1", port)).await {
            Ok(stream) => {
                match timeout(Duration::from_secs(4), response_latency(stream)).await {
                    Ok(Ok(latency)) => return Ok(latency),
                    Ok(Err(err)) => {
                        last_error = err;
                        tokio::time::sleep(Duration::from_millis(350)).await;
                    }
                    Err(_) => {
                        last_error = "Remote HTTP check timed out.".to_string();
                        tokio::time::sleep(Duration::from_millis(350)).await;
                    }
                }
            }
            Err(error) => {
                last_error = format!("Xray HTTP proxy did not start on port {port}: {error}");
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
        }
    }
    Err(if last_error.is_empty() {
        "Xray started, but the remote HTTP check timed out.".to_string()
    } else {
        last_error
    })
}

async fn response_latency(mut stream: TcpStream) -> Result<u128, String> {
    let started = Instant::now();
    stream.write_all(b"GET http://www.gstatic.com/generate_204 HTTP/1.1\r\nHost: www.gstatic.com\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n")
        .await.map_err(|e| e.to_string())?;
    let mut header = Vec::with_capacity(128);
    while header.len() < 8192 {
        let byte = stream
            .read_u8()
            .await
            .map_err(|e| format!("No complete proxy response: {e}"))?;
        header.push(byte);
        if header.ends_with(b"\r\n\r\n") {
            let status = std::str::from_utf8(&header)
                .ok()
                .and_then(|s| s.lines().next())
                .and_then(|line| line.split_whitespace().nth(1));
            return match status {
                Some("204") => Ok(started.elapsed().as_millis()),
                Some("503") => Err("Remote proxy server is unreachable or refused connection (HTTP 503)".into()),
                Some("502") => Err("Remote proxy server returned Bad Gateway (HTTP 502)".into()),
                Some("504") => Err("Remote proxy server gateway timed out (HTTP 504)".into()),
                Some(code) => Err(format!("Latency endpoint returned HTTP {code}")),
                None => Err("Latency endpoint returned invalid HTTP response".into()),
            };
        }
    }
    Err("Latency response headers are too large".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires NETRAY_TEST_XRAY pointing to an installed core"]
    async fn real_core_repeated_latency() {
        let binary = std::env::var("NETRAY_TEST_XRAY").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    header.push(stream.read_u8().await.unwrap());
                }
                assert!(header.starts_with(b"CONNECT "));
                stream
                    .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                    .await
                    .unwrap();
                header.clear();
                while !header.ends_with(b"\r\n\r\n") {
                    header.push(stream.read_u8().await.unwrap());
                }
                assert!(header.starts_with(b"GET "));
                tokio::time::sleep(Duration::from_millis(60)).await;
                stream
                    .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                    .await
                    .unwrap();
            }
        });
        for _ in 0..2 {
            let config = json!({"outbounds": [{"protocol": "http", "settings": {
                "servers": [{"address": "127.0.0.1", "port": port}]
            }}]});
            assert!(measure(Path::new(&binary), config).await.unwrap() >= 50);
        }
        server.await.unwrap();
    }

    #[tokio::test]
    async fn repeated_probes_wait_for_remote_response() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0; 512];
                assert!(stream.read(&mut request).await.unwrap() > 0);
                tokio::time::sleep(Duration::from_millis(60)).await;
                stream
                    .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                    .await
                    .unwrap();
            }
        });
        for _ in 0..2 {
            let stream = TcpStream::connect(address).await.unwrap();
            assert!(response_latency(stream).await.unwrap() >= 50);
        }
        server.await.unwrap();
    }

    #[tokio::test]
    async fn startup_probe_waits_for_proxy_then_checks_remote_response() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 512];
            assert!(stream.read(&mut request).await.unwrap() > 0);
            tokio::time::sleep(Duration::from_millis(60)).await;
            stream
                .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
        });
        assert!(wait_for_http_proxy(port).await.unwrap() >= 50);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn proxy_error_is_not_a_successful_ping() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 512];
            stream.read(&mut request).await.unwrap();
            stream
                .write_all(b"HTTP/1.1 502 Bad Gateway\r\n\r\n")
                .await
                .unwrap();
        });
        assert!(response_latency(TcpStream::connect(address).await.unwrap())
            .await
            .unwrap_err()
            .contains("502"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_ping_local_http_proxy_roundtrip_and_unavailable() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 512];
            let _ = stream.read(&mut request).await.unwrap();
            stream
                .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
        });

        let latency = ping_local_http_proxy(port).await.expect("ping should succeed");
        assert!(latency < 5000);
        server.await.unwrap();

        // Testing an unbound port immediately reports proxy unavailable
        let unbound_port = 59881;
        let err = ping_local_http_proxy(unbound_port).await.unwrap_err();
        assert!(err.contains("Local Xray HTTP proxy is unavailable"));
    }
}
