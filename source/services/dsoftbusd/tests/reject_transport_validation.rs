#[path = "../src/os/netstack/validate.rs"]
mod validate;

#[test]
fn test_reject_nonce_mismatch_response() {
    let nonce_ok = 0xAA55AA55AA55AA55u64;
    let nonce_bad = 0xAA55AA55AA55AA56u64;
    let mut rsp = [0u8; 13];
    rsp[0] = b'N';
    rsp[1] = b'S';
    rsp[2] = 1;
    rsp[3] = 0x84;
    rsp[4] = 0;
    rsp[5..13].copy_from_slice(&nonce_bad.to_le_bytes());
    assert!(!validate::response_matches(&rsp, 0x84, nonce_ok));
}

#[test]
fn test_reject_unexpected_response_opcode() {
    let nonce = 7u64;
    let mut rsp = [0u8; 13];
    rsp[0] = b'N';
    rsp[1] = b'S';
    rsp[2] = 1;
    rsp[3] = 0x85;
    rsp[4] = 0;
    rsp[5..13].copy_from_slice(&nonce.to_le_bytes());
    assert!(!validate::response_matches(&rsp, 0x84, nonce));
}

#[test]
fn test_reject_zero_length_status_ok_read_frame() {
    let rsp = [b'N', b'S', 1, 0x84, 0, 0, 0];
    assert!(validate::parse_read_ok_len(&rsp).is_err());
}

#[test]
fn test_reject_oversized_udp_payload() {
    assert!(!validate::is_valid_udp_payload_len(257));
    assert!(validate::is_valid_udp_payload_len(256));
}

#[test]
fn test_parse_helpers_cover_status_and_nonce_extraction() {
    let nonce = 0x1122334455667788u64;
    let mut rsp = [0u8; 16];
    rsp[0] = b'N';
    rsp[1] = b'S';
    rsp[2] = 1;
    rsp[3] = 0x85;
    rsp[4] = 0;
    rsp[5] = 2;
    rsp[6] = 0;
    rsp[8..16].copy_from_slice(&nonce.to_le_bytes());

    assert_eq!(validate::parse_status_frame(&rsp, 0x85), Ok(0));
    assert_eq!(validate::parse_write_ok_wrote(&rsp), Ok(2));
    assert_eq!(validate::extract_netstack_reply_nonce(&rsp), Some(nonce));
}
