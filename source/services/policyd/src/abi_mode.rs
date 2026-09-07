// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0091 §6 `OP_SET_ABI_MODE` — the ONE runtime policy
//! transition. Authenticated by the kernel-attributed sender holding the
//! `policy.abi_mode` capability (deny-by-default, granted in
//! `policies/base.toml`; proof boots: `selftest-client`, production: the
//! `nx policy` device channel once TASK-0229 lands) — a privileged proxy
//! does NOT bypass it. Epoch-guarded: the request must name the subject's
//! current profile epoch (`STATUS_STALE` otherwise). Applied through
//! `EvalHost::set_mode` (never persisted; every boot starts in Enforce);
//! a full mode table fails closed (`STATUS_UNSUPPORTED`, mode unchanged).
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below (`test_reject_unauthenticated_mode_switch`,
//!   `test_reject_stale_mode_switch_epoch`); QEMU `SELFTEST: abi mode switch
//!   auth ok` / `SELFTEST: abi stale epoch reject ok`
//! RFC: docs/rfcs/RFC-0091-policy-profile-v2-schema-wire-argument-matchers.md

use nexus_abi::policyd::{
    decode_abi_learn_stats_v2, decode_set_abi_mode_v2, encode_abi_learn_stats_rsp_v2,
    OP_ABI_LEARN_STATS, OP_SET_ABI_MODE, STATUS_ALLOW, STATUS_DENY, STATUS_MALFORMED, STATUS_STALE,
    STATUS_UNSUPPORTED,
};
use nexus_sel::Policy;

use crate::abi_learn::{AbiMode, EvalHost};
use crate::lite_protocol::{normalize_subject_id, rsp_v2, FrameOut};

/// Capability a mode-switch authority must hold.
pub const CAP_ABI_MODE: &str = "policy.abi_mode";

/// Handles one `OP_SET_ABI_MODE` v2 frame.
pub fn handle_set_abi_mode(
    policy: &Policy<'_>,
    frame: &[u8],
    sender_service_id: u64,
    host: &mut dyn EvalHost,
) -> FrameOut {
    let Some((nonce, subject_id, mode, epoch)) = decode_set_abi_mode_v2(frame) else {
        return rsp_v2(OP_SET_ABI_MODE, 0, STATUS_MALFORMED);
    };
    let Some(mode) = AbiMode::from_u8(mode) else {
        return rsp_v2(OP_SET_ABI_MODE, nonce, STATUS_MALFORMED);
    };
    // Authentication: the kernel-attributed sender, never the payload.
    let authority = normalize_subject_id(sender_service_id);
    if !policy.allows(authority, CAP_ABI_MODE) {
        return rsp_v2(OP_SET_ABI_MODE, nonce, STATUS_DENY);
    }
    let subject_id = normalize_subject_id(subject_id);
    if epoch != crate::abi_profile::subject_epoch(subject_id) {
        return rsp_v2(OP_SET_ABI_MODE, nonce, STATUS_STALE);
    }
    if !host.set_mode(subject_id, mode) {
        return rsp_v2(OP_SET_ABI_MODE, nonce, STATUS_UNSUPPORTED);
    }
    rsp_v2(OP_SET_ABI_MODE, nonce, STATUS_ALLOW)
}

/// Handles one `OP_ABI_LEARN_STATS` v2 frame: the subject's mode and the
/// collector's counters, for a mode-switch authority only (same
/// `policy.abi_mode` gate — the counters are observability of the switch).
pub fn handle_learn_stats(
    policy: &Policy<'_>,
    frame: &[u8],
    sender_service_id: u64,
    host: &mut dyn EvalHost,
) -> FrameOut {
    let Some((nonce, subject_id)) = decode_abi_learn_stats_v2(frame) else {
        return rsp_v2(OP_ABI_LEARN_STATS, 0, STATUS_MALFORMED);
    };
    if !policy.allows(normalize_subject_id(sender_service_id), CAP_ABI_MODE) {
        return rsp_v2(OP_ABI_LEARN_STATS, nonce, STATUS_DENY);
    }
    let subject_id = normalize_subject_id(subject_id);
    let state = host.learn_state();
    let (admitted, emitted, dropped) = (state.admitted(), state.emitted(), state.dropped());
    let mode = host.mode_of(subject_id) as u8;
    let mut out = FrameOut { buf: [0u8; crate::lite_protocol::MAX_FRAME_BYTES], len: 0 };
    match encode_abi_learn_stats_rsp_v2(
        nonce,
        STATUS_ALLOW,
        mode,
        admitted,
        emitted,
        dropped,
        &mut out.buf,
    ) {
        Some(n) => {
            out.len = n;
            out
        }
        None => rsp_v2(OP_ABI_LEARN_STATS, nonce, STATUS_UNSUPPORTED),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi_learn::{LearnState, ModeTable};
    use nexus_abi::policyd::{
        decode_rsp_v2, encode_set_abi_mode_v2, ABI_MODE_ENFORCE, ABI_MODE_LEARN,
    };
    use nexus_sel::PolicyEntry;

    struct TableHost(ModeTable, LearnState);

    impl EvalHost for TableHost {
        fn now_ns(&self) -> u64 {
            0
        }
        fn mode_of(&self, subject: u64) -> AbiMode {
            self.0.mode_of(subject)
        }
        fn learn_state(&self) -> &LearnState {
            &self.1
        }
        fn emit_learn(&mut self, _record: &[u8]) -> bool {
            true
        }
        fn set_mode(&mut self, subject: u64, mode: AbiMode) -> bool {
            self.0.set_mode(subject, mode)
        }
    }

    fn selftest() -> u64 {
        nexus_abi::service_id_from_name(b"selftest-client")
    }

    fn switch(
        host: &mut dyn EvalHost,
        policy: &Policy<'_>,
        sender: u64,
        subject: u64,
        mode: u8,
        epoch: u32,
    ) -> u8 {
        let mut req = [0u8; 32];
        let n = encode_set_abi_mode_v2(0x77, subject, mode, epoch, &mut req).unwrap();
        let out = handle_set_abi_mode(policy, &req[..n], sender, host);
        let (op, nonce, status) = decode_rsp_v2(out.as_slice()).unwrap();
        assert_eq!((op, nonce), (OP_SET_ABI_MODE, 0x77));
        status
    }

    fn authority_policy() -> Policy<'static> {
        let entries: &'static [PolicyEntry] = Box::leak(Box::new([PolicyEntry {
            service_id: selftest(),
            capabilities: &["ipc.core", CAP_ABI_MODE],
        }]));
        Policy::new(entries)
    }

    #[test]
    fn test_reject_unauthenticated_mode_switch() {
        let policy = authority_policy();
        let mut host = TableHost(ModeTable::new(), LearnState::new());
        let s = selftest();
        let other = nexus_abi::service_id_from_name(b"demo.testsvc");
        let epoch = crate::abi_profile::subject_epoch(s);
        // A sender without `policy.abi_mode` is denied — for itself and for anyone else.
        assert_eq!(switch(&mut host, &policy, other, other, ABI_MODE_LEARN, 0), STATUS_DENY);
        assert_eq!(switch(&mut host, &policy, other, s, ABI_MODE_LEARN, epoch), STATUS_DENY);
        assert_eq!(host.mode_of(s), AbiMode::Enforce);
        assert_eq!(host.mode_of(other), AbiMode::Enforce);
        // With an empty policy nobody is an authority (deny by default).
        let empty = Policy::new(&[]);
        assert_eq!(switch(&mut host, &empty, s, s, ABI_MODE_LEARN, epoch), STATUS_DENY);
        // Unknown mode byte / malformed frame fail closed.
        assert_eq!(switch(&mut host, &policy, s, s, 7, epoch), STATUS_MALFORMED);
        let out = handle_set_abi_mode(&policy, &[b'P', b'O', 2, OP_SET_ABI_MODE, 1], s, &mut host);
        assert_eq!(decode_rsp_v2(out.as_slice()).unwrap().2, STATUS_MALFORMED);
        assert_eq!(host.mode_of(s), AbiMode::Enforce);
    }

    #[test]
    fn test_reject_stale_mode_switch_epoch() {
        let policy = authority_policy();
        let mut host = TableHost(ModeTable::new(), LearnState::new());
        let s = selftest();
        let epoch = crate::abi_profile::subject_epoch(s);
        assert!(epoch >= 1, "policies/base.toml authors an epoch for the selftest");
        assert_eq!(switch(&mut host, &policy, s, s, ABI_MODE_LEARN, epoch - 1), STATUS_STALE);
        assert_eq!(switch(&mut host, &policy, s, s, ABI_MODE_LEARN, epoch + 1), STATUS_STALE);
        assert_eq!(host.mode_of(s), AbiMode::Enforce);
        // An un-profiled subject's epoch is 0.
        let other = nexus_abi::service_id_from_name(b"demo.testsvc");
        assert_eq!(switch(&mut host, &policy, s, other, ABI_MODE_LEARN, 1), STATUS_STALE);
        assert_eq!(switch(&mut host, &policy, s, other, ABI_MODE_LEARN, 0), STATUS_ALLOW);
    }

    #[test]
    fn learn_stats_are_authority_only_and_count_admissions() {
        use nexus_abi::policyd::{decode_abi_learn_stats_rsp_v2, encode_abi_learn_stats_v2};
        let policy = authority_policy();
        let mut host = TableHost(ModeTable::new(), LearnState::new());
        let s = selftest();
        let other = nexus_abi::service_id_from_name(b"demo.testsvc");
        let mut req = [0u8; 32];
        let n = encode_abi_learn_stats_v2(5, s, &mut req).unwrap();
        // Not an authority ⇒ DENY (generic status reply).
        let out = handle_learn_stats(&policy, &req[..n], other, &mut host);
        assert_eq!(decode_rsp_v2(out.as_slice()).unwrap().2, STATUS_DENY);
        // Authority ⇒ mode + counters; an admission shows up as admitted+1.
        host.1.offer(0x1234, 0);
        assert!(host.0.set_mode(s, AbiMode::Learn));
        let out = handle_learn_stats(&policy, &req[..n], s, &mut host);
        let (nonce, status, mode, admitted, emitted, dropped) =
            decode_abi_learn_stats_rsp_v2(out.as_slice()).unwrap();
        assert_eq!((nonce, status, mode), (5, STATUS_ALLOW, ABI_MODE_LEARN));
        assert_eq!((admitted, emitted, dropped), (1, 0, 0));
        // Malformed frame fails closed.
        let out = handle_learn_stats(&policy, &[b'P', b'O', 2, OP_ABI_LEARN_STATS], s, &mut host);
        assert_eq!(decode_rsp_v2(out.as_slice()).unwrap().2, STATUS_MALFORMED);
    }

    #[test]
    fn authenticated_switch_applies_and_reverts() {
        let policy = authority_policy();
        let mut host = TableHost(ModeTable::new(), LearnState::new());
        let s = selftest();
        let epoch = crate::abi_profile::subject_epoch(s);
        assert_eq!(switch(&mut host, &policy, s, s, ABI_MODE_LEARN, epoch), STATUS_ALLOW);
        assert_eq!(host.mode_of(s), AbiMode::Learn);
        assert_eq!(switch(&mut host, &policy, s, s, ABI_MODE_ENFORCE, epoch), STATUS_ALLOW);
        assert_eq!(host.mode_of(s), AbiMode::Enforce);
        // The side-effect-free host cannot switch: fail closed, mode unchanged.
        let mut none = crate::abi_learn::EnforceOnlyHost;
        assert_eq!(switch(&mut none, &policy, s, s, ABI_MODE_LEARN, epoch), STATUS_UNSUPPORTED);
    }
}
