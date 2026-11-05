#[path = "../src/log_activity.rs"]
mod log_activity;

#[test]
fn hidden_logs_skip_lookup_and_visible_logs_reuse_results() {
    log_activity::set_detailed(false);
    assert_eq!(log_activity::resolve_process(65000, |_| panic!("hidden lookup")), None);
    log_activity::set_detailed(true);
    assert_eq!(log_activity::resolve_process(65000, |_| Some("sample".into())), Some("sample".into()));
    assert_eq!(log_activity::resolve_process(65000, |_| panic!("uncached repeated lookup")), Some("sample".into()));
    log_activity::set_detailed(false);
}
