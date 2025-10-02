#[test]
fn composed_checksum() {
    assert_eq!(compositor::checksum(), 15_196_384);
}
