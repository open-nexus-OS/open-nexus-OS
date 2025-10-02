#[test]
fn headless_checksum() {
    assert_eq!(windowd::frame_checksum(), 14_680_288);
}
