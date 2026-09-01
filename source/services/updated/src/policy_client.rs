// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]
#![forbid(unsafe_code)]

//! CONTEXT: updated's policyd client (TASK-0140) — resolves the `policyd`
//! route once via the init responder (the `bootctl_client` pattern) and
//! answers the `updates.manage` question over the delegated capability
//! wire. Replies ride updated's OWN CAP_MOVE inbox (deterministic slots
//! 0x0A/0x0B). Transport trouble maps to `Unreachable`, which the gate
//! treats as deny (fail closed).
//! OWNERS: @runtime @security
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: gate mapping in `manage_gate`; QEMU deny lane (TASK-0140).
//! ADR: docs/rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md

use core::sync::atomic::{AtomicU32, Ordering};
use core::time::Duration;

use nexus_ipc::budget::{self, NonceMismatchBudget, RouteRetryOutcome};

use crate::manage_gate::{PolicyAnswer, MANAGE_CAP};

/// init-lite control-channel slots (route requests via the responder).
const CTRL_SEND_SLOT: u32 = 1;
const CTRL_RECV_SLOT: u32 = 2;
/// updated's deterministic CAP_MOVE reply inbox (slot_map SSOT).
const REPLY_RECV_SLOT: u32 = 0x0a;
const REPLY_SEND_SLOT: u32 = 0x0b;

/// Asks policyd whether `sender` holds `updates.manage`.
pub(crate) fn manage_answer(sender: u64) -> PolicyAnswer {
    let Some(send_slot) = cached_send_slot() else {
        return PolicyAnswer::Unreachable;
    };
    match nexus_ipc::policyd::check_cap_on(
        send_slot,
        REPLY_SEND_SLOT,
        REPLY_RECV_SLOT,
        sender,
        MANAGE_CAP,
    ) {
        nexus_ipc::policyd::CapDecision::Allow => PolicyAnswer::Allow,
        nexus_ipc::policyd::CapDecision::Deny => PolicyAnswer::Deny,
        nexus_ipc::policyd::CapDecision::Unreachable => PolicyAnswer::Unreachable,
    }
}

fn cached_send_slot() -> Option<u32> {
    static SEND: AtomicU32 = AtomicU32::new(0);
    let cached = SEND.load(Ordering::Relaxed);
    if cached != 0 {
        return Some(cached);
    }
    match budget::route_with_nonce_budgeted(
        b"policyd",
        CTRL_SEND_SLOT,
        CTRL_RECV_SLOT,
        Duration::from_secs(2),
        NonceMismatchBudget::new(64),
    ) {
        RouteRetryOutcome::Success { send_slot, .. } => {
            SEND.store(send_slot, Ordering::Relaxed);
            Some(send_slot)
        }
        _ => None,
    }
}
