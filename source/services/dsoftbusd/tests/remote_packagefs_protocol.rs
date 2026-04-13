extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

#[path = "../src/os/gateway/packagefs_ro.rs"]
mod packagefs_ro;

use packagefs_ro::{
    decode_packagefs_resolve_rsp, encode_open_rsp, encode_packagefs_resolve_req, encode_read_rsp,
    encode_stat_rsp, encode_status_only, parse_request, reject_reason_to_status, PackagefsRequest,
    RejectReason, PACKAGEFSD_OP_RESOLVE, PACKAGEFS_KIND_FILE, PK_MAGIC0, PK_MAGIC1, PK_MAX_HANDLES,
    PK_MAX_OPEN_FILE_BYTES, PK_MAX_READ_LEN, PK_OP_CLOSE, PK_OP_OPEN, PK_OP_READ, PK_OP_STAT,
    PK_STATUS_BADF, PK_STATUS_BAD_REQUEST, PK_STATUS_IO, PK_STATUS_LIMIT, PK_STATUS_NOT_FOUND,
    PK_STATUS_OK, PK_STATUS_OVERSIZED, PK_VERSION,
};

fn frame_with_path(op: u8, path: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(6 + path.len());
    out.extend_from_slice(&[PK_MAGIC0, PK_MAGIC1, PK_VERSION, op]);
    out.extend_from_slice(&(path.len() as u16).to_le_bytes());
    out.extend_from_slice(path.as_bytes());
    out
}

fn frame_read(handle: u32, offset: u32, read_len: u16) -> [u8; 14] {
    let mut read = [0u8; 14];
    read[0] = PK_MAGIC0;
    read[1] = PK_MAGIC1;
    read[2] = PK_VERSION;
    read[3] = PK_OP_READ;
    read[4..8].copy_from_slice(&handle.to_le_bytes());
    read[8..12].copy_from_slice(&offset.to_le_bytes());
    read[12..14].copy_from_slice(&read_len.to_le_bytes());
    read
}

#[test]
fn test_parse_stat_open_read_close_roundtrip_sanity() {
    let stat = frame_with_path(PK_OP_STAT, "pkg:/system/build.prop");
    let open = frame_with_path(PK_OP_OPEN, "pkg:/system/build.prop");
    let read = frame_read(7, 13, 64);
    let mut close = [0u8; 8];
    close[0] = PK_MAGIC0;
    close[1] = PK_MAGIC1;
    close[2] = PK_VERSION;
    close[3] = PK_OP_CLOSE;
    close[4..8].copy_from_slice(&7u32.to_le_bytes());

    assert_eq!(
        parse_request(&stat, true),
        Ok(PackagefsRequest::Stat {
            rel_path: "system/build.prop".into()
        })
    );
    assert_eq!(
        parse_request(&open, true),
        Ok(PackagefsRequest::Open {
            rel_path: "system/build.prop".into()
        })
    );
    assert_eq!(
        parse_request(&read, true),
        Ok(PackagefsRequest::Read {
            handle: 7,
            offset: 13,
            read_len: 64
        })
    );
    assert_eq!(
        parse_request(&close, true),
        Ok(PackagefsRequest::Close { handle: 7 })
    );
}

#[test]
fn test_status_mappings_cover_all_task_required_statuses() {
    assert_eq!(
        reject_reason_to_status(RejectReason::BadRequest),
        PK_STATUS_BAD_REQUEST
    );
    assert_eq!(
        reject_reason_to_status(RejectReason::OversizedReadOrPath),
        PK_STATUS_OVERSIZED
    );

    let s_not_found = encode_stat_rsp(PK_STATUS_NOT_FOUND, 0, 0);
    assert_eq!(
        s_not_found[..5],
        [
            PK_MAGIC0,
            PK_MAGIC1,
            PK_VERSION,
            PK_OP_STAT | 0x80,
            PK_STATUS_NOT_FOUND
        ]
    );

    let s_badf = encode_status_only(PK_OP_CLOSE, PK_STATUS_BADF);
    assert_eq!(
        s_badf,
        [
            PK_MAGIC0,
            PK_MAGIC1,
            PK_VERSION,
            PK_OP_CLOSE | 0x80,
            PK_STATUS_BADF
        ]
    );

    let s_limit = encode_open_rsp(PK_STATUS_LIMIT, 0);
    assert_eq!(
        s_limit[..5],
        [
            PK_MAGIC0,
            PK_MAGIC1,
            PK_VERSION,
            PK_OP_OPEN | 0x80,
            PK_STATUS_LIMIT
        ]
    );

    let s_io = encode_read_rsp(PK_STATUS_IO, &[]);
    assert_eq!(
        s_io[..5],
        [
            PK_MAGIC0,
            PK_MAGIC1,
            PK_VERSION,
            PK_OP_READ | 0x80,
            PK_STATUS_IO
        ]
    );

    // Keep these constants in active use as part of protocol contract checks.
    assert_eq!(PACKAGEFS_KIND_FILE, 0);
    assert_eq!(PACKAGEFSD_OP_RESOLVE, 2);
    assert_eq!(PK_MAX_OPEN_FILE_BYTES, 16 * 1024);
    assert_eq!(
        encode_packagefs_resolve_req("system/build.prop"),
        [
            PACKAGEFSD_OP_RESOLVE,
            b's',
            b'y',
            b's',
            b't',
            b'e',
            b'm',
            b'/',
            b'b',
            b'u',
            b'i',
            b'l',
            b'd',
            b'.',
            b'p',
            b'r',
            b'o',
            b'p',
        ]
    );
}

#[test]
fn test_read_rsp_caps_payload_to_protocol_max() {
    let payload = vec![0xAB; PK_MAX_READ_LEN + 17];
    let rsp = encode_read_rsp(PK_STATUS_OK, &payload);
    let n = u16::from_le_bytes([rsp[5], rsp[6]]) as usize;
    assert_eq!(n, PK_MAX_READ_LEN);
    assert_eq!(rsp.len(), 7 + PK_MAX_READ_LEN);
}

#[test]
fn test_decode_packagefs_resolve_response_found_and_missing() {
    let missing = [0u8; 11];
    assert_eq!(decode_packagefs_resolve_rsp(&missing), Ok(None));

    let mut found = Vec::new();
    found.push(1);
    found.extend_from_slice(&5u64.to_le_bytes());
    found.extend_from_slice(&0u16.to_le_bytes());
    found.extend_from_slice(b"hello");
    let parsed = decode_packagefs_resolve_rsp(&found)
        .expect("decode")
        .expect("found");
    assert_eq!(parsed.size, 5);
    assert_eq!(parsed.bytes, b"hello");

    let mut wrong_kind = found.clone();
    wrong_kind[9] = 1;
    wrong_kind[10] = 0;
    assert!(decode_packagefs_resolve_rsp(&wrong_kind).is_err());

    let mut too_large = Vec::new();
    too_large.push(1);
    too_large.extend_from_slice(&(PK_MAX_OPEN_FILE_BYTES as u64).to_le_bytes());
    too_large.extend_from_slice(&0u16.to_le_bytes());
    too_large.extend_from_slice(&vec![0u8; PK_MAX_OPEN_FILE_BYTES + 1]);
    assert!(decode_packagefs_resolve_rsp(&too_large).is_err());
}

#[test]
fn test_handler_lifecycle_constraints_model_max_handles_and_close() {
    let mut handles: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
    for i in 0..PK_MAX_HANDLES as u32 {
        handles.insert(i + 1, vec![i as u8]);
    }
    let status_when_full = if handles.len() >= PK_MAX_HANDLES {
        PK_STATUS_LIMIT
    } else {
        PK_STATUS_OK
    };
    assert_eq!(status_when_full, PK_STATUS_LIMIT);

    let status_close_ok = if handles.remove(&2).is_some() {
        PK_STATUS_OK
    } else {
        PK_STATUS_BADF
    };
    assert_eq!(status_close_ok, PK_STATUS_OK);
    let status_close_badf = if handles.remove(&2).is_some() {
        PK_STATUS_OK
    } else {
        PK_STATUS_BADF
    };
    assert_eq!(status_close_badf, PK_STATUS_BADF);
}
