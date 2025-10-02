#[test]
fn capture_log() {
    assert!(logd::execute(&["event"]).contains("event"));
}
