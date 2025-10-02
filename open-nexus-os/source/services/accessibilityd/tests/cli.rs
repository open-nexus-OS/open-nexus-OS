#[test]
fn hint_output() {
    assert!(accessibilityd::execute(&["zoom"]).contains("zoom"));
}
