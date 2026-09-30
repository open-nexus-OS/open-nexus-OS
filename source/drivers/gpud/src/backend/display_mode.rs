// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: gpud decides the display mode (RFC-0098 C7, TASK-0251 P1) — the one authority. It
//! reads the lane's REQUEST from its own read-only tree slot (`/chosen/nexus,display-mode`,
//! written by nxboot from the launcher's knob; absent on the board), takes the device's
//! CAPABILITY from the device (virtio display info on QEMU; the monitor's EDID with the board's
//! display controller, TASK-0251 P2), and applies the one policy,
//! `nexus_display_proto::resolve_display_mode`. windowd receives the decision with the
//! framebuffer grant; nobody else reads the request (the kernel's relay, syscall 50, is gone).
//!
//! Markers (alloc-free — the heap never sees boot markers): `gpud: display info WxH` (the
//! device's advertised capability), `gpud: display mode WxH (<source>)` (the decision and the
//! candidate it came from: request / device / maximum), `gpud: FAIL display mode <req> vs device
//! <cap>` (a disagreement, named instead of silently overruled — the GTK-window race that made a
//! device report its unrealized default used to be invisible).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal

/// The lane's display-mode request, read from gpud's own tree slot; `None` when the tree names
/// none (the board, a direct-kernel boot) or the value is not a request.
#[cfg(all(feature = "os-lite", target_os = "none"))]
pub(super) fn request_from_tree() -> Option<(u32, u32)> {
    let tree =
        nexus_abi::device_tree::map_read_only(nexus_service_topology::slots::gpud::DEVICE_TREE)?;
    let chosen = nexus_fdt::Fdt::new(tree).ok()?.chosen().ok()?;
    nexus_display_proto::parse_display_request(chosen.nexus_str("display-mode")?.as_bytes())
}

/// The VISIBLE mode gpud commands onto the scanout and grants to windowd. Names a request the
/// device disagrees with and the candidate the policy took.
#[cfg(all(feature = "os-lite", target_os = "none"))]
pub(super) fn resolve(request: Option<(u32, u32)>, device: Option<(u32, u32)>) -> (u32, u32) {
    if let (Some((rw, rh)), Some((dw, dh))) = (request, device) {
        if (rw, rh) != (dw, dh) {
            let mut line = Line::new();
            line.text(b"gpud: FAIL display mode ").mode(rw, rh);
            line.text(b" vs device ").mode(dw, dh).emit();
        }
    }
    let (mode, source) = nexus_display_proto::resolve_display_mode_sourced(
        request,
        device,
        nexus_display_proto::LAYOUT_MAX,
    );
    let mut line = Line::new();
    line.text(b"gpud: display mode ").mode(mode.0, mode.1);
    line.text(b" (").text(source.label().as_bytes()).text(b")").emit();
    mode
}

/// `gpud: display info WxH` — the device's advertised capability (diagnostic).
#[cfg(all(feature = "os-lite", target_os = "none"))]
pub(super) fn emit_display_info_marker(w: u32, h: u32) {
    Line::new().text(b"gpud: display info ").mode(w, h).emit();
}

/// A bounded stack line, emitted with one `trace_line` (one atomic console line).
#[cfg(all(feature = "os-lite", target_os = "none"))]
struct Line {
    buf: [u8; 72],
    len: usize,
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
impl Line {
    const fn new() -> Self {
        Self { buf: [0; 72], len: 0 }
    }

    fn text(&mut self, s: &[u8]) -> &mut Self {
        for &b in s {
            if self.len < self.buf.len() {
                self.buf[self.len] = b;
                self.len += 1;
            }
        }
        self
    }

    fn dec(&mut self, mut v: u32) -> &mut Self {
        let mut tmp = [0u8; 10];
        let mut n = 0;
        loop {
            tmp[n] = b'0' + (v % 10) as u8;
            v /= 10;
            n += 1;
            if v == 0 {
                break;
            }
        }
        while n > 0 {
            n -= 1;
            let digit = [tmp[n]];
            self.text(&digit);
        }
        self
    }

    fn mode(&mut self, w: u32, h: u32) -> &mut Self {
        self.dec(w).text(b"x").dec(h)
    }

    fn emit(&self) {
        let _ =
            nexus_abi::trace_line(core::str::from_utf8(&self.buf[..self.len]).unwrap_or("gpud"));
    }
}
