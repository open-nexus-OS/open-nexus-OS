// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! CONTEXT: os-lite glue for the BSB projection (TASK-0036-B; split out of
//! `os_lite.rs` under the structure ratchet): the startup attach +
//! reconciliation and the after-commit projection hook. Pure machine logic
//! (project/fields_equal/resync_verdict) lives host-tested in `bsb.rs` —
//! this file only moves bytes and emits the honest markers.
//! OWNERS: @reliability @runtime
//! STATUS: Experimental (TASK-0036 Phase B)
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU ladder (`bootctld: bsb sync/resync` + selftest tail)
//! ADR: docs/adr/0058-boot-selection-block-dual-actor-discipline.md

use storage::remote_blk::RemoteBlockDevice;

use crate::machine::BootCtrl;
use crate::os_lite::{emit, Authority};

/// Startup: attach the blockproto client on the bsb partition and
/// reconcile the on-disk pair against the loaded record (idempotent;
/// loader actuator effects are left for the boot-attempt tick to
/// converge — see `bsb::resync_verdict`). The 4th return names a LOADER
/// ROLLBACK OBSERVATION (TASK-0289-B): the on-disk pair shows the
/// actuator exhausted our pending trial (next cleared, tries zero) while
/// the record still carries it — the record is rolled back HERE, in RAM;
/// the caller persists and announces it (both-or-neither discipline).
pub(crate) fn attach_and_reconcile(
    boot: &mut BootCtrl,
) -> (Option<RemoteBlockDevice>, u64, bool, bool) {
    let mut bsb_dev = crate::bsb::sync::attach();
    let mut bsb_seq = 0u64;
    let mut bsb_synced = false;
    let mut rollback_observed = false;
    match bsb_dev.as_mut() {
        None => emit("bootctld: bsb attach FAIL (projection disabled)"),
        Some(dev) => {
            let current = crate::bsb::sync::read_current(dev);
            // FLOOR ADOPTION (RFC-0089 §10): the factory can only hand the
            // anti-downgrade floor over through the BSB, but the RECORD is
            // the authority — a fresh record starts at 0 and would project
            // that back over the factory value, silently disarming the
            // gate on every new device. The floor NEVER decreases, so the
            // reconciliation is simply: take the higher of the two.
            if let Some((cur, _)) = current.as_ref() {
                if cur.rollback_min_index > boot.rollback_min_index() {
                    let adopted = cur.rollback_min_index;
                    boot.raise_rollback_min(adopted);
                    emit("bootctld: rollback-min adopted from factory bsb");
                }
            }
            // TASK-0289-B rollback observation: the record still carries a
            // pending trial but the disk shows the loader EXHAUSTED it
            // (next cleared, tries zero — the ADR-0058 actuator's only
            // other write). Re-projecting would hand the broken image its
            // tries back; instead the record follows the actuator: roll
            // back to the recorded rollback slot. Ordinary trial
            // decrements (next still set) stay untouched — the
            // boot-attempt tick owns that convergence.
            if let Some((cur, _)) = current.as_ref() {
                if crate::bsb::exhaustion_observed(cur, boot.pending_slot().is_some())
                    && boot.rollback().is_ok()
                {
                    rollback_observed = true;
                }
            }
            let desired = crate::bsb::project(boot);
            match crate::bsb::resync_verdict(current.as_ref().map(|(b, _)| b), &desired) {
                crate::bsb::ResyncVerdict::Equal | crate::bsb::ResyncVerdict::ActuatorPending => {
                    if let Some((cur, _)) = current {
                        bsb_seq = cur.seq;
                        bsb_synced = true;
                    }
                }
                crate::bsb::ResyncVerdict::Drift => match crate::bsb::sync::sync(dev, &desired) {
                    crate::bsb::sync::Outcome::Written(seq) => {
                        bsb_seq = seq;
                        bsb_synced = true;
                        emit("bootctld: bsb resync");
                    }
                    crate::bsb::sync::Outcome::Unchanged(seq) => {
                        bsb_seq = seq;
                        bsb_synced = true;
                    }
                    crate::bsb::sync::Outcome::Io => {
                        emit("bootctld: bsb sync FAIL (io)");
                    }
                },
            }
        }
    }
    (bsb_dev, bsb_seq, bsb_synced, rollback_observed)
}

/// TASK-0036-B: project the record to the bsb partition after a COMMITTED
/// mutation (record first, BSB second — ADR-0058). Projection failure
/// never fails the commit (the record is the authority) but is loud.
pub(crate) fn project_after_commit(auth: &mut Authority) {
    let Some(dev) = auth.bsb_dev.as_mut() else { return };
    let desired = crate::bsb::project(&auth.boot);
    match crate::bsb::sync::sync(dev, &desired) {
        crate::bsb::sync::Outcome::Unchanged(seq) => {
            auth.bsb_seq = seq;
            auth.bsb_synced = true;
        }
        crate::bsb::sync::Outcome::Written(seq) => {
            auth.bsb_seq = seq;
            auth.bsb_synced = true;
            emit_bsb_sync(seq, &desired);
        }
        crate::bsb::sync::Outcome::Io => {
            auth.bsb_synced = false;
            emit("bootctld: bsb sync FAIL (io)");
        }
    }
}

/// `bootctld: bsb sync (seq=<n> active=<s> next=<s|none> tries=<n>)` —
/// fixed-buffer render (bump-heap doctrine: no per-op format!).
fn emit_bsb_sync(seq: u64, b: &bootfmt::bsb::Bsb) {
    let mut line = [0u8; 80];
    let mut len = 0usize;
    let push = |bytes: &[u8], line: &mut [u8; 80], len: &mut usize| {
        if *len + bytes.len() <= line.len() {
            line[*len..*len + bytes.len()].copy_from_slice(bytes);
            *len += bytes.len();
        }
    };
    push(b"bootctld: bsb sync (seq=", &mut line, &mut len);
    let mut digits = [0u8; 20];
    let mut n = seq;
    let mut count = 0usize;
    loop {
        digits[count] = b'0' + (n % 10) as u8;
        count += 1;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    for i in (0..count).rev() {
        push(&[digits[i]], &mut line, &mut len);
    }
    push(b" active=", &mut line, &mut len);
    push(slot_bytes(b.active_slot), &mut line, &mut len);
    push(b" next=", &mut line, &mut len);
    match b.next_slot {
        Some(slot) => push(slot_bytes(slot), &mut line, &mut len),
        None => push(b"none", &mut line, &mut len),
    }
    push(b" tries=", &mut line, &mut len);
    push(&[b'0' + b.tries_left.min(9)], &mut line, &mut len);
    push(b")", &mut line, &mut len);
    if let Ok(msg) = core::str::from_utf8(&line[..len]) {
        emit(msg);
    }
}

fn slot_bytes(slot: bootfmt::bsb::Slot) -> &'static [u8] {
    match slot {
        bootfmt::bsb::Slot::A => b"a",
        bootfmt::bsb::Slot::B => b"b",
    }
}
