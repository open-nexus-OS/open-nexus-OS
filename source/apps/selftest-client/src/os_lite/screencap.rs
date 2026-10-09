// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: screencapd probe (RFC-0095, TASK-0068). Once the display shows the desktop, the
//! harness asks the capture facade over its declared route to read a strip at the centre of
//! the frame the display shows — screencapd → windowd's capture verb (`PROBE`, no freeze) →
//! gpud's ONE readback into screencapd's frame VMO — and answers the strip's brightest pixel.
//! Non-black proves the whole pixel path: the readback on this lane's backend (GL front
//! target, virtio 2D display plane), windowd's gate (screencapd's kernel sid), the lent frame
//! VMO. Only one pixel value crosses back, never pixels.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: QEMU marker ladder (`SELFTEST: ui v7 screencap ok`, display lanes)
//! RFC: docs/rfcs/RFC-0095-screen-capture-v1-readback-freeze-shell-ui.md

use nexus_wire::screencapd as wire;

/// The strip: 64×4 pixels at the centre of the smallest display mode the lanes run (1280×800);
/// inside every larger mode too.
const STRIP: (u16, u16, u16, u16) = (608, 398, 64, 4);

/// `Some(true)` when the strip's brightest pixel is not black (its B+G+R sum clears a
/// dark-frame floor), `Some(false)` when the read failed or found black, `None` when the system
/// runs without a display (a headless lane: nothing to prove, no marker).
pub(crate) fn screencap_probe() -> Option<bool> {
    let route = nexus_service_topology::slots::selftest_client::SCREENCAPD;
    let reply = nexus_service_topology::slots::selftest_client::REPLY;
    let (x, y, w, h) = STRIP;
    let mut rsp = [0u8; wire::REPLY_MAX_BYTES];
    let req = wire::encode_probe(x, y, w, h);
    let Ok(n) = nexus_ipc::exchange::call_into(route.send, reply, &req, &mut rsp) else {
        return Some(false);
    };
    let Some((status, brightest)) = wire::decode_probe_reply(wire::OP_PROBE, &rsp[..n]) else {
        return Some(false);
    };
    if status == wire::STATUS_NO_DISPLAY {
        return None;
    }
    let [b, g, r, _] = brightest.to_le_bytes();
    let sum = u32::from(b) + u32::from(g) + u32::from(r);
    Some(status == wire::STATUS_OK && sum > 3 * 8)
}
