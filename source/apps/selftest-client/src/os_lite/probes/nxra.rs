// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `.nxra` break-glass proof (RFC-0088, TASK-0053), every proof
//! boot. Uses `OP_SWITCH` because the selftest holds NO standing grant
//! there (the OTA gate admits only updated) AND the machine rejects an
//! unstaged switch (`NotStaged`) — so a token-authorized attempt proves
//! verify + consume end to end while staying state-neutral. Sequence:
//! (1) require: no token ⇒ the standing DENY holds; (2) accept: token with
//! `seq = persisted hwm + 1` (read from `/state/boot/nxra.hwm.<keyid8>` —
//! monotone across keep-blk boots, no wall clock needed) ⇒ authorization
//! passes, machine answers `FAILED/NotStaged`; (3) replay: the SAME token
//! again ⇒ DENY (`reason=replay`). The signer is the PROOF key from
//! `policies/nxra-trust.toml` (deterministic seed, proof images only).
//! OWNERS: @security @reliability
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU ladder (headless/full/smp1 + reset boot 3), gated.
//! ADR: docs/rfcs/RFC-0088-nxra-signed-recovery-action-tokens.md

extern crate alloc;

use nexus_ipc::KernelClient;
use statefs::protocol as statefs_proto;

use crate::markers::emit_line;
use crate::os_lite::probes::reset::bootctl_call_raw;
use crate::os_lite::services::statefs::statefs_send_recv;

/// Deterministic proof signing key (public by construction; the baked
/// trust list carries its verifying half for proof images only).
const PROOF_KEY: [u8; 32] = [42u8; 32];
const OP_SWITCH: u8 = 2;
const STATUS_FAILED: u8 = 3;
const STATUS_DENIED: u8 = 4;
/// Machine reject detail for an unstaged switch (bootctld machine_fail).
const REASON_NOT_STAGED: u8 = 1;

/// Runs the three-step break-glass chain; bootctld + statefsd must be up.
pub(crate) fn nxra_proof(statefsd: &KernelClient) {
    // (1) Standing deny holds without a token.
    match bootctl_call_raw(&[b'B', b'T', 1, OP_SWITCH, 1], OP_SWITCH) {
        Some((STATUS_DENIED, _)) => emit_line(crate::markers::M_SELFTEST_NXRA_REQUIRE_OK),
        _ => {
            emit_line(crate::markers::M_SELFTEST_NXRA_REQUIRE_FAIL);
            return;
        }
    }

    // (2) Token with the next sequence: authorization passes, the machine
    // rejects the unstaged switch — state-neutral by construction.
    let seq = persisted_hwm(statefsd).saturating_add(1);
    let token = nxra::sign(nxra::Action::SlotSwitch, seq, None, 1, &PROOF_KEY);
    let mut frame = [0u8; 5 + nxra::TOKEN_LEN];
    frame[..5].copy_from_slice(&[b'B', b'T', 1, OP_SWITCH, 1]);
    frame[5..].copy_from_slice(&token);
    match bootctl_call_raw(&frame, OP_SWITCH) {
        Some((STATUS_FAILED, [REASON_NOT_STAGED, _])) => {
            emit_line(crate::markers::M_SELFTEST_NXRA_ACCEPT_OK)
        }
        _ => {
            emit_line(crate::markers::M_SELFTEST_NXRA_ACCEPT_FAIL);
            return;
        }
    }

    // (3) The SAME token replays into a deny (hwm consumed before act).
    match bootctl_call_raw(&frame, OP_SWITCH) {
        Some((STATUS_DENIED, _)) => emit_line(crate::markers::M_SELFTEST_NXRA_REPLAY_DENY_OK),
        _ => emit_line(crate::markers::M_SELFTEST_NXRA_REPLAY_DENY_FAIL),
    }
}

/// Persisted high-water mark for the proof key (0 when absent) — the
/// monotone-seq source that works without any wall clock.
fn persisted_hwm(statefsd: &KernelClient) -> u64 {
    let pubkey = *ed25519_dalek::SigningKey::from_bytes(&PROOF_KEY).verifying_key().as_bytes();
    let id = nxra::keyid8(&pubkey);
    let mut key = [0u8; 32];
    let prefix = b"/state/boot/nxra.hwm.";
    key[..prefix.len()].copy_from_slice(prefix);
    key[prefix.len()..prefix.len() + 8].copy_from_slice(&id);
    let Ok(key) = core::str::from_utf8(&key[..prefix.len() + 8]) else { return 0 };
    let Ok(req) = statefs_proto::encode_key_only_request(statefs_proto::OP_GET, key) else {
        return 0;
    };
    let Ok(rsp) = statefs_send_recv(statefsd, &req) else { return 0 };
    match statefs_proto::decode_get_response(&rsp) {
        // Integrity-envelope record (bootctld seals it; raw legacy accepted
        // by the same opener).
        Ok(value) => match statefs::writer::open_stored(&value) {
            Ok(stored) if stored.payload().len() == 8 => {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(stored.payload());
                u64::from_le_bytes(buf)
            }
            _ => 0,
        },
        _ => 0,
    }
}
