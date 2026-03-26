extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

#[path = "../src/os/gateway/statefs_rw.rs"]
mod statefs_rw;

use statefs::protocol as sfp;
use statefs_rw::{
    audit_label_for_status, encode_reject_response, parse_request, reject_label_for_request,
    reject_reason_to_status, validate_response_shape, RejectReason, RS_MAX_RESPONSE_LEN,
};

fn emulate_statefsd(frame: &[u8], kv: &mut BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let (request, nonce) = sfp::decode_request_with_nonce(frame).expect("decode");
    match request {
        sfp::Request::Put { key, value } => {
            kv.insert(String::from(key), value.to_vec());
            sfp::encode_status_response_with_nonce(sfp::OP_PUT, sfp::STATUS_OK, nonce)
        }
        sfp::Request::Get { key } => {
            if let Some(v) = kv.get(key) {
                sfp::encode_get_response_with_nonce(sfp::STATUS_OK, v, nonce)
            } else {
                sfp::encode_get_response_with_nonce(sfp::STATUS_NOT_FOUND, &[], nonce)
            }
        }
        sfp::Request::Delete { key } => {
            let status =
                if kv.remove(key).is_some() { sfp::STATUS_OK } else { sfp::STATUS_NOT_FOUND };
            sfp::encode_status_response_with_nonce(sfp::OP_DEL, status, nonce)
        }
        sfp::Request::List { prefix, limit } => {
            let mut out = Vec::new();
            for key in kv.keys() {
                if key.starts_with(prefix) {
                    out.push(key.clone());
                    if out.len() >= limit as usize {
                        break;
                    }
                }
            }
            sfp::encode_list_response_with_nonce(sfp::STATUS_OK, &out, RS_MAX_RESPONSE_LEN, nonce)
        }
        sfp::Request::Sync => {
            sfp::encode_status_response_with_nonce(sfp::OP_SYNC, sfp::STATUS_OK, nonce)
        }
        sfp::Request::Reopen => {
            sfp::encode_status_response_with_nonce(sfp::OP_REOPEN, sfp::STATUS_UNSUPPORTED, nonce)
        }
    }
}

#[test]
fn test_gateway_contract_rw_roundtrip_uses_protocol_shapes() {
    let mut kv = BTreeMap::new();
    let key = "/state/shared/selftest/e2e";
    let value = b"nexus";

    let put = sfp::encode_put_request(key, value).expect("put");
    let put_parsed = parse_request(&put, true).expect("parse put");
    let put_rsp = emulate_statefsd(&put, &mut kv);
    assert!(validate_response_shape(put_parsed.op(), put_parsed.nonce(), &put_rsp).is_ok());
    assert_eq!(
        sfp::decode_status_response(sfp::OP_PUT, &put_rsp).expect("put rsp"),
        sfp::STATUS_OK
    );
    assert_eq!(
        audit_label_for_status(sfp::OP_PUT, sfp::STATUS_OK),
        Some("dsoftbusd: audit remote statefs put allow")
    );

    let get = sfp::encode_key_only_request(sfp::OP_GET, key).expect("get");
    let get_parsed = parse_request(&get, true).expect("parse get");
    let get_rsp = emulate_statefsd(&get, &mut kv);
    assert!(validate_response_shape(get_parsed.op(), get_parsed.nonce(), &get_rsp).is_ok());
    assert_eq!(sfp::decode_get_response(&get_rsp).expect("get rsp"), value);

    let del = sfp::encode_key_only_request(sfp::OP_DEL, key).expect("del");
    let del_parsed = parse_request(&del, true).expect("parse del");
    let del_rsp = emulate_statefsd(&del, &mut kv);
    assert!(validate_response_shape(del_parsed.op(), del_parsed.nonce(), &del_rsp).is_ok());
    assert_eq!(
        sfp::decode_status_response(sfp::OP_DEL, &del_rsp).expect("del rsp"),
        sfp::STATUS_OK
    );
    assert_eq!(
        audit_label_for_status(sfp::OP_DEL, sfp::STATUS_OK),
        Some("dsoftbusd: audit remote statefs delete allow")
    );
}

#[test]
fn test_gateway_contract_rejects_outside_acl_before_backend() {
    let req = sfp::encode_put_request("/state/private/forbidden", b"x").expect("encode");
    let reject = match parse_request(&req, true) {
        Ok(_) => panic!("must reject"),
        Err(reason) => reason,
    };
    assert_eq!(reject, RejectReason::WriteOutsideAcl);

    let rsp = encode_reject_response(sfp::OP_PUT, reject);
    assert_eq!(
        sfp::decode_status_response(sfp::OP_PUT, &rsp).expect("status"),
        reject_reason_to_status(RejectReason::WriteOutsideAcl)
    );
    assert_eq!(
        reject_label_for_request(sfp::OP_PUT, RejectReason::WriteOutsideAcl),
        Some("dsoftbusd: audit remote statefs put reject outside_acl")
    );
}

#[test]
fn test_gateway_symbols_are_linked_for_host_seam() {
    let _classify: fn(u8) -> RejectReason = statefs_rw::classify_decode_error;
    let _op_from_frame: fn(&[u8]) -> Option<u8> = statefs_rw::op_from_frame;
    let _nonce_from_frame: fn(&[u8]) -> Option<u64> = statefs_rw::request_nonce_from_frame;
    let _validate: fn(u8, Option<u64>, &[u8]) -> core::result::Result<(), ()> =
        statefs_rw::validate_response_shape;
    let _enc_status: fn(u8, u8, Option<u64>) -> alloc::vec::Vec<u8> =
        statefs_rw::encode_status_response;

    let del_req = sfp::Request::Delete { key: "/state/shared/selftest/link" };
    assert!(statefs_rw::is_mutating_request(&del_req));
}
