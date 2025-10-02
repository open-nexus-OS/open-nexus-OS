use nexus_abi::MsgHeader;

#[test]
fn header_matches() {
    let header = MsgHeader::new(7, 8, 9);
    assert_eq!(MsgHeader::deserialize(header.serialize()), header);
}
