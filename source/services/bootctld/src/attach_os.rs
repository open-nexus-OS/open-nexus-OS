// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! CONTEXT: bootctld attach half (split out of `os_lite.rs` under the
//! structure ratchet when the TASK-0289-B rollback observation grew it
//! past the module cap; behavior unchanged): record load over the fixed
//! wired statefs slots, BSB reconcile (incl. the loader rollback
//! observation persist), the measured-record read and the target
//! announcement line.
//! OWNERS: @reliability @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU ladder (attach markers); machine/bsb halves are
//!   host-tested in `machine.rs` / `bsb.rs` / tests/bsb_projection.rs.
//! ADR: docs/adr/0055-bootctld-single-boot-state-authority.md

use nexus_ipc::KernelClient;
use statefs::client::StatefsClient;
use statefs::StatefsError;

use crate::machine::{BootCtrl, BootTarget, Slot};
use crate::os_lite::{emit, Authority, REPLY_RECV_SLOT, REPLY_SEND_SLOT, STATEFS_SEND_SLOT};
use crate::record::{self, BOOT_RECORD_KEY};

/// Load the record via the shared statefs client over the FIXED wired
/// slots (@reply inbox — never the shared response queue, never the
/// responder). `NotFound` = fresh image (defaults); corrupt = LOUD +
/// defaults (fatal in proof boots via the harness guard); wire trouble =
/// retry (bounded by the caller).
pub(crate) fn try_attach() -> Option<Authority> {
    let client = KernelClient::new_with_slots(STATEFS_SEND_SLOT, REPLY_RECV_SLOT).ok()?;
    let reply = KernelClient::new_with_slots(REPLY_SEND_SLOT, REPLY_RECV_SLOT).ok();
    let statefs = StatefsClient::from_clients(client, reply);
    let boot = match statefs.get(BOOT_RECORD_KEY) {
        Ok(bytes) => match record::open_record(&bytes) {
            Ok((boot, _seq)) => boot,
            Err(_) => {
                emit("bootctld: record corrupt (defaults)");
                BootCtrl::new(Slot::A)
            }
        },
        Err(StatefsError::NotFound) => BootCtrl::new(Slot::A),
        Err(_) => return None,
    };
    // Until the boot-attempt consumes a one-shot, this session's graph is
    // whatever the armed next_boot says (init WILL consume it) falling
    // back to the persistent target.
    let session_graph = boot.next_boot().unwrap_or(boot.boot_target());
    // TASK-0036-B: attach the bsb projection client and reconcile the
    // on-disk pair against the loaded record (bsb_os.rs).
    let mut boot = boot;
    let (bsb_dev, bsb_seq, bsb_synced, rollback_observed) =
        crate::bsb_os::attach_and_reconcile(&mut boot);
    let measured = nexus_abi::boot_measured_read();
    let auth =
        Authority { boot, client: statefs, session_graph, bsb_dev, bsb_seq, bsb_synced, measured };
    // TASK-0289-B: the loader exhausted our pending trial while userspace
    // was dead — the record was rolled back in RAM above; persist it now
    // and say so. A failed persist is LOUD and leaves the RAM state
    // authoritative (it re-persists with the next mutation).
    if rollback_observed {
        match crate::persist_os::persist_record(&auth.client, &auth.boot) {
            Ok(()) => emit("bootctld: rollback observed (trial exhausted)"),
            Err(_) => emit("bootctld: rollback observed (persist FAIL)"),
        }
    }
    Some(auth)
}

pub(crate) fn announce_target(boot: &BootCtrl) {
    let target = target_label(boot.boot_target());
    let next = boot.next_boot().map(target_label).unwrap_or("none");
    let mut line = [0u8; 48];
    let mut len = 0usize;
    for part in ["bootctld: target=", target, " next=", next] {
        let bytes = part.as_bytes();
        if len + bytes.len() > line.len() {
            return;
        }
        line[len..len + bytes.len()].copy_from_slice(bytes);
        len += bytes.len();
    }
    if let Ok(msg) = core::str::from_utf8(&line[..len]) {
        emit(msg);
    }
}

fn target_label(target: BootTarget) -> &'static str {
    match target {
        BootTarget::Normal => "normal",
        BootTarget::Recovery => "recovery",
        BootTarget::Safe => "safe",
    }
}
