// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]
#![forbid(unsafe_code)]

//! CONTEXT: updated's stage/feed request handlers (TASK-0179; split from
//! `os_lite.rs` under the structure ratchet). Owns the path-based staging
//! entry point, the offline-feed ops, and the reject/forensics surface —
//! the apply mechanics live in `apply_os`, the verify+apply core in
//! `updates::component_set`, and this file only turns wire frames into
//! those calls and their outcomes into honest markers.
//! OWNERS: @services-team @security
//! STATUS: Experimental (TASK-0179)
//! API_STABILITY: Internal
//! TEST_COVERAGE: pipeline host-proven in tests/updates_host; the QEMU
//!   `ota-flip` crown lane and the headless deny lanes prove this glue.
//! ADR: docs/rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md

extern crate alloc;

use alloc::vec::Vec;

use updates::Slot;

use crate::bootctl_client;
use crate::os_lite::{
    audit, emit_bytes, emit_line, local_verify, rsp, KeystoredVerifier, UpdatedState, OP_CHECK,
    OP_FEED_LIST, OP_STAGE_SOURCE, STATUS_FAILED, STATUS_MALFORMED, STATUS_OK,
};

/// TASK-0179 (RFC-0089 §8): path-based staging. The container streams
/// from the data volume's `/updates/*.nxs`; every byte is verified against the DEVICE
/// anchor and the stage-time floor before the slot's commit point exists.
pub(crate) fn handle_stage_source(state: &mut UpdatedState, frame: &[u8]) -> Vec<u8> {
    use updates::component_set::RejectReason;

    let path_bytes = match nexus_abi::updated::decode_stage_source_req(frame) {
        Some(bytes) => bytes,
        None => return rsp(OP_STAGE_SOURCE, STATUS_MALFORMED, &[]),
    };
    let Ok(path) = core::str::from_utf8(path_bytes) else {
        return stage_reject(RejectReason::Path);
    };
    if let Err(reason) = crate::apply_os::validate_source_path(path) {
        return stage_reject(reason);
    }
    emit_bytes(b"updated: stage begin (source=");
    emit_bytes(path.as_bytes());
    emit_bytes(b")\n");

    // Authority state: active slot (never writable here) + floor (§10).
    let Some((active, floor)) = bootctl_status_ext() else {
        audit("stage", "fail", Some("bootctld-unreachable"));
        return rsp(OP_STAGE_SOURCE, STATUS_FAILED, &[]);
    };
    let inactive = active.other();

    let source = match crate::apply_os::read_source(path) {
        Ok(source) => source,
        Err(reason) => return stage_reject(reason),
    };
    let mut sink = match crate::apply_os::SlotSink::attach(inactive) {
        Ok(sink) => sink,
        Err(reason) => return stage_reject(reason),
    };

    // Transport witness: the digest of the bytes THIS service received.
    // `nx image fixtures` prints the same 8 bytes for every container it
    // writes, so a mismatch names transport corruption instead of blaming
    // the publisher (that ambiguity cost a full QEMU round trip once).
    {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(source.bytes());
        let digest = hasher.finalize();
        let mut line = [0u8; 64];
        let head = b"updated: source digest ";
        line[..head.len()].copy_from_slice(head);
        let mut len = head.len();
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for byte in digest.iter().take(8) {
            line[len] = HEX[(byte >> 4) as usize];
            line[len + 1] = HEX[(byte & 0xf) as usize];
            len += 2;
        }
        line[len] = b'\n';
        len += 1;
        emit_bytes(&line[..len]);
    }
    let verifier = KeystoredVerifier;
    let mut yield_ticks: u32 = 0;
    let outcome = updates::component_set::verify_and_apply(
        source.bytes(),
        &verifier,
        updates::trust::BAKED_PUBLISHERS,
        floor,
        &mut sink,
        &mut || {
            yield_ticks = yield_ticks.wrapping_add(1);
            if (yield_ticks & 0x3) == 0 {
                let _ = nexus_abi::yield_();
            }
        },
    );
    match outcome {
        Ok(manifest) => {
            emit_bytes(b"updated: component boot-image verified (build=");
            let id8 = build_id8(&manifest.build_id);
            emit_bytes(&id8);
            emit_bytes(b")\n");
            // Record commit: bootctld learns the staged rollback index so
            // the floor can raise on health commit (§10 commit rung).
            let idx = manifest.rollback_index.to_le_bytes();
            match bootctl_client::call_with_args(bootctld::wire::OP_STAGE, &idx) {
                Some(reply) if reply.status == bootctld::wire::STATUS_OK => {
                    state.staged_build = Some(id8);
                    state.staged_slot = Some(inactive);
                    audit("stage", "ok", None);
                    emit_bytes(b"updated: stage done (slot=");
                    emit_bytes(match inactive {
                        Slot::A => b"a",
                        Slot::B => b"b",
                    });
                    emit_bytes(b" build=");
                    emit_bytes(&id8);
                    emit_bytes(b")\n");
                    rsp(OP_STAGE_SOURCE, STATUS_OK, &[])
                }
                Some(_) => {
                    audit("stage", "fail", Some("persist"));
                    rsp(OP_STAGE_SOURCE, STATUS_FAILED, &[])
                }
                None => {
                    audit("stage", "fail", Some("bootctld-unreachable"));
                    rsp(OP_STAGE_SOURCE, STATUS_FAILED, &[])
                }
            }
        }
        Err(reason) => {
            if reason == updates::component_set::RejectReason::Sig {
                emit_verifier_kat();
                emit_sig_forensics(source.bytes());
            }
            stage_reject(reason)
        }
    }
}

/// Known-answer test for the DEVICE's own signature verifier, over the
/// baked anchor. A verifier that always says "invalid" would make every
/// deny lane look green while silently blocking every legitimate update —
/// the failure mode a reject-only test suite can never catch. Runs once
/// per stage reject, so it costs nothing on the happy path.
fn emit_verifier_kat() {
    const KAT_MSG: &[u8] = b"nexus-ota-kat-v1";
    const KAT_SIG: [u8; 64] = [
        0x60, 0x1e, 0xe0, 0x9f, 0xf2, 0x92, 0xe6, 0xa6, 0x0c, 0x02, 0x24, 0xe1, 0x7b, 0x41, 0xc1,
        0xe1, 0x34, 0x4e, 0x50, 0x7a, 0xaf, 0xe6, 0xab, 0x87, 0x7c, 0xaf, 0x3e, 0x02, 0xa7, 0xc9,
        0x08, 0x23, 0xca, 0x87, 0xac, 0x7d, 0xf2, 0x26, 0xbb, 0x03, 0x19, 0x82, 0xb4, 0x8b, 0xe8,
        0x33, 0x21, 0x74, 0x4d, 0xef, 0xb5, 0xf0, 0xbb, 0x93, 0x2e, 0xf6, 0xa6, 0x9e, 0xcb, 0xaa,
        0xc9, 0x10, 0x1a, 0x07,
    ];
    let Some(anchor) = updates::trust::BAKED_PUBLISHERS.first() else {
        emit_line("updated: verifier KAT FAIL (no anchor)");
        return;
    };
    match local_verify(anchor, KAT_MSG, &KAT_SIG) {
        Ok(()) => emit_line("updated: verifier KAT ok (local)"),
        Err(_) => emit_line("updated: verifier KAT FAIL (local rejects a known-good vector)"),
    }
}

/// On a `sig` reject: name the three signing inputs so the next question
/// ("whose bytes changed?") is answerable from the uart alone.
fn emit_sig_forensics(container: &[u8]) {
    let Some((len, manifest4, sig4, hint)) = updates::component_set::describe(container) else {
        emit_line("updated: sig forensics unavailable (container unparseable)");
        return;
    };
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut line = [0u8; 96];
    let mut n = 0usize;
    let head = b"updated: sig inputs msg=";
    line[..head.len()].copy_from_slice(head);
    n += head.len();
    let mut digits = [0u8; 10];
    let mut value = len;
    let mut count = 0usize;
    loop {
        digits[count] = b'0' + (value % 10) as u8;
        count += 1;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    for i in (0..count).rev() {
        line[n] = digits[i];
        n += 1;
    }
    // The ANCHOR is a signing input too — omitting it once cost several
    // QEMU round trips of guessing which side was wrong.
    let anchor4 = updates::trust::BAKED_PUBLISHERS.first().map(|k| [k[0], k[1], k[2], k[3]]);
    let anchor4 = anchor4.unwrap_or([0xff; 4]);
    for (label, bytes) in [
        (&b" m="[..], &manifest4[..]),
        (&b" s="[..], &sig4[..]),
        (&b" k="[..], &hint[..]),
        (&b" a="[..], &anchor4[..]),
    ] {
        line[n..n + label.len()].copy_from_slice(label);
        n += label.len();
        for byte in bytes {
            line[n] = HEX[(byte >> 4) as usize];
            line[n + 1] = HEX[(byte & 0xf) as usize];
            n += 2;
        }
    }
    line[n] = b'/';
    n += 1;
    line[n] = b'0' + (updates::trust::BAKED_PUBLISHERS.len().min(9) as u8);
    n += 1;
    if let Ok(msg) = core::str::from_utf8(&line[..n]) {
        emit_line(msg);
    }
}

/// First 8 chars of the build id, space-padded (marker discipline: fixed
/// width, no allocation).
fn build_id8(build_id: &str) -> [u8; 8] {
    let mut out = [b' '; 8];
    for (slot, byte) in out.iter_mut().zip(build_id.as_bytes()) {
        *slot = *byte;
    }
    out
}

fn stage_reject(reason: updates::component_set::RejectReason) -> Vec<u8> {
    emit_bytes(b"updated: stage rejected (");
    emit_bytes(reason.label().as_bytes());
    emit_bytes(b")\n");
    audit("stage", "fail", Some(reason.label()));
    rsp(OP_STAGE_SOURCE, STATUS_FAILED, &[reject_code(reason)])
}

/// Stable wire code for the reject reason (reply payload byte).
fn reject_code(reason: updates::component_set::RejectReason) -> u8 {
    use updates::component_set::RejectReason as R;
    match reason {
        R::UntrustedPublisher => 1,
        R::Sig => 2,
        R::Digest => 3,
        R::Bounds => 4,
        R::Path => 5,
        R::KindUnsupported => 6,
        R::Downgrade => 7,
        R::Io => 8,
        R::SlotActive => 9,
    }
}

/// Active slot + rollback floor from the authority (GET_STATUS tail).
fn bootctl_status_ext() -> Option<(Slot, u32)> {
    let reply = bootctl_client::call(bootctld::wire::OP_GET_STATUS, None)?;
    if reply.status != bootctld::wire::STATUS_OK || reply.payload_len < 4 {
        return None;
    }
    let active = match reply.payload.first().copied() {
        Some(1) => Slot::A,
        Some(2) => Slot::B,
        _ => return None,
    };
    // Floor rides at [13..17) since TASK-0179 (additive tail after the
    // TASK-0036-B projection fields); absent = 0 (pre-raise default).
    let floor = if reply.payload_len >= 17 {
        u32::from_le_bytes([
            reply.payload[13],
            reply.payload[14],
            reply.payload[15],
            reply.payload[16],
        ])
    } else {
        0
    };
    Some((active, floor))
}

/// Offline feed v1 (RFC-0089 §9): deterministic candidate listing.
pub(crate) fn handle_feed_list(frame: &[u8]) -> Vec<u8> {
    if frame.len() != 4 || frame[3] != OP_FEED_LIST {
        return rsp(OP_FEED_LIST, STATUS_MALFORMED, &[]);
    }
    match crate::apply_os::feed_list() {
        Ok(names) => {
            let mut payload = Vec::with_capacity(256);
            payload.push(names.len().min(255) as u8);
            for name in names.iter().take(8) {
                let bytes = name.as_bytes();
                let len = bytes.len().min(64);
                payload.push(len as u8);
                payload.extend_from_slice(&bytes[..len]);
            }
            audit("feed_list", "ok", None);
            rsp(OP_FEED_LIST, STATUS_OK, &payload)
        }
        Err(reason) => {
            audit("feed_list", "fail", Some(reason.label()));
            rsp(OP_FEED_LIST, STATUS_FAILED, &[reject_code(reason)])
        }
    }
}

pub(crate) fn handle_check(frame: &[u8]) -> Vec<u8> {
    if frame.len() != 4 || frame[3] != OP_CHECK {
        return rsp(OP_CHECK, STATUS_MALFORMED, &[]);
    }
    match crate::apply_os::feed_list() {
        Ok(names) => {
            let count = names.len().min(255) as u8;
            audit("check", "ok", None);
            rsp(OP_CHECK, STATUS_OK, &[count, u8::from(count > 0)])
        }
        Err(reason) => {
            audit("check", "fail", Some(reason.label()));
            rsp(OP_CHECK, STATUS_FAILED, &[reject_code(reason)])
        }
    }
}
