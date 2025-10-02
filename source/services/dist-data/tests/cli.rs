#[test]
fn sync_message_contains_bus() {
    assert!(dist_data::execute(&["node8"]).contains("dsoftbus"));
}
