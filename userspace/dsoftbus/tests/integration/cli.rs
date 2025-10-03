#[test]
fn healthy_status() {
    assert!(dsoftbus::execute(&["--status", "node9"]).contains("healthy"));
}
