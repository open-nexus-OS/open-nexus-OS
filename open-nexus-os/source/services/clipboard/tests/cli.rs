#[test]
fn clipboard_roundtrip() {
    clipboard::execute(&["--set", "snippet"]);
    assert!(clipboard::execute(&[]).contains("snippet"));
}
