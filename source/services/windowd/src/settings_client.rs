// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: settingsd route resolution for windowd's watch subscriptions
//! (RFC-0083): windowd never writes or polls settings any more — settingsd
//! is the one authority, values arrive as watch events (the registration
//! burst is the boot restore). Only the cached route lookup survives here.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: exercised by the watch subscription path (QEMU ladder).
//! RFC: docs/rfcs/RFC-0083-settings-distribution-v2-single-authority-versioned-snapshots.md

#![cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]

use core::time::Duration;
use nexus_ipc::budget::{self, NonceMismatchBudget, RouteRetryOutcome};

/// init-lite control-channel slots (route requests go through the responder).
const CTRL_SEND_SLOT: u32 = 1;
const CTRL_RECV_SLOT: u32 = 2;

/// The resolved settingsd `(send, recv)` slots — for callers that need the
/// raw request endpoint (the region watch subscription cap-moves its push
/// channel alongside an `OP_WATCH` frame).
pub(crate) fn settingsd_slots() -> Option<(u32, u32)> {
    route_blocking(b"settingsd")
}

/// Resolves a service (or `@reply`) to its `(send, recv)` slots via the responder.
fn route_blocking(name: &[u8]) -> Option<(u32, u32)> {
    match budget::route_with_nonce_budgeted(
        name,
        CTRL_SEND_SLOT,
        CTRL_RECV_SLOT,
        Duration::from_secs(2),
        NonceMismatchBudget::new(64),
    ) {
        RouteRetryOutcome::Success { send_slot, recv_slot } => Some((send_slot, recv_slot)),
        _ => None,
    }
}
