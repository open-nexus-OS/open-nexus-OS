// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]
#![forbid(unsafe_code)]

//! CONTEXT: statefsd record-encryption os-lite glue (TASK-0027) — the
//!   enable path called at mount, after the virtio backend upgrade, after
//!   a device-key write (keys became derivable), and after the admin meta
//!   key is written. Marker honesty: `statefsd: encryption on
//!   (xchacha20poly1305)` is emitted ONLY after `enc_svc::self_check` ran
//!   real AEAD seal/open/tamper-reject in this process; every other
//!   outcome has its own loud line. Marker contract: scripts/qemu-test.sh.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable (service-internal)
//! TEST_COVERAGE: QEMU marker ladder; core logic host-tested via enc_svc
//! ADR: docs/adr/0023-statefs-persistence-architecture.md

use statefs::enc;
use statefs::JournalEngine;

use crate::emit_os::emit_line;
use crate::enc_svc;
use crate::os_lite::Backend;

/// Try to flip encryption on for `engine`. Idempotent: once a context is
/// installed nothing happens. `announce_off` makes the "no meta key" case
/// emit the `statefsd: encryption off` marker (mount-time call sites).
pub(crate) fn try_enable(engine: &mut JournalEngine<Backend>, announce_off: bool) {
    if engine.enc_context().is_some() {
        return;
    }
    let meta = match engine.get(enc::META_KEY) {
        Ok(bytes) => match enc::decode_meta(&bytes) {
            Ok(meta) => meta,
            Err(_) => {
                // Present but malformed: loud, and encryption stays off
                // (fail-closed for writes is wrong here — plaintext puts
                // keep working; the ADMIN made a mistake, not a client).
                emit_line("statefsd: enc meta invalid");
                return;
            }
        },
        Err(_) => {
            if announce_off {
                emit_line("statefsd: encryption off");
            }
            return;
        }
    };
    let seed = match engine
        .get(crate::hardening::DEVICE_KEY_PATH)
        .ok()
        .and_then(|bytes| enc_svc::device_seed_from_stored(&bytes))
    {
        Some(seed) => seed,
        None => {
            // Enabled but the device key does not exist yet (fresh store
            // with a preserved meta record): honest, keys pending.
            emit_line("statefsd: encryption unavailable (keys)");
            return;
        }
    };
    let ctx = match enc_svc::build_context(&seed, meta.salt) {
        Ok(ctx) => ctx,
        Err(_) => {
            emit_line("statefsd: enc self-check failed");
            return;
        }
    };
    if !enc_svc::self_check(&ctx) {
        emit_line("statefsd: enc self-check failed");
        return;
    }
    engine.set_enc_context(ctx);
    emit_line("statefsd: encryption on (xchacha20poly1305)");
}
