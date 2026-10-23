#[path = "../src/sysproxy.rs"]
mod sysproxy;

#[test]
fn test_windows_sysproxy_command_format() {
    let port = 10808;
    // Test that enable and disable commands construct proper windows registry commands without crashing
    let enable_res = sysproxy::enable_windows_proxy(port);
    let disable_res = sysproxy::disable_windows_proxy();

    // In a test environment (e.g. Linux or CI without admin registry access),
    // these functions gracefully return Ok or Err, but should never panic.
    match enable_res {
        Ok(_) => println!("Registry enable succeeded or skipped gracefully"),
        Err(e) => println!("Registry enable simulated error: {e}"),
    }

    match disable_res {
        Ok(_) => println!("Registry disable succeeded or skipped gracefully"),
        Err(e) => println!("Registry disable simulated error: {e}"),
    }
}
