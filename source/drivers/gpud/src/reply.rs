// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: gpud's reply encoding (RFC-0093 §5) — the ONE place a request's answer takes its
//! wire shape: the framebuffer grant (its sender in `framebuffer_grant`, it moves a cap), the
//! 21-byte attach ack (with the mode gpud commands, as evidence for windowd's
//! cross-check against the one mode source), the 5-byte present ack `[status, seq]` (also the
//! shape of a cursor reply, whose payload is a magic no present seq ever equals), and the bare
//! status byte of a fire-and-forget control op. Split out of `service.rs` under the
//! module-size ratchet.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal

use crate::backend::display::Display;
use nexus_display_proto::{
    encode_attach_ack, encode_present_ack, AttachAck, OP_SET_FRAMEBUFFER_VMO,
};
use nexus_ipc::{KernelServer, Server as _, Wait};

/// Answer the request `op` with `status`. `payload` is the present `seq` or a cursor magic;
/// `None` means a bare status byte.
pub(crate) fn send(
    server: &KernelServer,
    display: &mut dyn Display,
    op: u8,
    status: u8,
    payload: Option<u32>,
    active_handoff_id: u32,
) {
    if op == nexus_display_proto::OP_FRAMEBUFFER_REQUEST {
        // RFC-0098 C7: the answer moves the framebuffer along — its own sender.
        crate::framebuffer_grant::answer(server, display, status);
        return;
    }
    if op == OP_SET_FRAMEBUFFER_VMO {
        let (mode_w, mode_h) = display.mode();
        let w = mode_w.min(u16::MAX as u32) as u16;
        let h = mode_h.min(u16::MAX as u32) as u16;
        let ack = AttachAck {
            status,
            handoff_id: payload.unwrap_or(active_handoff_id),
            seq: 0,
            mode_w: w,
            mode_h: h,
            content_x: 0,
            content_y: 0,
            content_w: w,
            content_h: h,
        };
        let _ = server.send(&encode_attach_ack(&ack), Wait::Blocking);
    } else if let Some(payload) = payload {
        let _ = server.send(&encode_present_ack(status, payload), Wait::Blocking);
    } else {
        let _ = server.send(&[status], Wait::Blocking);
    }
}

/// The status a successful present is acked with (RFC-0093 §5): the FIRST present after
/// `OP_REVEAL` that finds the splash released is the frame that revealed the desktop, and its
/// ack says so — exactly once per boot, whichever present (windowd's or a splash self-tick)
/// uploaded the wallpaper. The ONE reveal marker prints here, with the seq of a frame that
/// was actually presented.
pub(crate) fn present_status(
    display: &dyn Display,
    status: u8,
    seq: u32,
    reveal_acked: &mut bool,
) -> u8 {
    if status == nexus_display_proto::STATUS_OK
        && display.reveal_requested()
        && !*reveal_acked
        && !display.holding_splash()
    {
        *reveal_acked = true;
        crate::markers::emit_desktop_reveal(seq);
        return nexus_display_proto::STATUS_REVEALED;
    }
    status
}
