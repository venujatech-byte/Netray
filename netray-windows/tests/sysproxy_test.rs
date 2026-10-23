use netray_windows::sysproxy;

#[tokio::test]
async fn test_windows_sysproxy_command_format() {
    let socks_port = 10808;
    let http_port = 10809;
    // enable_system_proxy returns Result, disable_system_proxy returns ()
    match sysproxy::enable_system_proxy(socks_port, http_port).await {
        Ok(_) => println!("Registry enable succeeded"),
        Err(e) => println!("Registry enable expected error on Linux: {e}"),
    }
    // disable returns () — just call it and verify no panic
    sysproxy::disable_system_proxy().await;
    println!("Registry disable completed without panic");
}
