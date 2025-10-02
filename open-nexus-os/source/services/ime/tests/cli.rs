#[test]
fn uppercase_cli() {
    assert_eq!(ime::execute(&["xyz"]), "XYZ");
}
