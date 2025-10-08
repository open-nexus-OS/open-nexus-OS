use nexus_abi::MsgHeader;

#[test]
fn header_matches() {
    let header = MsgHeader::new(1, 2, 3, 4, 5);
    assert_eq!(MsgHeader::from_le_bytes(header.to_le_bytes()), header);
}
