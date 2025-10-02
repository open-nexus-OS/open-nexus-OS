#[test]
fn assignment_applies() {
    assert!(settingsd::execute(&["lang=en"]).contains("lang"));
}
