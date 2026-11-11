use iced::futures::StreamExt;
use std::{collections::HashMap, io::Read, time::Duration};
use zbus::{
    zvariant::{OwnedObjectPath, OwnedValue, Value},
    Connection, MatchRule, MessageStream, Proxy,
};

#[derive(Debug)]
pub(super) enum PortalError {
    Unavailable(String),
    Failed(String),
}

pub(super) fn screenshot() -> Result<Vec<u8>, PortalError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| PortalError::Unavailable(e.to_string()))?
        .block_on(async {
            let connection = Connection::session().await.map_err(|e| {
                PortalError::Unavailable(format!("Desktop portal is unavailable: {e}"))
            })?;
            request(&connection).await
        })
}

async fn request(connection: &Connection) -> Result<Vec<u8>, PortalError> {
    let failed =
        |error: zbus::Error| PortalError::Failed(format!("Desktop screenshot failed: {error}"));
    let portal = Proxy::new(
        connection,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Screenshot",
    )
    .await
    .map_err(failed)?;
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.freedesktop.portal.Desktop")
        .map_err(failed)?
        .interface("org.freedesktop.portal.Request")
        .map_err(failed)?
        .member("Response")
        .map_err(failed)?
        .build();
    // Subscribe before requesting capture; the response can arrive before the method reply.
    let mut responses = MessageStream::for_match_rule(rule, connection, Some(8))
        .await
        .map_err(failed)?;
    let mut random = [0u8; 8];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut random))
        .map_err(|e| PortalError::Failed(format!("Could not create a screenshot request: {e}")))?;
    let token = format!("netray_{}", u64::from_ne_bytes(random));
    let options = HashMap::from([
        ("handle_token", Value::from(token.as_str())),
        ("interactive", Value::from(true)),
    ]);
    let handle: OwnedObjectPath =
        portal
            .call("Screenshot", &("", options))
            .await
            .map_err(|error| match &error {
                zbus::Error::MethodError(name, ..)
                    if matches!(
                        name.as_str(),
                        "org.freedesktop.DBus.Error.ServiceUnknown"
                            | "org.freedesktop.DBus.Error.UnknownMethod"
                            | "org.freedesktop.DBus.Error.UnknownInterface"
                            | "org.freedesktop.DBus.Error.UnknownObject"
                    ) =>
                {
                    PortalError::Unavailable(format!(
                        "Desktop screenshot portal is unavailable: {error}"
                    ))
                }
                _ => failed(error),
            })?;
    let result = tokio::time::timeout(Duration::from_secs(90), async {
        while let Some(message) = responses.next().await {
            let message = message.map_err(failed)?;
            if message.header().path().map(|path| path.as_str()) != Some(handle.as_str()) {
                continue;
            }
            let (response, results): (u32, HashMap<String, OwnedValue>) =
                message.body().deserialize().map_err(failed)?;
            return read_response(response, results);
        }
        Err(PortalError::Failed(
            "The desktop screenshot service closed the request.".into(),
        ))
    })
    .await;
    match result {
        Ok(result) => result,
        Err(_) => {
            let _: Result<(), _> = connection
                .call_method(
                    Some("org.freedesktop.portal.Desktop"),
                    handle.as_str(),
                    Some("org.freedesktop.portal.Request"),
                    "Close",
                    &(),
                )
                .await
                .map(|_| ());
            Err(PortalError::Failed(
                "Screenshot request timed out. Try again or upload a saved image.".into(),
            ))
        }
    }
}

fn read_response(
    response: u32,
    mut results: HashMap<String, OwnedValue>,
) -> Result<Vec<u8>, PortalError> {
    match response {
        0 => {}
        1 => return Err(PortalError::Failed("Screenshot cancelled.".into())),
        _ => {
            return Err(PortalError::Failed(
                "The desktop could not capture a screenshot. Try again or upload a saved image."
                    .into(),
            ))
        }
    }
    let uri = results
        .remove("uri")
        .and_then(|value| String::try_from(value).ok())
        .ok_or_else(|| PortalError::Failed("The screenshot service returned no image.".into()))?;
    let path = url::Url::parse(&uri)
        .ok()
        .and_then(|uri| uri.to_file_path().ok())
        .ok_or_else(|| {
            PortalError::Failed("The screenshot service returned an invalid local file URI.".into())
        })?;
    // The portal owns this file; it may be a user-selected or portal-managed image.
    std::fs::read(path)
        .map_err(|e| PortalError::Failed(format!("Could not read the captured screenshot: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader},
        process::{Child, Command, Stdio},
    };

    struct PrivateBus(Child);
    impl Drop for PrivateBus {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    struct TestPortal {
        response: u32,
        uri: String,
    }
    #[zbus::interface(name = "org.freedesktop.portal.Screenshot")]
    impl TestPortal {
        async fn screenshot(
            &self,
            _parent: &str,
            options: HashMap<String, OwnedValue>,
            #[zbus(header)] header: zbus::message::Header<'_>,
            #[zbus(connection)] connection: &Connection,
        ) -> zbus::fdo::Result<OwnedObjectPath> {
            let sender = header.sender().unwrap();
            let token: &str = options.get("handle_token").unwrap().try_into().unwrap();
            let path = format!(
                "/org/freedesktop/portal/desktop/request/{}/{}",
                sender.as_str().trim_start_matches(':').replace('.', "_"),
                token
            );
            let results = HashMap::from([("uri", Value::from(self.uri.as_str()))]);
            // Intentionally send before returning the handle to exercise the signal race.
            connection
                .emit_signal(
                    Some(sender.as_str()),
                    path.as_str(),
                    "org.freedesktop.portal.Request",
                    "Response",
                    &(self.response, results),
                )
                .await
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
            Ok(OwnedObjectPath::try_from(path).unwrap())
        }
    }

    #[tokio::test]
    async fn screenshot_portal_handles_early_responses_cancellation_and_invalid_uris() {
        let path =
            std::env::temp_dir().join(format!("netray-portal-test-{}.png", std::process::id()));
        std::fs::write(&path, b"test image").unwrap();
        let uri = url::Url::from_file_path(&path).unwrap().to_string();
        for (response, uri, success) in [
            (0, uri.clone(), true),
            (1, uri.clone(), false),
            (2, uri.clone(), false),
            (0, "https://example.com/image.png".into(), false),
        ] {
            let mut bus = PrivateBus(
                Command::new("dbus-daemon")
                    .args(["--session", "--nofork", "--print-address=1"])
                    .stdout(Stdio::piped())
                    .spawn()
                    .expect("dbus-daemon is required for the isolated portal test"),
            );
            let mut address = String::new();
            BufReader::new(bus.0.stdout.take().unwrap())
                .read_line(&mut address)
                .unwrap();
            let _service = zbus::connection::Builder::address(address.trim())
                .unwrap()
                .name("org.freedesktop.portal.Desktop")
                .unwrap()
                .serve_at(
                    "/org/freedesktop/portal/desktop",
                    TestPortal { response, uri },
                )
                .unwrap()
                .build()
                .await
                .unwrap();
            let client = zbus::connection::Builder::address(address.trim())
                .unwrap()
                .build()
                .await
                .unwrap();
            let result = request(&client).await;
            if success {
                assert_eq!(result.unwrap(), b"test image");
            } else {
                assert!(matches!(result, Err(PortalError::Failed(_))), "Cancellation and invalid responses must not fall back to another capture method");
            }
        }
        std::fs::remove_file(path).unwrap();
    }
}
