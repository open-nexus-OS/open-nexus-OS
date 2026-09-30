// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: inputd's display space — the coordinate space windowd hit-tests in — asked of windowd
//! ONCE at start (RFC-0093 §5 as amended 2026-09-30, RFC-0098 C7). The mode is gpud's decision;
//! windowd receives it with the framebuffer grant and is the one peer inputd has for pointer
//! semantics. One call over inputd's declared reply inbox (`slots::inputd::REPLY`), no retry, no
//! clock: windowd's answer or its death ends the wait. The polled query this replaces, with its
//! silent 1280x800 default, stays retired.
//! OWNERS: @runtime @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the wire in `input_live_protocol::display_space` (host, `test_reject_*`); every
//!   QEMU lane (`inputd: display space from windowd (WxH)`)

use super::alloc::format;

use input_live_protocol::{
    decode_display_space, encode_get_display_space, DISPLAY_SPACE_FRAME_LEN,
};
use nexus_abi::debug_println;
use nexus_service_topology::slots::inputd as topo;

/// The space windowd hit-tests in. A failed ask is NAMED and leaves the layout maximum — the
/// space windowd itself takes when it has no display, so the two still agree.
pub(super) fn from_windowd() -> (u32, u32) {
    let mut answer = [0u8; DISPLAY_SPACE_FRAME_LEN];
    let ask = encode_get_display_space();
    match nexus_ipc::exchange::call_into(topo::WINDOWD.send, topo::REPLY, &ask, &mut answer) {
        Ok(n) => match decode_display_space(&answer[..n.min(answer.len())]) {
            Some((w, h)) => {
                let _ = debug_println(&format!("inputd: display space from windowd ({w}x{h})"));
                (w, h)
            }
            None => {
                let _ = debug_println("inputd: FAIL display space refused by windowd");
                nexus_display_proto::LAYOUT_MAX
            }
        },
        Err(err) => {
            let _ = debug_println(&format!("inputd: FAIL display space ask ({err:?})"));
            nexus_display_proto::LAYOUT_MAX
        }
    }
}
