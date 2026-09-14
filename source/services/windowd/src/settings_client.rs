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
/// The declared settingsd `(send, recv)` slots (TASK-0324 P4a) — for callers that need
/// the raw request endpoint (the region watch subscription cap-moves its push channel
/// alongside an `OP_WATCH` frame). No route ask (P7-b).
pub(crate) fn settingsd_slots() -> Option<(u32, u32)> {
    let leg = nexus_service_topology::slots::windowd::SETTINGSD;
    Some((leg.send, leg.recv))
}
