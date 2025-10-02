#[test]
fn systemui_checksum() {
    assert_eq!(systemui::checksum(), 182_315_734);
}
