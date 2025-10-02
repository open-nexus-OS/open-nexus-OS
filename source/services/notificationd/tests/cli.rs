#[test]
fn dispatcher_listens() {
    assert!(notificationd::execute(&[]).contains("dispatcher"));
}
