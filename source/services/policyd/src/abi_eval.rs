// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0091 §7 `OP_ABI_EVAL` — an enforcement seam (statefsd,
//! netstackd) asks policyd to evaluate ONE governed argument tuple for a
//! subject. policyd owns the profile (build-time table), the mode table
//! and the learn collector, so the decision, the `limits` check and the
//! learn emission happen in one place. Identity: a seam (init-privileged
//! or a holder of `policy.delegate` — checked by the dispatcher) names the
//! subject it serves; any other sender may only evaluate itself
//! (`test_reject_eval_subject_spoof`). The reply
//! is the generic v2 status; in `Learn` mode the decision is unchanged
//! and a would-deny/limit evaluation additionally emits a bounded learn
//! record through `EvalHost` (never blocking, never failing the seam).
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below; abi_learn_roundtrip_tests.rs
//! RFC: docs/rfcs/RFC-0091-policy-profile-v2-schema-wire-argument-matchers.md

use nexus_abi::abi_filter::{AbiProfile, AddrClass, RuleAction, SyscallClass};
use nexus_abi::policyd::{
    decode_abi_eval_v2, OP_ABI_EVAL, STATUS_ALLOW, STATUS_DENY, STATUS_MALFORMED,
    STATUS_UNSUPPORTED,
};

use crate::abi_learn::{learn_key, AbiMode, Admission, EvalHost};
use crate::learn_record::{
    statefs_learn_prefix, LearnAddr, LearnArg, LearnRecord, Would, MAX_LEARN_RECORD_BYTES,
};
use crate::lite_protocol::{rsp_v2, FrameOut};

/// Decision + why, for audit lines and learn records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Accepting allow rule, limits honoured.
    Allow,
    /// No accepting allow rule (or deny won).
    Deny,
    /// Allow matched but a `limits` ceiling was exceeded.
    Limit,
}

/// One decoded evaluation request.
#[derive(Clone, Copy, Debug)]
pub struct EvalRequest<'a> {
    /// Governed subject (already identity-bound by the caller).
    pub subject_id: u64,
    /// Class.
    pub class: SyscallClass,
    /// `net.bind` address class.
    pub addr_class: AddrClass,
    /// Port (`net.bind`, `net.connect`).
    pub port: u16,
    /// IPv4 destination (`net.connect`).
    pub addr: [u8; 4],
    /// `statefs.put` payload size.
    pub payload_len: u32,
    /// Requested deadline (0 = none).
    pub deadline_ms: u32,
    /// Canonical statefs path (`statefs.put`).
    pub path: &'a [u8],
}

/// Pure evaluation against a profile (the matcher decides; this only
/// names the reason).
pub fn evaluate(profile: &AbiProfile, req: &EvalRequest<'_>) -> Verdict {
    if req.deadline_ms != 0 && profile.check_deadline(req.deadline_ms) == RuleAction::Deny {
        return Verdict::Limit;
    }
    match req.class {
        SyscallClass::StatefsPut => {
            match profile.check_statefs_put(req.path, req.payload_len as usize) {
                RuleAction::Allow => Verdict::Allow,
                RuleAction::Deny if profile.check_statefs_put(req.path, 0) == RuleAction::Allow => {
                    Verdict::Limit
                }
                RuleAction::Deny => Verdict::Deny,
            }
        }
        SyscallClass::NetBind => match profile.check_net_bind(req.port, req.addr_class) {
            RuleAction::Allow => Verdict::Allow,
            RuleAction::Deny => Verdict::Deny,
        },
        SyscallClass::NetConnect => match profile.check_net_connect(req.addr, req.port) {
            RuleAction::Allow => Verdict::Allow,
            RuleAction::Deny => Verdict::Deny,
        },
    }
}

/// Emits the learn record for a refused evaluation (Learn mode only).
/// Bounded by the collector; a failed delivery is counted, never raised.
pub fn learn(
    profile: &AbiProfile,
    req: &EvalRequest<'_>,
    verdict: Verdict,
    host: &mut dyn EvalHost,
) {
    let would = match verdict {
        Verdict::Allow => return,
        Verdict::Deny => Would::Deny,
        Verdict::Limit => Would::Limit,
    };
    if host.mode_of(req.subject_id) != AbiMode::Learn {
        return;
    }
    let arg = match req.class {
        SyscallClass::StatefsPut => match statefs_learn_prefix(req.path) {
            Some(p) => LearnArg::Statefs(p),
            None => {
                host.learn_state().note_emit_failed();
                return;
            }
        },
        SyscallClass::NetBind => LearnArg::NetBind(
            req.port,
            match req.addr_class {
                AddrClass::Loopback => LearnAddr::Loopback,
                AddrClass::Any => LearnAddr::Any,
            },
        ),
        SyscallClass::NetConnect => LearnArg::NetConnect(req.addr, req.port),
    };
    let record = LearnRecord { epoch: profile.epoch(), subject: req.subject_id, arg, would };
    let mut line = [0u8; MAX_LEARN_RECORD_BYTES];
    let Some(n) = record.write(&mut line) else {
        host.learn_state().note_emit_failed();
        return;
    };
    // Key = subject + class + argument text (the `arg=` token), so the same
    // refusal is learned once per boot regardless of `would`.
    let arg_start = line[..n].windows(5).position(|w| w == b" arg=").map_or(0, |p| p + 5);
    let arg_end = line[..n].windows(7).rposition(|w| w == b" would=").unwrap_or(n);
    let key = learn_key(req.subject_id, req.class as u8, &line[arg_start..arg_end]);
    match host.learn_state().offer(key, host.now_ns()) {
        Admission::Admit => {
            if host.emit_learn(&line[..n]) {
                host.learn_state().note_emitted();
            } else {
                host.learn_state().note_emit_failed();
            }
        }
        Admission::Duplicate | Admission::RateLimited => {}
    }
}

/// Handles one `OP_ABI_EVAL` v2 frame.
pub fn handle_abi_eval(
    frame: &[u8],
    sender_service_id: u64,
    privileged_proxy: bool,
    host: &mut dyn EvalHost,
) -> FrameOut {
    let Some((nonce, subject_id, class, addr_class, port, addr_be, payload_len, deadline_ms, path)) =
        decode_abi_eval_v2(frame)
    else {
        return rsp_v2(OP_ABI_EVAL, 0, STATUS_MALFORMED);
    };
    let subject_id = crate::lite_protocol::normalize_subject_id(subject_id);
    if !privileged_proxy
        && subject_id != crate::lite_protocol::normalize_subject_id(sender_service_id)
    {
        return rsp_v2(OP_ABI_EVAL, nonce, STATUS_DENY);
    }
    let Some(class) = SyscallClass::from_u8(class) else {
        return rsp_v2(OP_ABI_EVAL, nonce, STATUS_MALFORMED);
    };
    let Some(addr_class) = AddrClass::from_u8(addr_class) else {
        return rsp_v2(OP_ABI_EVAL, nonce, STATUS_MALFORMED);
    };
    // Governed = an authored profile. An un-profiled subject is NOT governed
    // by the argument filters yet (capability checks still apply at the seam):
    // `STATUS_UNSUPPORTED` tells the seam so, distinct from a deny. Authoring
    // every statefs writer and flipping this to deny is the tracked follow-up.
    if !crate::abi_profile::is_governed(subject_id) {
        return rsp_v2(OP_ABI_EVAL, nonce, STATUS_UNSUPPORTED);
    }
    let Some(profile) = crate::abi_profile::subject_profile(subject_id) else {
        return rsp_v2(OP_ABI_EVAL, nonce, STATUS_UNSUPPORTED);
    };
    let req = EvalRequest {
        subject_id,
        class,
        addr_class,
        port,
        addr: addr_be.to_be_bytes(),
        payload_len,
        deadline_ms,
        path,
    };
    let verdict = evaluate(&profile, &req);
    learn(&profile, &req, verdict, host);
    rsp_v2(OP_ABI_EVAL, nonce, if verdict == Verdict::Allow { STATUS_ALLOW } else { STATUS_DENY })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi_learn::{LearnState, LEARN_BURST};
    use nexus_abi::policyd::{
        decode_rsp_v2, encode_abi_eval_v2, ABI_CLASS_NET_BIND, ABI_CLASS_NET_CONNECT,
        ABI_CLASS_STATEFS_PUT,
    };

    /// Test host: fixed mode, fake clock, records everything it is asked to emit.
    struct Recorder {
        mode: AbiMode,
        now_ns: u64,
        state: LearnState,
        lines: Vec<String>,
        deliver: bool,
    }

    impl Recorder {
        fn new(mode: AbiMode) -> Self {
            Self { mode, now_ns: 0, state: LearnState::new(), lines: Vec::new(), deliver: true }
        }
    }

    impl EvalHost for Recorder {
        fn now_ns(&self) -> u64 {
            self.now_ns
        }
        fn mode_of(&self, _subject: u64) -> AbiMode {
            self.mode
        }
        fn learn_state(&self) -> &LearnState {
            &self.state
        }
        fn emit_learn(&mut self, record: &[u8]) -> bool {
            if self.deliver {
                self.lines.push(core::str::from_utf8(record).unwrap().to_string());
            }
            self.deliver
        }
        fn set_mode(&mut self, _subject: u64, mode: AbiMode) -> bool {
            self.mode = mode;
            true
        }
    }

    fn selftest() -> u64 {
        nexus_abi::service_id_from_name(b"selftest-client")
    }

    #[allow(clippy::too_many_arguments)]
    fn eval(
        host: &mut dyn EvalHost,
        sender: u64,
        privileged: bool,
        subject: u64,
        class: u8,
        addr_class: u8,
        port: u16,
        addr: [u8; 4],
        payload: u32,
        deadline: u32,
        path: &[u8],
    ) -> u8 {
        let mut req = [0u8; 256];
        let n = encode_abi_eval_v2(
            0x55,
            subject,
            class,
            addr_class,
            port,
            u32::from_be_bytes(addr),
            payload,
            deadline,
            path,
            &mut req,
        )
        .unwrap();
        let out = handle_abi_eval(&req[..n], sender, privileged, host);
        let (op, nonce, status) = decode_rsp_v2(out.as_slice()).unwrap();
        assert_eq!((op, nonce), (OP_ABI_EVAL, 0x55));
        status
    }

    #[test]
    fn enforce_mode_decides_and_never_learns() {
        let mut h = Recorder::new(AbiMode::Enforce);
        let s = selftest();
        let z = [0u8; 4];
        assert_eq!(
            eval(
                &mut h,
                s,
                false,
                s,
                ABI_CLASS_STATEFS_PUT,
                0,
                0,
                z,
                16,
                0,
                b"/state/app/selftest/token"
            ),
            STATUS_ALLOW
        );
        assert_eq!(
            eval(&mut h, s, false, s, ABI_CLASS_STATEFS_PUT, 0, 0, z, 16, 0, b"/state/forbidden/x"),
            STATUS_DENY
        );
        assert_eq!(
            eval(
                &mut h,
                s,
                false,
                s,
                ABI_CLASS_STATEFS_PUT,
                0,
                0,
                z,
                5000,
                0,
                b"/state/app/selftest/big"
            ),
            STATUS_DENY
        );
        assert_eq!(eval(&mut h, s, false, s, ABI_CLASS_NET_BIND, 0, 80, z, 0, 0, b""), STATUS_DENY);
        assert_eq!(
            eval(&mut h, s, false, s, ABI_CLASS_NET_BIND, 0, 1024, z, 0, 0, b""),
            STATUS_ALLOW
        );
        assert_eq!(
            eval(&mut h, s, false, s, ABI_CLASS_NET_BIND, 1, 1024, z, 0, 0, b""),
            STATUS_DENY
        );
        assert_eq!(
            eval(&mut h, s, false, s, ABI_CLASS_NET_CONNECT, 0, 443, [10, 0, 2, 2], 0, 0, b""),
            STATUS_ALLOW
        );
        assert_eq!(
            eval(&mut h, s, false, s, ABI_CLASS_NET_CONNECT, 0, 443, [10, 0, 3, 2], 0, 0, b""),
            STATUS_DENY
        );
        assert_eq!(
            eval(
                &mut h,
                s,
                false,
                s,
                ABI_CLASS_STATEFS_PUT,
                0,
                0,
                z,
                16,
                5000,
                b"/state/app/selftest/token"
            ),
            STATUS_DENY
        );
        assert!(h.lines.is_empty());
        assert_eq!(h.state.emitted(), 0);
    }

    #[test]
    fn learn_mode_emits_bounded_records_without_changing_decisions() {
        let mut h = Recorder::new(AbiMode::Learn);
        let s = selftest();
        let z = [0u8; 4];
        // Same decisions as Enforce…
        assert_eq!(
            eval(&mut h, s, false, s, ABI_CLASS_STATEFS_PUT, 0, 0, z, 16, 0, b"/state/forbidden/x"),
            STATUS_DENY
        );
        assert_eq!(
            eval(
                &mut h,
                s,
                false,
                s,
                ABI_CLASS_STATEFS_PUT,
                0,
                0,
                z,
                16,
                0,
                b"/state/app/selftest/token"
            ),
            STATUS_ALLOW
        );
        assert_eq!(
            eval(
                &mut h,
                s,
                false,
                s,
                ABI_CLASS_STATEFS_PUT,
                0,
                0,
                z,
                5000,
                0,
                b"/state/app/selftest/big"
            ),
            STATUS_DENY
        );
        assert_eq!(
            eval(&mut h, s, false, s, ABI_CLASS_NET_BIND, 1, 8080, z, 0, 0, b""),
            STATUS_DENY
        );
        assert_eq!(
            eval(&mut h, s, false, s, ABI_CLASS_NET_CONNECT, 0, 443, [10, 0, 3, 2], 0, 0, b""),
            STATUS_DENY
        );
        // …plus one record per refused argument, deduplicated.
        assert_eq!(
            eval(&mut h, s, false, s, ABI_CLASS_STATEFS_PUT, 0, 0, z, 16, 0, b"/state/forbidden/y"),
            STATUS_DENY
        );
        let sid = format!("{s:016x}");
        let e = crate::abi_profile::subject_epoch(s);
        assert_eq!(
            h.lines,
            vec![
                format!("abi.learn epoch={e} subject={sid} class=statefs arg=/state/forbidden/ would=deny"),
                format!("abi.learn epoch={e} subject={sid} class=statefs arg=/state/app/selftest/ would=limit"),
                format!("abi.learn epoch={e} subject={sid} class=net.bind arg=8080/any would=deny"),
                format!("abi.learn epoch={e} subject={sid} class=net.connect arg=10.0.3.2:443 would=deny"),
            ]
        );
        assert!(h.lines.iter().all(|l| l.len() <= MAX_LEARN_RECORD_BYTES));
        assert_eq!((h.state.emitted(), h.state.dropped()), (4, 0));
        // Every record parses back through the shared reader.
        for l in &h.lines {
            assert!(crate::learn_record::parse_learn_record(l).is_some());
        }
    }

    #[test]
    fn learn_emission_is_rate_limited_and_counts_drops() {
        let mut h = Recorder::new(AbiMode::Learn);
        let s = selftest();
        let z = [0u8; 4];
        for port in 1..=(LEARN_BURST as u16 + 10) {
            assert_eq!(
                eval(&mut h, s, false, s, ABI_CLASS_NET_BIND, 0, port, z, 0, 0, b""),
                STATUS_DENY
            );
        }
        assert_eq!(h.lines.len(), LEARN_BURST as usize);
        assert_eq!(h.state.dropped(), 10);
        // logd down: the decision still returns, the drop is counted.
        h.deliver = false;
        h.now_ns = 2_000_000_000;
        assert_eq!(
            eval(&mut h, s, false, s, ABI_CLASS_NET_BIND, 0, 600, z, 0, 0, b""),
            STATUS_DENY
        );
        assert_eq!(h.state.dropped(), 11);
    }

    #[test]
    fn ungoverned_subject_is_unsupported_not_denied() {
        let mut h = Recorder::new(AbiMode::Learn);
        let other = nexus_abi::service_id_from_name(b"demo.testsvc");
        let z = [0u8; 4];
        assert_eq!(
            eval(&mut h, other, false, other, ABI_CLASS_STATEFS_PUT, 0, 0, z, 16, 0, b"/state/x"),
            STATUS_UNSUPPORTED
        );
        assert!(h.lines.is_empty(), "nothing to learn without a profile");
    }

    #[test]
    fn test_reject_eval_subject_spoof() {
        let mut h = Recorder::new(AbiMode::Learn);
        let s = selftest();
        let other = nexus_abi::service_id_from_name(b"demo.testsvc");
        let z = [0u8; 4];
        // A non-privileged sender may not evaluate another subject's profile…
        assert_eq!(
            eval(
                &mut h,
                other,
                false,
                s,
                ABI_CLASS_STATEFS_PUT,
                0,
                0,
                z,
                16,
                0,
                b"/state/app/selftest/token"
            ),
            STATUS_DENY
        );
        assert!(h.lines.is_empty());
        // …a privileged proxy (the seam) may.
        assert_eq!(
            eval(
                &mut h,
                other,
                true,
                s,
                ABI_CLASS_STATEFS_PUT,
                0,
                0,
                z,
                16,
                0,
                b"/state/app/selftest/token"
            ),
            STATUS_ALLOW
        );
        // Unknown class / address class / malformed frame fail closed.
        assert_eq!(eval(&mut h, s, false, s, 9, 0, 0, z, 0, 0, b""), STATUS_MALFORMED);
        assert_eq!(
            eval(&mut h, s, false, s, ABI_CLASS_NET_BIND, 7, 80, z, 0, 0, b""),
            STATUS_MALFORMED
        );
        let out = handle_abi_eval(&[b'P', b'O', 2, OP_ABI_EVAL, 1], s, false, &mut h);
        assert_eq!(decode_rsp_v2(out.as_slice()).unwrap().2, STATUS_MALFORMED);
        // Non-canonical path is denied before any rule and learned as its directory.
        assert_eq!(
            eval(
                &mut h,
                s,
                false,
                s,
                ABI_CLASS_STATEFS_PUT,
                0,
                0,
                z,
                16,
                0,
                b"/state/app/selftest/../x"
            ),
            STATUS_DENY
        );
    }
}
