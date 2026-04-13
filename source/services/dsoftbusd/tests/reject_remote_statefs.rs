extern crate alloc;

#[path = "../src/os/gateway/statefs_rw.rs"]
mod statefs_rw;

use statefs::protocol as sfp;
use statefs_rw::{parse_request, RejectReason, RS_MAX_VALUE_LEN};

type ValidateResponseShapeFn = fn(u8, Option<u64>, &[u8]) -> core::result::Result<(), ()>;

#[test]
fn test_reject_statefs_write_outside_acl() {
    let req = sfp::encode_put_request("/state/private/secret", b"deny").expect("encode");
    assert!(matches!(
        parse_request(&req, true),
        Err(RejectReason::WriteOutsideAcl)
    ));
}

#[test]
fn test_reject_statefs_prefix_escape() {
    let req = sfp::encode_put_request("/state/shared/../escape", b"deny").expect("encode");
    assert!(matches!(
        parse_request(&req, true),
        Err(RejectReason::PrefixEscape)
    ));
}

#[test]
fn test_reject_oversize_statefs_write() {
    let oversized = alloc::vec![0xAA; RS_MAX_VALUE_LEN + 1];
    let req =
        sfp::encode_put_request("/state/shared/selftest/oversize", &oversized).expect("encode");
    assert!(matches!(
        parse_request(&req, true),
        Err(RejectReason::Oversized)
    ));
}

#[test]
fn test_reject_unauthenticated_statefs_request() {
    let req = sfp::encode_put_request("/state/shared/selftest/auth", b"v").expect("encode");
    assert!(matches!(
        parse_request(&req, false),
        Err(RejectReason::Unauthenticated)
    ));
}

#[test]
fn test_reject_malformed_empty_frame() {
    assert!(matches!(
        parse_request(&[], true),
        Err(RejectReason::BadRequest)
    ));
}

#[test]
fn test_statefs_protocol_symbols_are_linked_for_host_seam() {
    let _ = statefs_rw::RS_MAX_FRAME_LEN;
    let _ = statefs_rw::RS_MAX_KEY_LEN;
    let _ = statefs_rw::RS_MAX_VALUE_LEN;
    let _ = statefs_rw::RS_MAX_LIST_LIMIT;
    let _ = statefs_rw::RS_MAX_RESPONSE_LEN;
    let _ = statefs_rw::RS_SHARED_PREFIX;

    let _classify: fn(u8) -> RejectReason = statefs_rw::classify_decode_error;
    let _to_status: fn(RejectReason) -> u8 = statefs_rw::reject_reason_to_status;
    let _enc_reject: fn(u8, RejectReason) -> alloc::vec::Vec<u8> =
        statefs_rw::encode_reject_response;
    let _enc_status: fn(u8, u8, Option<u64>) -> alloc::vec::Vec<u8> =
        statefs_rw::encode_status_response;
    let _validate: ValidateResponseShapeFn = statefs_rw::validate_response_shape;
    let _op_from_frame: fn(&[u8]) -> Option<u8> = statefs_rw::op_from_frame;
    let _nonce_from_frame: fn(&[u8]) -> Option<u64> = statefs_rw::request_nonce_from_frame;
    let _reject_label: fn(u8, RejectReason) -> Option<&'static str> =
        statefs_rw::reject_label_for_request;
    let _audit_label: fn(u8, u8) -> Option<&'static str> = statefs_rw::audit_label_for_status;

    let req = sfp::encode_put_request("/state/shared/selftest/link", b"v").expect("encode");
    let parsed = parse_request(&req, true).expect("parse");
    let _ = parsed.request;
    let _ = parsed.nonce;
    let _ = parsed.op();
    let _ = parsed.nonce();

    let put_req = sfp::Request::Put {
        key: "/state/shared/selftest/link",
        value: b"v",
    };
    assert!(statefs_rw::is_mutating_request(&put_req));
    assert_eq!(statefs_rw::request_op(&put_req), sfp::OP_PUT);
}
