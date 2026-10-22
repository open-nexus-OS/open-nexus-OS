// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: gpud's reply encoding (RFC-0093 §5) — the ONE place a request's answer takes its
//! wire shape: the framebuffer grant (its sender in `framebuffer_grant`, it moves a cap), the
//! 21-byte attach ack (with the mode gpud commands, as evidence for windowd's
//! cross-check against the one mode source), the present ack `[status, seq]` with the time its
//! frame reached the glass (RFC-0110: a landed flip's boundary, or the time a present acked at
//! once completed), the 5-byte `[status, magic]` of a cursor reply (a magic no present seq ever
//! equals), and the bare status byte of a fire-and-forget control op. Split out of `service.rs`
//! under the module-size ratchet.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal

use crate::backend::display::{Display, Landed};
use nexus_display_proto::frame_time::{encode_present_ack_shown, Shown};
use nexus_display_proto::{
    encode_attach_ack, encode_present_ack, AttachAck, OP_PRESENT_DAMAGE, OP_SET_FRAMEBUFFER_VMO,
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
    if op == nexus_display_proto::OP_READBACK {
        // RFC-0095: two bytes — never the cursor status (one) or a present ack (five).
        let reply = nexus_display_proto::readback::encode_readback_reply(status);
        let _ = server.send(&reply, Wait::Blocking);
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
        if op == OP_PRESENT_DAMAGE {
            // RFC-0110: a present acked at once names the time it completed (a display without a
            // scan clock: QEMU's) and its period; a landed flip names its boundary (`send_landed`).
            let at_ns = nexus_abi::nsec().unwrap_or(0);
            let shown = Shown { at_ns, period_ns: display.period_ns(), cadence: 1 };
            let _ = server.send(&encode_present_ack_shown(status, payload, shown), Wait::Blocking);
        } else {
            let _ = server.send(&encode_present_ack(status, payload), Wait::Blocking);
        }
    } else {
        let _ = server.send(&[status], Wait::Blocking);
    }
}

/// A present whose flip landed (RFC-0101 §8, RFC-0110): its ack, with the frame boundary it
/// reached the glass at, the refresh period and the cadence the display holds.
pub(crate) fn send_landed(server: &KernelServer, landed: Landed) {
    let ack = encode_present_ack_shown(landed.status, landed.seq, landed.shown);
    let _ = server.send(&ack, Wait::Blocking);
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
