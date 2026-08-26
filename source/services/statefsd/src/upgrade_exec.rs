// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]

//! CONTEXT: The pristine-window backend upgrade (split from `os_lite.rs`
//! under the structure ratchet; TASK-0315 form): attach the STATE
//! partition over the blockproto plane (bounded blocking — the window
//! must never lose to virtioblkd still bringing the device up), swap the
//! RAM engine for the disk engine, rebuild anti-rollback state, and
//! account failures against the window's bounded retry budget.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU ladder (`statefsd: virtio upgrade ok` + keep-blk
//!   double boot).
//! ADR: docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md

use nexus_abi::yield_;
use statefs::envelope::SeqTracker;
use statefs::JournalEngine;

use crate::emit_os::{emit_blk_marker, emit_degrade_ram_backed, emit_line, emit_statefs_error};
use crate::hardening_os::observe_enrolled;
use crate::os_lite::{Backend, Hardening};
use crate::upgrade_window::UpgradeState;

/// One bounded upgrade attempt (called while the window wants one).
pub(crate) fn try_upgrade(
    engine: &mut JournalEngine<Backend>,
    hard: &mut Hardening,
    window: &mut UpgradeState,
) {
    // TASK-0315: the device left this process — attach the
    // STATE partition over the blockproto plane instead
    // (virtioblkd owns the queue; same pristine-window
    // discipline as the old direct-MMIO upgrade). The
    // attempt BLOCKS bounded, so the first request stalls
    // like the old inline device init did — the window can
    // never lose to virtioblkd still coming up.
    match crate::route_os::attach_state_partition() {
        None => {
            emit_line("statefsd: journal open failed (virtio)");
            match window.on_open_failed() {
                crate::upgrade_window::UpgradeAction::AnnounceRetriesExhausted => {
                    emit_degrade_ram_backed("virtio retries exhausted");
                }
                _ => {
                    emit_line("statefsd: virtio retry scheduled");
                }
            }
        }
        Some(blk) => {
            emit_blk_marker(&blk);
            match JournalEngine::open(Backend::Remote(blk)) {
                Ok(new_engine) => {
                    *engine = new_engine;
                    let _ = window.on_open_ok();
                    // New backing store: rebuild the
                    // anti-rollback state from its replay
                    // (the mem engine was still pristine).
                    hard.tracker = SeqTracker::new();
                    observe_enrolled(engine, &mut hard.tracker);
                    emit_line("statefsd: virtio upgrade ok");
                    crate::enc_os::try_enable(engine, false);
                }
                Err(err) => {
                    emit_line("statefsd: journal open failed (virtio)");
                    emit_statefs_error(err);
                    match window.on_open_failed() {
                        crate::upgrade_window::UpgradeAction::AnnounceRetriesExhausted => {
                            emit_degrade_ram_backed("virtio retries exhausted");
                        }
                        _ => {
                            // Delay before next retry to let QEMU virtio settle
                            emit_line("statefsd: virtio retry scheduled");
                            for _ in 0..100 {
                                let _ = yield_();
                            }
                        }
                    }
                }
            }
        }
    }
}
