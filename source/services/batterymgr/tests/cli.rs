#[test]
fn nominal_status() {
    assert!(batterymgr::execute(&[]).contains("nominal"));
}
