// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! CONTEXT: `.nxra` break-glass gate on bootctld's mutating ops (RFC-0088,
//! TASK-0053). Runs ONLY after the standing gates denied — additive, never
//! weakening. Pipeline is the shared `nxra` crate (trust = build-time-baked
//! anchor); this module adds the storage seam: the per-key replay
//! high-water mark lives at `/state/boot/nxra.hwm.<keyid8>` (bootctld's
//! existing `statefs.boot` grant) and is persisted BEFORE the action runs
//! (consume-before-act — a persist failure REJECTS, fail closed). Markers
//! carry the stable reject labels verbatim; no trusted wall clock is wired
//! in v1, so windowed tokens reject `no-clock` by design.
//! OWNERS: @security @reliability
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: decision pipeline host-tested (nxra + tests/nxra_host);
//! this seam is QEMU-proven (`bootctld: nxra accept/reject` chain).
//! ADR: docs/rfcs/RFC-0088-nxra-signed-recovery-action-tokens.md

use statefs::client::StatefsClient;

/// Token offset inside mutating op frames (`[B, T, 1, op, arg, token…]`).
pub(crate) const TOKEN_AT: usize = 5;

const HWM_PREFIX: &str = "/state/boot/nxra.hwm.";

/// Outcome of the break-glass attempt for a standing-denied request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gate {
    /// No token bytes present — the caller keeps the standing deny.
    NoToken,
    /// Verified + consumed: the op may execute.
    Authorized,
    /// Token present but refused (reason already marked/audited).
    Rejected,
}

/// Verifies + consumes an inline token authorizing `action(arg)`.
pub(crate) fn authorize(
    client: &StatefsClient,
    frame: &[u8],
    action: nxra::Action,
    arg: u64,
) -> Gate {
    let Some(bytes) = frame.get(TOKEN_AT..TOKEN_AT + nxra::TOKEN_LEN) else {
        return Gate::NoToken;
    };
    // Structural parse first so the hwm lookup has a key; verify() repeats
    // it as part of the full deterministic pipeline.
    let token = match nxra::parse(bytes) {
        Ok(token) => token,
        Err(reject) => {
            emit_reject(reject.label());
            return Gate::Rejected;
        }
    };
    let hwm = read_hwm(client, &token.pubkey);
    match nxra::verify(bytes, nxra::BAKED_TRUST, action, arg, hwm, None) {
        Ok(token) => {
            // Consume-before-act (RFC-0088): the mark persists first; a
            // crash after this burns the token, never replays it.
            if write_hwm(client, &token.pubkey, token.seq).is_err() {
                emit_reject("consume-failed");
                return Gate::Rejected;
            }
            emit_accept(&token);
            Gate::Authorized
        }
        Err(reject) => {
            emit_reject(reject.label());
            Gate::Rejected
        }
    }
}

/// Persisted high-water mark for this verifier + key (Integrity envelope —
/// `/state/boot/` is an integrity-floor prefix, raw values there would
/// only ever ride the migration accept). Absent = 0; an UNREADABLE record
/// poisons to `u64::MAX` (everything rejects as replay — fail closed
/// beats a replay window).
fn read_hwm(client: &StatefsClient, pubkey: &[u8; 32]) -> u64 {
    let (key, len) = hwm_key(pubkey);
    let Ok(key) = core::str::from_utf8(&key[..len]) else { return u64::MAX };
    match client.get(key) {
        Ok(value) => match statefs::writer::open_stored(&value) {
            Ok(stored) if stored.payload().len() == 8 => {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(stored.payload());
                u64::from_le_bytes(buf)
            }
            _ => u64::MAX,
        },
        Err(statefs::StatefsError::NotFound) => 0,
        Err(_) => u64::MAX,
    }
}

fn write_hwm(client: &StatefsClient, pubkey: &[u8; 32], seq: u64) -> Result<(), ()> {
    let (key, len) = hwm_key(pubkey);
    let key = core::str::from_utf8(&key[..len]).map_err(|_| ())?;
    // Envelope seq: reuse the token seq — it is monotone per key by the
    // exact property the hwm enforces, so rollback detection composes.
    let ts = nexus_abi::nsec().unwrap_or(0);
    let sealed = statefs::writer::seal_integrity(
        key,
        seq,
        crate::record::SUBJECT,
        "nxra",
        ts,
        &seq.to_le_bytes(),
    )
    .map_err(|_| ())?;
    client.put(key, &sealed).map_err(|_| ())?;
    client.sync().map_err(|_| ())
}

fn hwm_key(pubkey: &[u8; 32]) -> ([u8; 32], usize) {
    let mut out = [0u8; 32];
    let prefix = HWM_PREFIX.as_bytes();
    out[..prefix.len()].copy_from_slice(prefix);
    out[prefix.len()..prefix.len() + 8].copy_from_slice(&nxra::keyid8(pubkey));
    (out, prefix.len() + 8)
}

fn emit_accept(token: &nxra::Token) {
    let mut line = [0u8; 96];
    let mut len = 0usize;
    for part in [
        b"bootctld: nxra accept (key=".as_slice(),
        &nxra::keyid8(&token.pubkey),
        b" action=",
        token.action.label().as_bytes(),
        b")",
    ] {
        if len + part.len() > line.len() {
            break;
        }
        line[len..len + part.len()].copy_from_slice(part);
        len += part.len();
    }
    if let Ok(msg) = core::str::from_utf8(&line[..len]) {
        let _ = nexus_abi::debug_println(msg);
    }
}

fn emit_reject(reason: &str) {
    let mut line = [0u8; 64];
    let mut len = 0usize;
    for part in ["bootctld: nxra reject (reason=", reason, ")"] {
        let bytes = part.as_bytes();
        if len + bytes.len() > line.len() {
            break;
        }
        line[len..len + bytes.len()].copy_from_slice(bytes);
        len += bytes.len();
    }
    if let Ok(msg) = core::str::from_utf8(&line[..len]) {
        let _ = nexus_abi::debug_println(msg);
    }
}
