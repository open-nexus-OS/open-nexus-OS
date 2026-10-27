// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the memory level, told to every app window (TASK-0333 6e-3, RFC-0100 §3, ADR-0086).
//! memd — the one memory authority — pushes `nexus_wire::memd::OP_LEVEL` to the compositor's
//! server endpoint; the compositor takes it from memd's kernel sender id alone and tells each
//! live app window the level as `OP_SURFACE_PRESSURE` on its dedicated event channel (NONBLOCK,
//! bounded — a slow app never wedges the compositor, ADR-0082), where app-host raises it into
//! the program as the host event `MemoryPressure`. The compositor decides nothing here and
//! drops nothing of its own: its scene is what is on screen. Marker: `windowd: mem pressure
//! (level= source= windows=)`.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the frame codecs in `nexus-wire` / `nexus-display-proto` (host); the visible
//!   lane (`windowd: mem pressure (level=warn` after the harness's drill)

use nexus_abi::debug_println;

use super::*;

impl DisplayServerRuntime {
    /// A `'M','D'` frame on the server endpoint: the level from the memory authority.
    pub(crate) fn handle_mem_pressure(&mut self, frame: &[u8], sender_sid: u64) {
        if sender_sid == 0 || sender_sid != nexus_abi::service_id_from_name(b"memd") {
            let _ = debug_println("windowd: FAIL mem pressure (foreign sender)");
            return;
        }
        let Some((level, source, _free, _total)) = nexus_wire::memd::decode_level(frame) else {
            let _ = debug_println("windowd: FAIL mem pressure (malformed)");
            return;
        };
        let push = nexus_display_proto::surface_pressure::encode_surface_pressure(level);
        let mut told = 0u32;
        for idx in 0..self.apps.len() {
            let live =
                self.apps[idx].surface_id.is_some() && self.apps[idx].event_channel.is_some();
            if live && self.send_app_frame(idx, &push) {
                told += 1;
            }
        }
        let _ = debug_println(&alloc::format!(
            "windowd: mem pressure (level={} source={} windows={told})",
            nexus_abi::syscall::memory::mem_pressure_name(level),
            nexus_wire::memd::source_name(source)
        ));
    }
}
