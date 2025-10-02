#[test]
fn accepts_valid() {
    assert!(identity::execute(&["node42"]).contains("accepted"));
}
