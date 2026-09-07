// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0091 §7 statefsd enforcement seam — after the capability
//! check, every `put` asks policyd (`OP_ABI_EVAL` over the init-wired
//! policyd slots 7/6/5, the same slots the delegated cap check uses) to
//! evaluate `statefs` for (subject, canonical path, payload length).
//! policyd holds the profile, the limits and the learn mode, so this seam
//! carries no policy of its own. Verdicts: ALLOW ⇒ proceed; DENY ⇒
//! `STATUS_ACCESS_DENIED` + audit line; UNSUPPORTED ⇒ the subject has no
//! authored profile (not governed yet — capability-only, tracked follow-up);
//! unreachable / malformed ⇒ fail closed.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: policyd host tests (abi_eval.rs); QEMU
//!   `statefsd: abi deny path=…` + `SELFTEST: abi enforce allow/deny ok`
//! RFC: docs/rfcs/RFC-0091-policy-profile-v2-schema-wire-argument-matchers.md

use nexus_abi::policyd::{ABI_CLASS_STATEFS_PUT, STATUS_ALLOW, STATUS_UNSUPPORTED};

use crate::emit_os::emit_abi_denied;

const POLICYD_SEND_SLOT: u32 = 0x07;
const REPLY_SEND_SLOT: u32 = 0x06;
const REPLY_RECV_SLOT: u32 = 0x05;

/// `true` when the argument filters admit `put(path, payload_len)` for
/// `subject_id` (or the subject is not governed). Emits the audit line on
/// a refusal; an unreachable policyd is a refusal.
pub(crate) fn abi_put_allowed(subject_id: u64, path: &str, payload_len: usize) -> bool {
    let payload = u32::try_from(payload_len).unwrap_or(u32::MAX);
    let status = nexus_ipc::policyd::abi_eval_on(
        POLICYD_SEND_SLOT,
        REPLY_SEND_SLOT,
        REPLY_RECV_SLOT,
        subject_id,
        ABI_CLASS_STATEFS_PUT,
        0,
        0,
        0,
        payload,
        0,
        path.as_bytes(),
    );
    match status {
        Some(STATUS_ALLOW) | Some(STATUS_UNSUPPORTED) => true,
        _ => {
            emit_abi_denied(path, subject_id);
            false
        }
    }
}
