#[test]
fn stable_state() {
    assert!(thermalmgr::execute(&[]).contains("stable"));
}
