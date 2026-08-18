// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: statefsd record-encryption core (TASK-0027) — the enrolled-
//!   prefix table, the admin-prefix capability rule, record-key derivation
//!   from the persisted device seed, and the mount/enable self-check that
//!   gates the `statefsd: encryption on` marker (no-fake-green: the marker
//!   only appears after real AEAD seal/open/tamper-reject executed in this
//!   process). cfg-free, shared by os_lite glue and host contract tests —
//!   exactly like `hardening.rs` and `txn.rs`.
//! OWNERS: @runtime
//! STATUS: Functional (host-tested; wired into os_lite via enc_os.rs)
//! API_STABILITY: Unstable (service-internal)
//! TEST_COVERAGE: tests/enc_contract.rs (context build, self-check, cap
//!   table, meta gate)
//! ADR: docs/adr/0023-statefs-persistence-architecture.md

extern crate alloc;

use ed25519_dalek::Signer;
use statefs::enc::{self, EncContext, MAX_LABEL_LEN, SALT_LEN};
use statefs::StatefsError;

/// Enrolled prefix → class table (v2b). Boot-critical prefixes are
/// structurally unenrollable (`EncContext::enroll` rejects them), so this
/// table can only ever name non-boot-critical state.
pub const ENCRYPTED_PREFIXES: &[(&str, &str)] = &[("/state/app/", "app")];

/// Capability gating writes under the statefsd admin prefix — the
/// encryption enablement switch (`statefs::enc::META_KEY`) lives there.
/// Ordinary `statefs.write` holders must NOT be able to toggle it.
pub const CAP_ADMIN: &str = "statefs.admin";

/// The admin prefix itself (per-key cap table row).
pub const ADMIN_PREFIX: &str = "/state/statefsd/";

/// True if `key` is governed by the admin capability.
#[must_use]
pub fn is_admin_key(key: &str) -> bool {
    key.starts_with(ADMIN_PREFIX)
}

/// Extract the raw 32-byte device seed from a stored device-key record
/// (Integrity envelope since TASK-0025 step 4; migration-era journals hold
/// the raw seed — `open_stored` handles both deterministically).
#[must_use]
pub fn device_seed_from_stored(bytes: &[u8]) -> Option<[u8; 32]> {
    let stored = statefs::writer::open_stored(bytes).ok()?;
    let payload = stored.payload();
    if payload.len() != 32 {
        return None;
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(payload);
    Some(seed)
}

/// Build the live encryption context: per class, the record key is the
/// shared HKDF over the deterministic Ed25519 signature of
/// `statefs.record.v1.<class>` (the raw seed is never used as an AEAD key —
/// same discipline as the TASK-0025 envelope MAC key; no keystored oracle
/// involved, statefsd is the only holder of record keys).
pub fn build_context(seed: &[u8; 32], salt: [u8; SALT_LEN]) -> Result<EncContext, StatefsError> {
    let signing_key = ed25519_dalek::SigningKey::from_bytes(seed);
    let mut ctx = EncContext::new(salt);
    let mut added: [Option<(&str, u8)>; enc::MAX_CLASSES] = [None; enc::MAX_CLASSES];
    for (prefix, class) in ENCRYPTED_PREFIXES {
        let idx = match added.iter().flatten().find(|(name, _)| name == class) {
            Some((_, idx)) => *idx,
            None => {
                let mut label = [0u8; MAX_LABEL_LEN];
                let len = enc::record_label(class, &mut label)?;
                let signature = signing_key.sign(&label[..len]);
                let key = enc::record_key_from_ikm(&signature.to_bytes(), class)?;
                let idx = ctx.add_class(class, &key)?;
                if let Some(slot) = added.iter_mut().find(|s| s.is_none()) {
                    *slot = Some((class, idx));
                }
                idx
            }
        };
        ctx.enroll(prefix, idx)?;
    }
    Ok(ctx)
}

/// Mount/enable-time self-check: a real seal → open roundtrip AND a real
/// tamper rejection must both pass before the `encryption on` marker may be
/// emitted. The probe nonce input `(u64::MAX, u32::MAX)` is unreachable by
/// real writes (the id counter saturates below it and chunk indices are
/// bounded by MAX_TXN_KEYS), so no live nonce is ever burned.
#[must_use]
pub fn self_check(ctx: &EncContext) -> bool {
    const PROBE_KEY: &str = "/state/app/__enc_selfcheck__";
    const PROBE_PAYLOAD: &[u8] = b"statefs-enc-selfcheck-v1";
    let Some(class) = ctx.class_for(PROBE_KEY) else {
        return false;
    };
    let mut sealed = alloc::vec::Vec::new();
    if enc::seal_into(ctx, class, PROBE_KEY, u64::MAX, u32::MAX, PROBE_PAYLOAD, &mut sealed)
        .is_err()
    {
        return false;
    }
    match enc::open(ctx, PROBE_KEY, &sealed) {
        Ok(plain) if plain == PROBE_PAYLOAD => {}
        _ => return false,
    }
    let last = sealed.len() - 1;
    sealed[last] ^= 0x01;
    enc::open(ctx, PROBE_KEY, &sealed).is_err()
}
