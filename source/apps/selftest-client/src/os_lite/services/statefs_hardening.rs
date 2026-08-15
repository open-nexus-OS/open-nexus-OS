// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: statefs write-hardening selftest probes (TASK-0025) — derive the
//!   envelope MAC key via keystored's sign oracle, then prove the
//!   Authenticated put/verify round-trip and the tamper/rollback denials
//!   against statefsd's verify-on-put path.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — bringup phase emits
//!   `SELFTEST: statefs auth put ok` / `tamper deny ok` / `rollback deny ok`.
//!
//! KEY DERIVATION (writer side of the v1 contract, see `statefs::derive`):
//!   keystored `OP_DEVICE_SIGN` over `ENVELOPE_KEY_LABEL_V1` (label-scoped,
//!   policy-gated `crypto.derive.statefs` — NOT generic signing power;
//!   deterministic Ed25519 => stable ikm) -> HKDF-SHA256.
//!   statefsd derives the identical key locally from the persisted device
//!   seed, so a valid seal here MUST verify there — no fake green possible:
//!   a broken derivation chain fails the put, not the check.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

extern crate alloc;

use alloc::vec::Vec;

use nexus_ipc::{Client, KernelClient, Wait as IpcWait};
use statefs::envelope::{self, EnvelopeKey, EnvelopeMeta, PolicyClass};
use statefs::protocol as statefs_proto;

use super::super::ipc::routing::route_with_retry;
use super::statefs::statefs_send_recv;

/// Authenticated-mandatory key exercised by the ladder (must stay inside
/// statefsd's `AUTHENTICATED_PREFIXES` table).
const AUTH_KEY: &str = "/state/selftest/secure/token";
const PAYLOAD: &[u8] = b"auth-envelope-v1";

/// Derive the envelope MAC key through keystored's sign oracle.
///
/// Ensures the device key exists first (idempotent keygen), then requests
/// the deterministic signature over the fixed label. Returns None on any
/// deny/transport failure — the caller emits the FAIL markers.
pub(crate) fn derive_envelope_key() -> Option<EnvelopeKey> {
    let client = route_with_retry("keystored").ok()?;
    let wait = IpcWait::Timeout(core::time::Duration::from_millis(2000));

    // 1. Idempotent device keygen (OP=10): OK (0) or KEY_EXISTS (10).
    let req = [b'K', b'S', 1, 10];
    client.send(&req, wait).ok()?;
    let rsp = client.recv(wait).ok()?;
    if rsp.len() < 7 || rsp[0] != b'K' || rsp[1] != b'S' || (rsp[4] != 0 && rsp[4] != 10) {
        return None;
    }

    // 2. Deterministic signature over the derivation label (OP=12).
    //    Request: [K, S, ver, OP, payload_len:u32le, payload...]
    let label = statefs::derive::ENVELOPE_KEY_LABEL_V1;
    let mut req = Vec::with_capacity(8 + label.len());
    req.extend_from_slice(&[b'K', b'S', 1, 12]);
    req.extend_from_slice(&(label.len() as u32).to_le_bytes());
    req.extend_from_slice(label);
    client.send(&req, wait).ok()?;
    let rsp = client.recv(wait).ok()?;
    // Response: [K, S, ver, op|0x80, status, val_len:u16le, sig(64)]
    if rsp.len() < 7 + 64 || rsp[0] != b'K' || rsp[1] != b'S' || rsp[3] != (12 | 0x80) {
        return None;
    }
    if rsp[4] != 0 {
        return None;
    }
    let val_len = u16::from_le_bytes([rsp[5], rsp[6]]) as usize;
    if val_len != 64 {
        return None;
    }
    let signature = &rsp[7..7 + 64];

    // 3. Shared HKDF (identical on the statefsd side).
    statefs::derive::envelope_key_from_ikm(signature).ok()
}

/// Seal `PAYLOAD` for `AUTH_KEY` at `seq` with the derived key.
fn seal_at(key: &EnvelopeKey, seq: u64, ts: u64) -> Option<Vec<u8>> {
    let meta = EnvelopeMeta { subject: "selftest-client", purpose: "hardening", ts };
    envelope::seal(PolicyClass::Authenticated, AUTH_KEY, seq, meta, PAYLOAD, Some(key)).ok()
}

/// Put pre-sealed `value` and return the wire status statefsd answered with.
fn put_sealed(statefsd: &KernelClient, value: &[u8]) -> Option<u8> {
    let frame = statefs_proto::encode_put_request(AUTH_KEY, value).ok()?;
    let rsp = statefs_send_recv(statefsd, &frame).ok()?;
    statefs_proto::decode_status_response(statefs_proto::OP_PUT, &rsp).ok()
}

/// Positive round-trip: authenticated put accepted, get returns the sealed
/// bytes, MAC verifies client-side and the payload matches.
pub(crate) fn statefs_auth_put(
    statefsd: &KernelClient,
    key: &EnvelopeKey,
    base_seq: u64,
) -> core::result::Result<(), ()> {
    let sealed = seal_at(key, base_seq, base_seq).ok_or(())?;
    if put_sealed(statefsd, &sealed) != Some(statefs_proto::STATUS_OK) {
        return Err(());
    }
    let get =
        statefs_proto::encode_key_only_request(statefs_proto::OP_GET, AUTH_KEY).map_err(|_| ())?;
    let rsp = statefs_send_recv(statefsd, &get)?;
    let stored = statefs_proto::decode_get_response(&rsp).map_err(|_| ())?;
    if stored != sealed {
        return Err(());
    }
    // Verified get: decode + MAC check against the derived key (real crypto,
    // not byte comparison alone) + payload equality.
    let env = envelope::decode(&stored).map_err(|_| ())?;
    env.verify_mac(AUTH_KEY, key).map_err(|_| ())?;
    if env.seq != base_seq || env.payload != PAYLOAD {
        return Err(());
    }
    Ok(())
}

/// Negative: a structurally valid envelope with one flipped payload bit must
/// be refused with `STATUS_INTEGRITY_VIOLATION` (9).
pub(crate) fn statefs_tamper_deny(
    statefsd: &KernelClient,
    key: &EnvelopeKey,
    base_seq: u64,
) -> core::result::Result<(), ()> {
    let mut forged = seal_at(key, base_seq + 1, base_seq).ok_or(())?;
    let last = forged.len() - 1;
    forged[last] ^= 0x01; // payload bit-flip: structure intact, MAC broken
    match put_sealed(statefsd, &forged) {
        Some(statefs_proto::STATUS_INTEGRITY_VIOLATION) => Ok(()),
        _ => Err(()),
    }
}

/// Negative: a validly MAC'd envelope with a stale (already seen) seq must
/// be refused with `STATUS_ROLLBACK_DETECTED` (10).
pub(crate) fn statefs_rollback_deny(
    statefsd: &KernelClient,
    key: &EnvelopeKey,
    base_seq: u64,
) -> core::result::Result<(), ()> {
    let stale = seal_at(key, base_seq, base_seq).ok_or(())?;
    match put_sealed(statefsd, &stale) {
        Some(statefs_proto::STATUS_ROLLBACK_DETECTED) => Ok(()),
        _ => Err(()),
    }
}
