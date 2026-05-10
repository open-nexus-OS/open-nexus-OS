//! CONTEXT: Remote packagefs read-only v1 protocol helpers (TASK-0016).
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Host reject tests (`source/services/dsoftbusd/tests/reject_remote_packagefs.rs`)
//!
//! SECURITY INVARIANTS:
//! - Protocol is read-only: STAT/OPEN/READ/CLOSE only.
//! - Inputs are bounded and fail-closed.
//! - Only `pkg:/...` and `/packages/...` namespaces are accepted.
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

pub(crate) const PK_MAGIC0: u8 = b'P';
pub(crate) const PK_MAGIC1: u8 = b'K';
pub(crate) const PK_VERSION: u8 = 1;

pub(crate) const PK_OP_STAT: u8 = 1;
pub(crate) const PK_OP_OPEN: u8 = 2;
pub(crate) const PK_OP_READ: u8 = 3;
pub(crate) const PK_OP_CLOSE: u8 = 4;

pub(crate) const PK_STATUS_OK: u8 = 0;
pub(crate) const PK_STATUS_BAD_REQUEST: u8 = 1;
pub(crate) const PK_STATUS_UNAUTHENTICATED: u8 = 2;
pub(crate) const PK_STATUS_PATH_TRAVERSAL: u8 = 3;
pub(crate) const PK_STATUS_NON_PACKAGEFS_SCHEME: u8 = 4;
pub(crate) const PK_STATUS_NOT_FOUND: u8 = 5;
pub(crate) const PK_STATUS_BADF: u8 = 6;
pub(crate) const PK_STATUS_OVERSIZED: u8 = 7;
pub(crate) const PK_STATUS_LIMIT: u8 = 8;
pub(crate) const PK_STATUS_IO: u8 = 9;

pub(crate) const PK_MAX_PATH_LEN: usize = 192;
pub(crate) const PK_MAX_READ_LEN: usize = 128;
pub(crate) const PK_MAX_HANDLES: usize = 8;
#[allow(dead_code)]
pub(crate) const PK_MAX_OPEN_FILE_BYTES: usize = 16 * 1024;

#[allow(dead_code)]
pub(crate) const PACKAGEFS_KIND_FILE: u16 = 0;
#[allow(dead_code)]
pub(crate) const PACKAGEFSD_OP_RESOLVE: u8 = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PackagefsEntry {
    pub(crate) kind: u16,
    pub(crate) size: u64,
    pub(crate) bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PackagefsRequest {
    Stat { rel_path: String },
    Open { rel_path: String },
    Read { handle: u32, offset: u32, read_len: u16 },
    Close { handle: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RejectReason {
    BadRequest,
    Unauthenticated,
    PathTraversal,
    NonPackagefsScheme,
    OversizedReadOrPath,
}

pub(crate) fn reject_reason_to_status(reason: RejectReason) -> u8 {
    match reason {
        RejectReason::BadRequest => PK_STATUS_BAD_REQUEST,
        RejectReason::Unauthenticated => PK_STATUS_UNAUTHENTICATED,
        RejectReason::PathTraversal => PK_STATUS_PATH_TRAVERSAL,
        RejectReason::NonPackagefsScheme => PK_STATUS_NON_PACKAGEFS_SCHEME,
        RejectReason::OversizedReadOrPath => PK_STATUS_OVERSIZED,
    }
}

pub(crate) fn parse_request(
    frame: &[u8],
    authenticated: bool,
) -> core::result::Result<PackagefsRequest, RejectReason> {
    if !authenticated {
        return Err(RejectReason::Unauthenticated);
    }
    if frame.len() < 4 {
        return Err(RejectReason::BadRequest);
    }
    if frame[0] != PK_MAGIC0 || frame[1] != PK_MAGIC1 || frame[2] != PK_VERSION {
        return Err(RejectReason::BadRequest);
    }
    match frame[3] {
        PK_OP_STAT | PK_OP_OPEN => parse_path_request(frame),
        PK_OP_READ => parse_read_request(frame),
        PK_OP_CLOSE => parse_close_request(frame),
        _ => Err(RejectReason::BadRequest),
    }
}

fn parse_path_request(frame: &[u8]) -> core::result::Result<PackagefsRequest, RejectReason> {
    if frame.len() < 6 {
        return Err(RejectReason::BadRequest);
    }
    let path_len = u16::from_le_bytes([frame[4], frame[5]]) as usize;
    if path_len == 0 || path_len > PK_MAX_PATH_LEN {
        return Err(RejectReason::OversizedReadOrPath);
    }
    if frame.len() != 6 + path_len {
        return Err(RejectReason::BadRequest);
    }
    let path = core::str::from_utf8(&frame[6..]).map_err(|_| RejectReason::BadRequest)?;
    let rel_path = normalize_packagefs_path(path)?;
    match frame[3] {
        PK_OP_STAT => Ok(PackagefsRequest::Stat { rel_path }),
        PK_OP_OPEN => Ok(PackagefsRequest::Open { rel_path }),
        _ => Err(RejectReason::BadRequest),
    }
}

fn parse_read_request(frame: &[u8]) -> core::result::Result<PackagefsRequest, RejectReason> {
    if frame.len() != 14 {
        return Err(RejectReason::BadRequest);
    }
    let handle = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
    let offset = u32::from_le_bytes([frame[8], frame[9], frame[10], frame[11]]);
    let read_len = u16::from_le_bytes([frame[12], frame[13]]);
    if read_len == 0 || (read_len as usize) > PK_MAX_READ_LEN {
        return Err(RejectReason::OversizedReadOrPath);
    }
    Ok(PackagefsRequest::Read { handle, offset, read_len })
}

fn parse_close_request(frame: &[u8]) -> core::result::Result<PackagefsRequest, RejectReason> {
    if frame.len() != 8 {
        return Err(RejectReason::BadRequest);
    }
    let handle = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
    Ok(PackagefsRequest::Close { handle })
}

pub(crate) fn normalize_packagefs_path(path: &str) -> core::result::Result<String, RejectReason> {
    let rel = if let Some(rest) = path.strip_prefix("pkg:/") {
        rest
    } else if let Some(rest) = path.strip_prefix("/packages/") {
        rest
    } else {
        return Err(RejectReason::NonPackagefsScheme);
    };
    if rel.is_empty() || rel.len() > PK_MAX_PATH_LEN {
        return Err(RejectReason::OversizedReadOrPath);
    }

    let mut normalized = String::new();
    let mut first = true;
    for seg in rel.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." {
            return Err(RejectReason::PathTraversal);
        }
        if seg.bytes().any(|b| b == b'\\' || b == 0) {
            return Err(RejectReason::PathTraversal);
        }
        if !first {
            normalized.push('/');
        }
        normalized.push_str(seg);
        first = false;
    }
    if normalized.is_empty() {
        return Err(RejectReason::PathTraversal);
    }
    Ok(normalized)
}

pub(crate) fn decode_packagefs_resolve_rsp(
    rsp: &[u8],
) -> core::result::Result<Option<PackagefsEntry>, ()> {
    if rsp.len() < 11 {
        return Err(());
    }
    let found = rsp[0];
    if found == 0 {
        return Ok(None);
    }
    if found != 1 {
        return Err(());
    }
    let size = u64::from_le_bytes([rsp[1], rsp[2], rsp[3], rsp[4], rsp[5], rsp[6], rsp[7], rsp[8]]);
    let kind = u16::from_le_bytes([rsp[9], rsp[10]]);
    if kind != PACKAGEFS_KIND_FILE {
        return Err(());
    }
    let bytes = rsp[11..].to_vec();
    if bytes.len() > PK_MAX_OPEN_FILE_BYTES {
        return Err(());
    }
    Ok(Some(PackagefsEntry { kind, size, bytes }))
}

#[allow(dead_code)]
pub(crate) fn encode_packagefs_resolve_req(rel_path: &str) -> Vec<u8> {
    let mut req = Vec::with_capacity(1 + rel_path.len());
    req.push(PACKAGEFSD_OP_RESOLVE);
    req.extend_from_slice(rel_path.as_bytes());
    req
}

pub(crate) fn encode_status_only(op: u8, status: u8) -> Vec<u8> {
    vec![PK_MAGIC0, PK_MAGIC1, PK_VERSION, op | 0x80, status]
}

pub(crate) fn encode_stat_rsp(status: u8, size: u64, kind: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(15);
    out.extend_from_slice(&[PK_MAGIC0, PK_MAGIC1, PK_VERSION, PK_OP_STAT | 0x80, status]);
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&kind.to_le_bytes());
    out
}

pub(crate) fn encode_open_rsp(status: u8, handle: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(9);
    out.extend_from_slice(&[PK_MAGIC0, PK_MAGIC1, PK_VERSION, PK_OP_OPEN | 0x80, status]);
    out.extend_from_slice(&handle.to_le_bytes());
    out
}

pub(crate) fn encode_read_rsp(status: u8, data: &[u8]) -> Vec<u8> {
    let n = core::cmp::min(data.len(), PK_MAX_READ_LEN);
    let mut out = Vec::with_capacity(7 + n);
    out.extend_from_slice(&[PK_MAGIC0, PK_MAGIC1, PK_VERSION, PK_OP_READ | 0x80, status]);
    out.extend_from_slice(&(n as u16).to_le_bytes());
    out.extend_from_slice(&data[..n]);
    out
}
