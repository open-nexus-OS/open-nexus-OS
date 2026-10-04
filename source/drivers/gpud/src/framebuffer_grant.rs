// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: gpud's half of the framebuffer grant (RFC-0093 §5 as amended 2026-09-30, RFC-0098 C7,
//! TASK-0251 P1). windowd asks once (`OP_FRAMEBUFFER_REQUEST`); the display makes its framebuffer
//! (once, for its scanout device — the virtio GPU's runs, the board controller's one contiguous
//! block) and gpud answers on its response endpoint with the mode the display decided and a clone
//! of the framebuffer object moved along. The attach that follows (`OP_SET_FRAMEBUFFER_VMO`) names
//! no cap: the framebuffer it scans out is the one it granted, and an attach that carries a cap is
//! a peer from before the amendment — refused, cap closed.
//! OWNERS: @gpu @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the wire in `nexus_display_proto::grant` (host, `test_reject_*`); every QEMU
//!   display lane (`gpud: framebuffer granted (WxH)`, `windowd: display mode from gpud (WxH)`)

use nexus_display_proto::{
    encode_framebuffer_grant, FramebufferGrant, STATUS_DEVICE_ERROR, STATUS_MALFORMED, STATUS_OK,
};
use nexus_ipc::{KernelServer, ReplyCap, Server as _, Wait};

use crate::backend::display::Display;

/// `OP_FRAMEBUFFER_REQUEST`: make (once) the framebuffer; the status the answer carries.
pub(crate) fn grant(display: &mut dyn Display) -> u8 {
    if display.framebuffer().is_some() {
        STATUS_OK
    } else {
        STATUS_DEVICE_ERROR
    }
}

/// The grant's answer on gpud's response endpoint (windowd reads it on its declared gpud route):
/// for an OK status the decided mode and a clone of the framebuffer moved along; otherwise — or
/// when the clone cannot travel — a refusal without a mode, which windowd names.
pub(crate) fn answer(server: &KernelServer, display: &mut dyn Display, status: u8) {
    let len = nexus_display_proto::layout::RESOURCE_BYTES as u32;
    let (w, h) = display.mode();
    let mode_w = u16::try_from(w).unwrap_or(0);
    let mode_h = u16::try_from(h).unwrap_or(0);
    if status == STATUS_OK {
        if let Some(clone) = display.framebuffer().and_then(|vmo| nexus_abi::cap_clone(vmo).ok()) {
            let frame = encode_framebuffer_grant(&FramebufferGrant { status, mode_w, mode_h, len });
            let send = nexus_service_topology::slots::gpud::SERVER.send;
            if nexus_ipc::exchange::send_with_cap(send, &frame, clone).is_ok() {
                emit_granted(w, h);
                return;
            }
            let _ = nexus_abi::cap_close(clone);
        }
    }
    let refused = if status == STATUS_OK { STATUS_DEVICE_ERROR } else { status };
    let frame = encode_framebuffer_grant(&FramebufferGrant {
        status: refused,
        mode_w: 0,
        mode_h: 0,
        len: 0,
    });
    let _ = server.send(&frame, Wait::Blocking);
    let _ = nexus_abi::debug_println("gpud: FAIL framebuffer grant refused");
}

/// `OP_SET_FRAMEBUFFER_VMO`: windowd's first frame is in the granted framebuffer — the display
/// scans it out (or holds it behind the splash until the reveal).
pub(crate) fn attach(display: &mut dyn Display, moved: Option<ReplyCap>) -> Result<(), u8> {
    if let Some(cap) = moved {
        cap.close();
        let _ =
            nexus_abi::debug_println("gpud: FAIL attach carried a cap (the framebuffer is gpud's)");
        return Err(STATUS_MALFORMED);
    }
    display.attach()
}

/// `gpud: framebuffer granted (WxH)` — alloc-free, one atomic line.
fn emit_granted(w: u32, h: u32) {
    let mut buf = [0u8; 48];
    let mut len = 0usize;
    let mut put = |bytes: &[u8]| {
        for &b in bytes {
            if len < buf.len() {
                buf[len] = b;
                len += 1;
            }
        }
    };
    put(b"gpud: framebuffer granted (");
    put(dec(w, &mut [0u8; 10]));
    put(b"x");
    put(dec(h, &mut [0u8; 10]));
    put(b")");
    let _ = nexus_abi::trace_line(
        core::str::from_utf8(&buf[..len]).unwrap_or("gpud: framebuffer granted"),
    );
}

/// `v` in decimal inside `scratch`.
fn dec(mut v: u32, scratch: &mut [u8; 10]) -> &[u8] {
    let mut i = scratch.len();
    loop {
        i -= 1;
        scratch[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 || i == 0 {
            break;
        }
    }
    &scratch[i..]
}
