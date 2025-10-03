#[test]
fn assignment_applies() {
    assert!(settings::execute(&["lang=en"]).contains("lang"));
}
