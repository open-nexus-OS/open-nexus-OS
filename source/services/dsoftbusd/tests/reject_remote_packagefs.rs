extern crate alloc;

#[path = "../src/os/gateway/packagefs_ro.rs"]
mod packagefs_ro;

use packagefs_ro::{
    parse_request, RejectReason, PK_MAGIC0, PK_MAGIC1, PK_MAX_PATH_LEN, PK_MAX_READ_LEN, PK_OP_READ,
    PK_OP_STAT, PK_VERSION,
};

fn stat_frame(path: &str) -> alloc::vec::Vec<u8> {
    let bytes = path.as_bytes();
    let mut out = alloc::vec::Vec::with_capacity(6 + bytes.len());
    out.extend_from_slice(&[PK_MAGIC0, PK_MAGIC1, PK_VERSION, PK_OP_STAT]);
    out.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
    out.extend_from_slice(bytes);
    out
}

#[test]
fn test_reject_unauthenticated_stream_request() {
    let req = stat_frame("pkg:/system/build.prop");
    assert_eq!(parse_request(&req, false), Err(RejectReason::Unauthenticated));
}

#[test]
fn test_reject_path_traversal() {
    let req = stat_frame("pkg:/system/../secrets");
    assert_eq!(parse_request(&req, true), Err(RejectReason::PathTraversal));
}

#[test]
fn test_reject_non_packagefs_scheme() {
    let req = stat_frame("file:/etc/passwd");
    assert_eq!(parse_request(&req, true), Err(RejectReason::NonPackagefsScheme));
}

#[test]
fn test_reject_oversize_read_or_path() {
    let long_path = alloc::string::String::from("pkg:/")
        + &"a".repeat(PK_MAX_PATH_LEN + 1);
    let req = stat_frame(&long_path);
    assert_eq!(parse_request(&req, true), Err(RejectReason::OversizedReadOrPath));

    let mut read = [0u8; 14];
    read[0] = PK_MAGIC0;
    read[1] = PK_MAGIC1;
    read[2] = PK_VERSION;
    read[3] = PK_OP_READ;
    read[12..14].copy_from_slice(&((PK_MAX_READ_LEN as u16) + 1).to_le_bytes());
    assert_eq!(parse_request(&read, true), Err(RejectReason::OversizedReadOrPath));
}

#[test]
fn test_packagefs_protocol_symbols_are_linked_for_host_seam() {
    let _ = packagefs_ro::PK_STATUS_OK;
    let _ = packagefs_ro::PK_STATUS_BAD_REQUEST;
    let _ = packagefs_ro::PK_STATUS_UNAUTHENTICATED;
    let _ = packagefs_ro::PK_STATUS_PATH_TRAVERSAL;
    let _ = packagefs_ro::PK_STATUS_NON_PACKAGEFS_SCHEME;
    let _ = packagefs_ro::PK_STATUS_NOT_FOUND;
    let _ = packagefs_ro::PK_STATUS_BADF;
    let _ = packagefs_ro::PK_STATUS_OVERSIZED;
    let _ = packagefs_ro::PK_STATUS_LIMIT;
    let _ = packagefs_ro::PK_STATUS_IO;
    let _ = packagefs_ro::PK_MAX_HANDLES;
    let _ = packagefs_ro::PK_MAX_OPEN_FILE_BYTES;
    let _ = packagefs_ro::PACKAGEFS_KIND_FILE;
    let _ = packagefs_ro::PACKAGEFSD_OP_RESOLVE;

    let _to_status: fn(RejectReason) -> u8 = packagefs_ro::reject_reason_to_status;
    let _decode: fn(&[u8]) -> core::result::Result<Option<packagefs_ro::PackagefsEntry>, ()> =
        packagefs_ro::decode_packagefs_resolve_rsp;
    let _enc_status: fn(u8, u8) -> alloc::vec::Vec<u8> = packagefs_ro::encode_status_only;
    let _enc_stat: fn(u8, u64, u16) -> alloc::vec::Vec<u8> = packagefs_ro::encode_stat_rsp;
    let _enc_open: fn(u8, u32) -> alloc::vec::Vec<u8> = packagefs_ro::encode_open_rsp;
    let _enc_read: fn(u8, &[u8]) -> alloc::vec::Vec<u8> = packagefs_ro::encode_read_rsp;
}
