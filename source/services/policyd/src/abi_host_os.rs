// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the OS-lite `EvalHost` for `OP_ABI_EVAL` (RFC-0091 §5–§7):
//! monotonic clock (`nexus_abi::nsec`), the process-lifetime mode table
//! and learn collector (never persisted — every boot starts in Enforce),
//! and logd (scope `policyd.learn`, the same deterministic append the
//! audit path uses) as the learn sink. `OP_SET_ABI_MODE` writes `ABI_MODES`
//! through `set_mode`; `emit_abi_mode_marker` prints the audited transition.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: host logic in abi_eval.rs / abi_learn.rs tests; QEMU
//!   `SELFTEST: abi learn collected ok` (P3)
//! RFC: docs/rfcs/RFC-0091-policy-profile-v2-schema-wire-argument-matchers.md

use crate::abi_learn::{AbiMode, EvalHost, LearnState, ModeTable, LEARN_SCOPE};
use crate::os_lite::append_logd_deterministic;

static ABI_MODES: ModeTable = ModeTable::new();
static ABI_LEARN: LearnState = LearnState::new();

/// The OS host for `OP_ABI_EVAL`: monotonic clock, the mode table and
/// logd (scope `policyd.learn`) as the learn sink.
pub(crate) struct OsEvalHost;

impl EvalHost for OsEvalHost {
    fn now_ns(&self) -> u64 {
        nexus_abi::nsec().unwrap_or(0)
    }
    fn mode_of(&self, subject: u64) -> AbiMode {
        ABI_MODES.mode_of(subject)
    }
    fn learn_state(&self) -> &LearnState {
        &ABI_LEARN
    }
    fn emit_learn(&mut self, record: &[u8]) -> bool {
        let ok = append_logd_deterministic(LEARN_SCOPE.as_bytes(), record);
        if ok {
            // `policyd: abi learn emitted (class=<c>)` — logd acknowledged the
            // append (the record itself is queried from logd by the selftest).
            emit_learn_echo(record);
        }
        ok
    }
    fn set_mode(&mut self, subject: u64, mode: AbiMode) -> bool {
        ABI_MODES.set_mode(subject, mode)
    }
}

fn emit_learn_echo(record: &[u8]) {
    let class = record
        .windows(7)
        .position(|w| w == b" class=")
        .map(|p| &record[p + 7..])
        .map(|rest| &rest[..rest.iter().position(|b| *b == b' ').unwrap_or(rest.len())])
        .unwrap_or(b"unknown");
    let mut line = [0u8; 64];
    let mut n = 0usize;
    for &b in b"policyd: abi learn emitted (class=".iter().chain(class.iter()).chain(b")".iter()) {
        if n == line.len() {
            break;
        }
        line[n] = b;
        n += 1;
    }
    if let Ok(text) = core::str::from_utf8(&line[..n]) {
        crate::os_lite::emit_line(text);
    }
}

/// `policyd: abi mode subject=<sid hex16> mode=<enforce|learn> epoch=<e>` —
/// the audited transition line (RFC-0091 §6), printed after an applied switch.
pub(crate) fn emit_abi_mode_marker(frame: &[u8]) {
    let Some((_, subject, mode, epoch)) = nexus_abi::policyd::decode_set_abi_mode_v2(frame) else {
        return;
    };
    let mut line = [0u8; 96];
    let mut n = 0usize;
    for &b in b"policyd: abi mode subject=" {
        line[n] = b;
        n += 1;
    }
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for i in 0..16 {
        line[n] = HEX[((subject >> (60 - 4 * i)) & 0xf) as usize];
        n += 1;
    }
    let tail: &[u8] = if mode == nexus_abi::policyd::ABI_MODE_LEARN {
        b" mode=learn epoch="
    } else {
        b" mode=enforce epoch="
    };
    for &b in tail {
        line[n] = b;
        n += 1;
    }
    let mut digits = [0u8; 10];
    let mut d = 0;
    let mut v = epoch;
    loop {
        digits[d] = b'0' + (v % 10) as u8;
        d += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    while d > 0 {
        d -= 1;
        line[n] = digits[d];
        n += 1;
    }
    if let Ok(text) = core::str::from_utf8(&line[..n]) {
        crate::os_lite::emit_line(text);
    }
}
