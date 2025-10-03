#[test]
fn dispatcher_listens() {
    assert!(notif::execute(&[]).contains("dispatcher"));
}
