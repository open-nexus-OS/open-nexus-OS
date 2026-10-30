// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The fire-and-forget pushes that arrive on the compositor's own server endpoint and
//! speak a protocol of their own — told apart by MAGIC before the `'I','N'` op switch would
//! misread them: imed's composition pushes (`'I','E'`, RFC-0075), settingsd's watch events
//! (`'S','T'`, RFC-0083 — the watch caps are clones of OUR OWN server send half, so a change
//! wakes an idle compositor as ordinary inbound IPC) and the memory authority's level
//! (`'M','D'`, TASK-0333 6e-3) — and one data-only app push of the surface protocol itself, the
//! focused field's caret (`OP_SURFACE_CARET`, RFC-0113 §5). None of them is answered.
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
    use nexus_display_proto::surface_caret::OP_SURFACE_CARET;
    match (frame[0], frame[1], frame[3]) {
        (b'I', b'E', _) => runtime.handle_imed_push(frame, sender_sid),
        (b'S', b'T', _) => runtime.handle_settings_event(frame),
        (b'M', b'D', _) => runtime.handle_mem_pressure(frame, sender_sid),
        (b'I', b'N', OP_SURFACE_CARET) => runtime.handle_surface_caret(frame),
        _ => return false,
    }
    true
}
