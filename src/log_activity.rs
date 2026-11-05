use std::collections::HashMap;
use std::sync::{atomic::{AtomicBool, Ordering}, Mutex};
use std::time::{Duration, Instant};

static DETAILED: AtomicBool = AtomicBool::new(false);
static PROCESS_CACHE: Mutex<Option<(Instant, HashMap<u16, Option<String>>)>> = Mutex::new(None);

pub fn set_detailed(visible: bool) { DETAILED.store(visible, Ordering::Relaxed); }
pub fn detailed() -> bool { DETAILED.load(Ordering::Relaxed) }

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

