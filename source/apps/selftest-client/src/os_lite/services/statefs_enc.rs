// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: statefs record-encryption probe (TASK-0027) — enable the
//!   opt-in mode end to end over the wire (salt from rngd, admin meta put
//!   under the `statefs.admin` cap), then prove a plaintext put/get
//!   roundtrip through an enrolled prefix BOTH before and after
//!   Sync+Reopen. The Reopen replays the sealed records under statefsd's
//!   installed context, so the AEAD-verified replay path runs in-VM.
//!   Tamper-at-rest negatives are host proofs (the wire carries plaintext;
//!   the disk is the adversary) — the `encryption on` marker itself is
//!   gated by statefsd's in-process seal/open/tamper self-check.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — bringup phase.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

extern crate alloc;

use alloc::vec::Vec;

use nexus_abi::yield_;
use nexus_ipc::{Client, KernelClient, Wait as IpcWait};
use statefs::protocol as statefs_proto;
use statefs::StatefsError;

use super::statefs::statefs_send_recv;
use super::statefs_v2::{get_value, put_ok, sync_and_reopen};

/// Enrolled-prefix probe key (under `/state/app/`, the v2b enrolled table).
const ENC_KEY: &str = "/state/app/selftest/enc/token";
const ENC_VAL: &[u8] = b"enc-roundtrip-plaintext-v1";

/// Fetch `SALT_LEN` bytes of real entropy from rngd (same wire + slots as
/// the rng probes). None = no entropy — the caller must NOT enable
/// encryption (RED rule: never claim secure encryption without a salt
/// provenance).
fn rng_salt() -> Option<[u8; statefs::enc::SALT_LEN]> {
    const RNGD_SEND_SLOT: u32 = 0x1e;
    const RNGD_RECV_SLOT: u32 = 0x1f;
    let nonce = (nexus_abi::nsec().unwrap_or(0) as u32) ^ 0x5E17_ECAF;
    let mut req = Vec::with_capacity(10);
    req.extend_from_slice(&[b'R', b'G', 1, 1]);
    req.extend_from_slice(&nonce.to_le_bytes());
    req.extend_from_slice(&(statefs::enc::SALT_LEN as u16).to_le_bytes());
    let client = KernelClient::new_with_slots(RNGD_SEND_SLOT, RNGD_RECV_SLOT).ok()?;
    client.send(&req, IpcWait::Timeout(core::time::Duration::from_millis(500))).ok()?;
    let deadline = nexus_abi::nsec().unwrap_or(0).saturating_add(500_000_000);
    loop {
        if nexus_abi::nsec().unwrap_or(0) >= deadline {
            return None;
        }
        match client.recv(IpcWait::NonBlocking) {
            Ok(rsp) => {
                if rsp.len() < 9 || rsp[0] != b'R' || rsp[1] != b'G' || rsp[3] != (1 | 0x80) {
                    continue;
                }
                if rsp[4] != 0
                    || u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]) != nonce
                    || rsp.len() != 9 + statefs::enc::SALT_LEN
                {
                    return None;
                }
                let mut salt = [0u8; statefs::enc::SALT_LEN];
                salt.copy_from_slice(&rsp[9..]);
                return Some(salt);
            }
            Err(_) => {
                let _ = yield_();
            }
        }
    }
}

/// Enable record encryption (idempotent) and prove the roundtrip.
///
/// The meta record is written ONLY if absent: the salt is a per-store
/// forever value — overwriting it would orphan every previously sealed
/// nonce (keep-blk second boots arrive here with the mode already on).
pub(crate) fn statefs_enc_roundtrip(client: &KernelClient) -> core::result::Result<(), ()> {
    let meta_get =
        statefs_proto::encode_key_only_request(statefs_proto::OP_GET, statefs::enc::META_KEY)
            .map_err(|_| ())?;
    let rsp = statefs_send_recv(client, &meta_get)?;
    match statefs_proto::decode_get_response(&rsp) {
        Ok(_) => {} // already enabled (preserved image)
        Err(StatefsError::NotFound) => {
            let salt = rng_salt().ok_or(())?;
            let meta = statefs::enc::encode_meta(&statefs::enc::EncMeta { salt });
            // Admin-gated put: flips statefsd's enable path synchronously.
            put_ok(client, statefs::enc::META_KEY, &meta)?;
        }
        Err(_) => return Err(()),
    }
    // Roundtrip through the enrolled prefix: plaintext on the wire, sealed
    // at rest (the harness pairs this with the `encryption on` marker).
    put_ok(client, ENC_KEY, ENC_VAL)?;
    if get_value(client, ENC_KEY)?.as_slice() != ENC_VAL {
        return Err(());
    }
    // Persist + replay: the reopened engine re-replays the sealed records
    // under the installed context and the value still opens.
    sync_and_reopen(client)?;
    if get_value(client, ENC_KEY)?.as_slice() != ENC_VAL {
        return Err(());
    }
    Ok(())
}
