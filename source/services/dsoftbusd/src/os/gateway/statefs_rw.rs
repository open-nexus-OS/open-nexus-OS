//! CONTEXT: Remote statefs read-write v1 protocol guards (TASK-0017).
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Host reject tests (`source/services/dsoftbusd/tests/reject_remote_statefs.rs`)
//!
//! SECURITY INVARIANTS:
//! - Remote statefs requests require authenticated sessions.
//! - ACL is deny-by-default and constrained to `/state/shared/*`.
//! - Prefix escapes and oversized frames fail closed deterministically.
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

use statefs::protocol as sfp;

pub(crate) const RS_MAX_FRAME_LEN: usize = 256;
pub(crate) const RS_MAX_KEY_LEN: usize = 96;
pub(crate) const RS_MAX_VALUE_LEN: usize = 128;
pub(crate) const RS_MAX_LIST_LIMIT: u16 = 64;
pub(crate) const RS_MAX_RESPONSE_LEN: usize = 512;
pub(crate) const RS_SHARED_PREFIX: &str = "/state/shared/";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RejectReason {
    BadRequest,
    Unauthenticated,
    WriteOutsideAcl,
    PrefixEscape,
    Oversized,
}

pub(crate) struct ParsedRequest<'a> {
    pub(crate) request: sfp::Request<'a>,
    pub(crate) nonce: Option<u64>,
}

impl ParsedRequest<'_> {
    #[must_use]
    pub(crate) fn op(&self) -> u8 {
        request_op(&self.request)
    }

    #[must_use]
    pub(crate) fn nonce(&self) -> Option<u64> {
        self.nonce
    }
}

pub(crate) fn parse_request(
    frame: &[u8],
    authenticated: bool,
) -> core::result::Result<ParsedRequest<'_>, RejectReason> {
    if !authenticated {
        return Err(RejectReason::Unauthenticated);
    }
    if frame.is_empty() {
        return Err(RejectReason::BadRequest);
    }
    if frame.len() > RS_MAX_FRAME_LEN {
        return Err(RejectReason::Oversized);
    }
    let (request, nonce) = sfp::decode_request_with_nonce(frame).map_err(classify_decode_error)?;
    validate_request_bounds_and_acl(&request)?;
    Ok(ParsedRequest { request, nonce })
}

#[must_use]
pub(crate) fn classify_decode_error(status: u8) -> RejectReason {
    match status {
        sfp::STATUS_KEY_TOO_LONG | sfp::STATUS_VALUE_TOO_LARGE => RejectReason::Oversized,
        sfp::STATUS_INVALID_KEY => RejectReason::PrefixEscape,
        _ => RejectReason::BadRequest,
    }
}

#[must_use]
pub(crate) fn reject_reason_to_status(reason: RejectReason) -> u8 {
    match reason {
        RejectReason::BadRequest => sfp::STATUS_MALFORMED,
        RejectReason::Unauthenticated => sfp::STATUS_ACCESS_DENIED,
        RejectReason::WriteOutsideAcl => sfp::STATUS_ACCESS_DENIED,
        RejectReason::PrefixEscape => sfp::STATUS_INVALID_KEY,
        RejectReason::Oversized => sfp::STATUS_VALUE_TOO_LARGE,
    }
}

#[must_use]
pub(crate) fn encode_reject_response(op: u8, reason: RejectReason) -> alloc::vec::Vec<u8> {
    sfp::encode_status_response(op, reject_reason_to_status(reason))
}

#[must_use]
pub(crate) fn encode_status_response(
    op: u8,
    status: u8,
    nonce: Option<u64>,
) -> alloc::vec::Vec<u8> {
    sfp::encode_status_response_with_nonce(op, status, nonce)
}

pub(crate) fn validate_response_shape(
    request_op: u8,
    request_nonce: Option<u64>,
    frame: &[u8],
) -> core::result::Result<(), ()> {
    if frame.len() < 5 || frame.len() > RS_MAX_RESPONSE_LEN {
        return Err(());
    }
    if frame[0] != sfp::MAGIC0 || frame[1] != sfp::MAGIC1 {
        return Err(());
    }
    if frame[3] != (request_op | 0x80) {
        return Err(());
    }
    match frame[2] {
        sfp::VERSION => Ok(()),
        sfp::VERSION_V2 => {
            if frame.len() < 13 {
                return Err(());
            }
            let Some(nonce) = request_nonce else {
                return Err(());
            };
            let mut got = [0u8; 8];
            got.copy_from_slice(&frame[5..13]);
            if u64::from_le_bytes(got) != nonce {
                return Err(());
            }
            Ok(())
        }
        _ => Err(()),
    }
}

#[must_use]
pub(crate) fn is_mutating_request(request: &sfp::Request<'_>) -> bool {
    matches!(request, sfp::Request::Put { .. } | sfp::Request::Delete { .. })
}

#[must_use]
pub(crate) fn request_op(request: &sfp::Request<'_>) -> u8 {
    match request {
        sfp::Request::Put { .. } => sfp::OP_PUT,
        sfp::Request::Get { .. } => sfp::OP_GET,
        sfp::Request::Delete { .. } => sfp::OP_DEL,
        sfp::Request::List { .. } => sfp::OP_LIST,
        sfp::Request::Sync => sfp::OP_SYNC,
        sfp::Request::Reopen => sfp::OP_REOPEN,
    }
}

#[must_use]
pub(crate) fn op_from_frame(frame: &[u8]) -> Option<u8> {
    if frame.len() < 4 || frame[0] != sfp::MAGIC0 || frame[1] != sfp::MAGIC1 {
        return None;
    }
    if frame[2] != sfp::VERSION && frame[2] != sfp::VERSION_V2 {
        return None;
    }
    Some(frame[3])
}

#[must_use]
pub(crate) fn request_nonce_from_frame(frame: &[u8]) -> Option<u64> {
    match sfp::decode_request_with_nonce(frame) {
        Ok((_, nonce)) => nonce,
        Err(_) => None,
    }
}

#[must_use]
pub(crate) fn reject_label_for_request(op: u8, reason: RejectReason) -> Option<&'static str> {
    if op == sfp::OP_PUT {
        return Some(match reason {
            RejectReason::BadRequest => "dsoftbusd: audit remote statefs put reject bad_request",
            RejectReason::Unauthenticated => {
                "dsoftbusd: audit remote statefs put reject unauthenticated"
            }
            RejectReason::WriteOutsideAcl => {
                "dsoftbusd: audit remote statefs put reject outside_acl"
            }
            RejectReason::PrefixEscape => {
                "dsoftbusd: audit remote statefs put reject prefix_escape"
            }
            RejectReason::Oversized => "dsoftbusd: audit remote statefs put reject oversized",
        });
    }
    if op == sfp::OP_DEL {
        return Some(match reason {
            RejectReason::BadRequest => "dsoftbusd: audit remote statefs delete reject bad_request",
            RejectReason::Unauthenticated => {
                "dsoftbusd: audit remote statefs delete reject unauthenticated"
            }
            RejectReason::WriteOutsideAcl => {
                "dsoftbusd: audit remote statefs delete reject outside_acl"
            }
            RejectReason::PrefixEscape => {
                "dsoftbusd: audit remote statefs delete reject prefix_escape"
            }
            RejectReason::Oversized => "dsoftbusd: audit remote statefs delete reject oversized",
        });
    }
    None
}

#[must_use]
pub(crate) fn audit_label_for_status(op: u8, status: u8) -> Option<&'static str> {
    if op == sfp::OP_PUT {
        return Some(if status == sfp::STATUS_OK {
            "dsoftbusd: audit remote statefs put allow"
        } else {
            "dsoftbusd: audit remote statefs put reject status"
        });
    }
    if op == sfp::OP_DEL {
        return Some(if status == sfp::STATUS_OK {
            "dsoftbusd: audit remote statefs delete allow"
        } else {
            "dsoftbusd: audit remote statefs delete reject status"
        });
    }
    None
}

fn validate_request_bounds_and_acl(
    request: &sfp::Request<'_>,
) -> core::result::Result<(), RejectReason> {
    match request {
        sfp::Request::Put { key, value } => {
            validate_key(key, true)?;
            if value.is_empty() || value.len() > RS_MAX_VALUE_LEN {
                return Err(RejectReason::Oversized);
            }
        }
        sfp::Request::Get { key } | sfp::Request::Delete { key } => {
            validate_key(key, true)?;
        }
        sfp::Request::List { prefix, limit } => {
            validate_key(prefix, false)?;
            if *limit == 0 || *limit > RS_MAX_LIST_LIMIT {
                return Err(RejectReason::Oversized);
            }
        }
        sfp::Request::Sync => {}
        sfp::Request::Reopen => return Err(RejectReason::BadRequest),
    }
    Ok(())
}

fn validate_key(key: &str, require_leaf: bool) -> core::result::Result<(), RejectReason> {
    if key.is_empty() || key.len() > RS_MAX_KEY_LEN {
        return Err(RejectReason::Oversized);
    }
    if contains_prefix_escape(key) {
        return Err(RejectReason::PrefixEscape);
    }
    if !key.starts_with(RS_SHARED_PREFIX) {
        return Err(RejectReason::WriteOutsideAcl);
    }
    if require_leaf && key.len() == RS_SHARED_PREFIX.len() {
        return Err(RejectReason::WriteOutsideAcl);
    }
    Ok(())
}

fn contains_prefix_escape(path: &str) -> bool {
    if path.bytes().any(|b| b == 0 || b == b'\\') {
        return true;
    }
    if path.contains("//")
        || path.contains("/./")
        || path.contains("/../")
        || path.ends_with("/.")
        || path.ends_with("/..")
    {
        return true;
    }
    false
}
