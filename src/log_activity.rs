use std::collections::HashMap;
use std::sync::{atomic::{AtomicBool, Ordering}, Mutex};
use std::time::{Duration, Instant};

static DETAILED: AtomicBool = AtomicBool::new(false);
static ENABLED: AtomicBool = AtomicBool::new(true);
static PROCESS_CACHE: Mutex<Option<(Instant, HashMap<u16, Option<String>>)>> = Mutex::new(None);

pub fn set_detailed(visible: bool) { DETAILED.store(visible, Ordering::Relaxed); }
pub fn set_enabled(enabled: bool) { ENABLED.store(enabled, Ordering::Relaxed); }
pub fn enabled() -> bool { ENABLED.load(Ordering::Relaxed) }
pub fn detailed() -> bool { enabled() && DETAILED.load(Ordering::Relaxed) }

pub fn is_connection_event(source: &str, message: &str) -> bool {
    (source == "Supervisor"
        && (message.starts_with("Tunnel ready ·")
            || message == "Tunnel is offline."
            || message.starts_with("Tunnel stopped by user.")))
        || (source == "SSH" && message == "SSH Authenticated successfully!")
        || (source == "Connection" && message == "Tunnel connected.")
}

pub fn should_store(enabled: bool, source: &str, message: &str) -> bool {
    enabled || is_connection_event(source, message)
}

pub fn should_forward(enabled: bool, source: &str, message: &str, is_error: bool) -> bool {
    enabled
        || is_connection_event(source, message)
        || is_error
        || (source == "Supervisor" && message.starts_with("Connection not yet verified:"))
        || (source == "Xray" && message.contains("proxy/vless/outbound: tunneling request to"))
}

pub fn resolve_process(port: u16, lookup: impl FnOnce(u16) -> Option<String>) -> Option<String> {
    if port == 0 || !detailed() { return None; }
    let mut guard = PROCESS_CACHE.lock().unwrap_or_else(|error| error.into_inner());
    let (updated, entries) = guard.get_or_insert_with(|| (Instant::now(), HashMap::new()));
    // Keep attribution best-effort: source ports can be reused by another process.
    if updated.elapsed() >= Duration::from_millis(250) || entries.len() >= 128 {
        entries.clear();
        *updated = Instant::now();
    }
    entries.entry(port).or_insert_with(|| lookup(port)).clone()
}

#[cfg(test)]
mod tests {
    use super::{is_connection_event, should_forward, should_store};

    #[test]
    fn disabled_logs_keep_only_lifecycle_entries_but_forward_state_signals() {
        assert!(should_store(false, "Supervisor", "Tunnel ready · HTTP 204 through proxy in 9 ms."));
        assert!(should_store(false, "Supervisor", "Tunnel is offline."));
        assert!(!should_store(false, "Xray", "app/dispatcher: taking detour [proxy]"));
        assert!(should_forward(false, "Xray", "connection failed", true));
        assert!(!should_forward(false, "DNS", "resolved example.com", false));
        assert!(!is_connection_event("Xray", "connection failed"));
    }
}

