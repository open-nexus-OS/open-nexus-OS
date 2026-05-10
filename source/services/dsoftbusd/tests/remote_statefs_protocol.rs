extern crate alloc;

#[path = "../src/os/gateway/statefs_rw.rs"]
mod statefs_rw;

use statefs::protocol as sfp;
use statefs_rw::{
    audit_label_for_status, encode_reject_response, op_from_frame, parse_request,
    reject_reason_to_status, request_nonce_from_frame, request_op, validate_response_shape,
    RejectReason, RS_SHARED_PREFIX,
};

#[test]
fn test_remote_statefs_parse_allows_shared_put_get_delete() {
    let put = sfp::encode_put_request("/state/shared/selftest/key", b"abc").expect("put");
    let parsed = parse_request(&put, true).expect("parse put");
    assert_eq!(request_op(&parsed.request), sfp::OP_PUT);

    let get = sfp::encode_key_only_request(sfp::OP_GET, "/state/shared/selftest/key").expect("get");
    let parsed = parse_request(&get, true).expect("parse get");
    assert_eq!(request_op(&parsed.request), sfp::OP_GET);

    let del = sfp::encode_key_only_request(sfp::OP_DEL, "/state/shared/selftest/key").expect("del");
    let parsed = parse_request(&del, true).expect("parse del");
    assert_eq!(request_op(&parsed.request), sfp::OP_DEL);

    assert_eq!(RS_SHARED_PREFIX, "/state/shared/");
}

#[test]
fn test_remote_statefs_reject_mapping_is_deterministic() {
    assert_eq!(
        reject_reason_to_status(RejectReason::BadRequest),
        sfp::STATUS_MALFORMED
    );
    assert_eq!(
        reject_reason_to_status(RejectReason::Unauthenticated),
        sfp::STATUS_ACCESS_DENIED
    );
    assert_eq!(
        reject_reason_to_status(RejectReason::WriteOutsideAcl),
        sfp::STATUS_ACCESS_DENIED
    );
    assert_eq!(
        reject_reason_to_status(RejectReason::PrefixEscape),
        sfp::STATUS_INVALID_KEY
    );
    assert_eq!(
        reject_reason_to_status(RejectReason::Oversized),
        sfp::STATUS_VALUE_TOO_LARGE
    );

    let rsp = encode_reject_response(sfp::OP_PUT, RejectReason::WriteOutsideAcl);
    assert_eq!(rsp[0], sfp::MAGIC0);
    assert_eq!(rsp[1], sfp::MAGIC1);
    assert_eq!(rsp[3], sfp::OP_PUT | 0x80);
    assert_eq!(rsp[4], sfp::STATUS_ACCESS_DENIED);
}

#[test]
fn test_remote_statefs_response_shape_validation() {
    let rsp = sfp::encode_status_response(sfp::OP_DEL, sfp::STATUS_OK);
    assert!(validate_response_shape(sfp::OP_DEL, None, &rsp).is_ok());

    let mut malformed = rsp.clone();
    malformed[3] = sfp::OP_GET | 0x80;
    assert!(validate_response_shape(sfp::OP_DEL, None, &malformed).is_err());
}

#[test]
fn test_remote_statefs_audit_label_contract() {
    assert_eq!(
        audit_label_for_status(sfp::OP_PUT, sfp::STATUS_OK),
        Some("dsoftbusd: audit remote statefs put allow")
    );
    assert_eq!(
        audit_label_for_status(sfp::OP_DEL, sfp::STATUS_ACCESS_DENIED),
        Some("dsoftbusd: audit remote statefs delete reject status")
    );
    assert_eq!(audit_label_for_status(sfp::OP_GET, sfp::STATUS_OK), None);
}

#[test]
fn test_remote_statefs_v2_nonce_correlation_is_strict() {
    let v2_rsp = sfp::encode_status_response_with_nonce(sfp::OP_SYNC, sfp::STATUS_OK, Some(42));
    assert!(validate_response_shape(sfp::OP_SYNC, None, &v2_rsp).is_err());
    assert!(validate_response_shape(sfp::OP_SYNC, Some(42), &v2_rsp).is_ok());
    assert!(validate_response_shape(sfp::OP_SYNC, Some(41), &v2_rsp).is_err());
}

#[test]
fn test_remote_statefs_parse_empty_is_bad_request() {
    assert!(matches!(
        parse_request(&[], true),
        Err(RejectReason::BadRequest)
    ));
}

#[test]
fn test_remote_statefs_frame_header_helpers_are_fail_closed() {
    let mut bad_ver = sfp::encode_sync_request();
    bad_ver[2] = 0xFF;
    assert_eq!(op_from_frame(&bad_ver), None);
    assert_eq!(request_nonce_from_frame(&bad_ver), None);

    let mut v2_req = sfp::encode_sync_request();
    v2_req[2] = sfp::VERSION_V2;
    v2_req.splice(4..4, 7u64.to_le_bytes());
    assert_eq!(request_nonce_from_frame(&v2_req), Some(7));
}

#[test]
fn test_remote_statefs_symbols_are_linked_for_host_seam() {
    let _ = statefs_rw::RS_MAX_FRAME_LEN;
    let _ = statefs_rw::RS_MAX_KEY_LEN;
    let _ = statefs_rw::RS_MAX_VALUE_LEN;
    let _ = statefs_rw::RS_MAX_LIST_LIMIT;
    let _ = statefs_rw::RS_MAX_RESPONSE_LEN;

    let req = sfp::encode_key_only_request(sfp::OP_GET, "/state/shared/selftest/symbol-link")
        .expect("req");
    let parsed = parse_request(&req, true).expect("parse");
    assert_eq!(parsed.op(), sfp::OP_GET);
    assert_eq!(parsed.nonce(), None);

    let put_req = sfp::Request::Put {
        key: "/state/shared/selftest/symbol-link",
        value: b"v",
    };
    assert!(statefs_rw::is_mutating_request(&put_req));
    assert_eq!(
        statefs_rw::reject_label_for_request(sfp::OP_PUT, RejectReason::Oversized),
        Some("dsoftbusd: audit remote statefs put reject oversized")
    );
    let _status_rsp = statefs_rw::encode_status_response(sfp::OP_SYNC, sfp::STATUS_OK, None);
}
