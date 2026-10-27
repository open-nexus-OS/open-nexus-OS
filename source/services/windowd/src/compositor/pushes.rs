// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The fire-and-forget pushes that arrive on the compositor's own server endpoint and
//! speak a protocol of their own — told apart by MAGIC before the `'I','N'` op switch would
//! misread them: imed's composition pushes (`'I','E'`, RFC-0075), settingsd's watch events
//! (`'S','T'`, RFC-0083 — the watch caps are clones of OUR OWN server send half, so a change
//! wakes an idle compositor as ordinary inbound IPC) and the memory authority's level
//! (`'M','D'`, TASK-0333 6e-3). None of them is answered.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: each push's handler and codec (host); the visible lane's markers

use super::runtime::DisplayServerRuntime;

/// Dispatches `frame` when it is one of the pushes above; `true` when it was (no reply is owed,
/// the op switch must not see it), `false` for every ordinary client frame.
pub(super) fn dispatch_push(
    runtime: &mut DisplayServerRuntime,
    frame: &[u8],
    sender_sid: u64,
) -> bool {
    if frame.len() < 4 {
        return false;
    }
    match (frame[0], frame[1]) {
        (b'I', b'E') => runtime.handle_imed_push(frame, sender_sid),
        (b'S', b'T') => runtime.handle_settings_event(frame),
        (b'M', b'D') => runtime.handle_mem_pressure(frame, sender_sid),
        _ => return false,
    }
    true
}
