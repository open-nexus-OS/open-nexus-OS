#[test]
fn power_ready() {
    assert!(powermgr::execute(&[]).contains("power"));
}
