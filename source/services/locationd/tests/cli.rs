#[test]
fn fix_estimated() {
    assert!(locationd::execute(&[]).contains("fix"));
}
