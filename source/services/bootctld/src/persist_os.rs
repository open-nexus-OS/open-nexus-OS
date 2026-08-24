// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]
#![forbid(unsafe_code)]

//! CONTEXT: bootctld record persistence (TASK-0050 PR-2) — the proven
//! read-modify-write discipline relocated from updated's
//! `persist_bootctrl_state`: learn the stored seq (missing/legacy → first
//! write seq 1), seal v2 with seq = last_seen + 1, PUT with ONE bounded
//! retry on a rollback race (statefsd's replay-fed tracker is
//! authoritative), then SYNC. Failure detail is rate-limited LOUD with the
//! statefs SSOT label — never a bare "err".
//! OWNERS: @reliability @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU OTA ladder (relocated authority; markers
//!   unchanged); codec halves host-tested in tests/record_v2.rs.
//! ADR: docs/adr/0055-bootctld-single-boot-state-authority.md

use statefs::client::StatefsClient;
use statefs::StatefsError;

use crate::machine::BootCtrl;
use crate::record::{self, BOOT_RECORD_KEY};

/// Durably persists the record; the caller owns RAM-state compensation.
pub(crate) fn persist_record(client: &StatefsClient, boot: &BootCtrl) -> Result<(), StatefsError> {
    static PERSIST_ERR_LOGGED: core::sync::atomic::AtomicBool =
        core::sync::atomic::AtomicBool::new(false);
    let log_err = |stage: &str, e: StatefsError| {
        if !PERSIST_ERR_LOGGED.swap(true, core::sync::atomic::Ordering::Relaxed) {
            let mut line = [0u8; 64];
            let mut len = 0usize;
            for part in ["bootctld: persist ", stage, " err=", e.label()] {
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
    };
    let mut retried = false;
    loop {
        let last_seen = match client.get(BOOT_RECORD_KEY) {
            Ok(bytes) => match statefs::writer::open_stored(&bytes) {
                Ok(stored) => stored.seq(),
                Err(e) => {
                    log_err("read", e);
                    return Err(e);
                }
            },
            Err(StatefsError::NotFound) => None,
            Err(e) => {
                log_err("read", e);
                return Err(e);
            }
        };
        let seq = statefs::writer::next_seq(last_seen);
        let ts = nexus_abi::nsec().unwrap_or(0);
        let sealed = match record::seal_record(boot, seq, ts) {
            Ok(sealed) => sealed,
            Err(e) => {
                log_err("seal", e);
                return Err(e);
            }
        };
        match client.put(BOOT_RECORD_KEY, &sealed) {
            Ok(()) => break,
            Err(StatefsError::RollbackDetected) if !retried => {
                retried = true;
            }
            Err(e) => {
                log_err("put", e);
                return Err(e);
            }
        }
    }
    if let Err(e) = client.sync() {
        log_err("sync", e);
        return Err(e);
    }
    Ok(())
}
