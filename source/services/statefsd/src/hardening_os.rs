// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]
#![forbid(unsafe_code)]

//! CONTEXT: statefsd envelope-hardening os-lite glue (TASK-0025) — the
//!   lazy MAC-key derivation from the persisted device-key record and the
//!   replay-time anti-rollback feed. Moved verbatim out of os_lite.rs for
//!   the structure ratchet (behavior unchanged); the cfg-free rules stay
//!   in `hardening.rs`.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable (service-internal)
//! TEST_COVERAGE: QEMU marker ladder; rule cores host-tested via hardening
//! ADR: docs/adr/0023-statefs-persistence-architecture.md

use statefs::envelope::{EnvelopeKey, SeqTracker};
use statefs::JournalEngine;

use crate::emit_os::emit_line;
use crate::hardening;
use crate::os_lite::Backend;

/// Lazily derive (and cache) the envelope MAC key from the persisted
/// device-key record. Returns None until keystored has generated it —
/// Authenticated-class ops fail closed until then.
pub(crate) fn envelope_mac_key<'a>(
    engine: &JournalEngine<Backend>,
    cached: &'a mut Option<EnvelopeKey>,
) -> Option<&'a EnvelopeKey> {
    if cached.is_none() {
        if let Ok(bytes) = engine.get(hardening::DEVICE_KEY_PATH) {
            // TASK-0025 step 4: keystored wraps the record as an Integrity
            // envelope; pre-migration journals still hold the raw 32-byte
            // seed. `device_seed_from_stored` handles both (deterministic,
            // no panic; shared with the TASK-0027 record-key derivation).
            if let Some(seed) = crate::enc_svc::device_seed_from_stored(&bytes) {
                if let Ok(key) = hardening::derive_key_from_device_seed(&seed) {
                    *cached = Some(key);
                }
            }
        }
    }
    cached.as_ref()
}

/// Replay-time anti-rollback feed: walk the enrolled prefixes of a freshly
/// replayed engine and record each envelope seq. Bounded; malformed or
/// migration-era (non-envelope) values are skipped without panic.
pub(crate) fn observe_enrolled(engine: &JournalEngine<Backend>, tracker: &mut SeqTracker) {
    /// Per-prefix replay-walk bound (matches the store's practical key count).
    const PER_PREFIX_LIMIT: usize = 256;
    for prefix in hardening::ENROLLED_PREFIXES {
        let keys = match engine.list(prefix, PER_PREFIX_LIMIT) {
            Ok(keys) => keys,
            Err(_) => continue,
        };
        for key in keys {
            let value = match engine.get(&key) {
                Ok(value) => value,
                Err(_) => continue,
            };
            if hardening::observe_replayed(&key, &value, tracker).is_err() {
                // Tracker capacity exhausted: fail loud, keep serving (the
                // put path still rejects stale seqs for tracked keys).
                emit_line("statefsd: replay observe limit");
                return;
            }
        }
    }
}
