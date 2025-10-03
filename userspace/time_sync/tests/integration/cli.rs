#[test]
fn offset_applied() {
    assert!(time_sync::execute(&["15"]).contains("15"));
}
