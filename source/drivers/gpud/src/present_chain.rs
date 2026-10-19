// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The present chain's hops as the request loop drives them — validate, execute into
//! the display, derive the damage, scan it out, trace the hops once — split out of
//! `service.rs` under the module-size ratchet (TASK-0216: the loop gained the vertical blank's
//! deferred ack). The display decides WHERE a present is drawn (RFC-0101 §8: a ring of planes);
//! this is what every present goes through.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: every visible QEMU lane; the board ladder (`gpud: chain …` hops once)

use nexus_abi::debug_println;
use nexus_display_proto::{STATUS_DEVICE_ERROR, STATUS_MALFORMED, STATUS_OK};
use nexus_gfx::backend::error::GfxError;
use nexus_gfx::backend::types::Rect;
use nexus_gfx::command::buffer::{Command, CommittedBuffer};

use crate::backend::display::Display;

/// The display plane's first row in the shared layout (`nexus_display_proto::layout`): an
/// absolute blit at or below it lands on screen.
const DISPLAY_PLANE_ROW: u32 = nexus_display_proto::layout::DISPLAY_ROW;

/// One present of validated commands: execute (G3) — on `frames`, the GPU that draws windowd's
/// frames, when one is ready — then scan the damage out (G4). The chain hops print while
/// `trace`; the status is the present's before the reveal check.
pub(crate) fn present(
    display: &mut dyn Display,
    frames: Option<&mut dyn crate::gpu_context::FrameGpu>,
    cb: &CommittedBuffer,
    trace: bool,
) -> u8 {
    let (w, h) = display.mode();
    let damage_rect = damage_rect_from_cb(cb, w, h);
    // A failed composite is never silent: the hop and the reason print.
    let executed = cb
        .validate()
        .map_err(crate::backend::map_nexus_error)
        .and_then(|()| display.execute_on(frames, cb.commands(), damage_rect));
    match executed {
        Ok(()) => {
            if trace {
                let _ = debug_println(crate::markers::GPUD_CHAIN_EXEC_OK);
            }
        }
        Err(e) => {
            let _ = debug_println(crate::markers::GPUD_CHAIN_EXEC_FAIL);
            let _ = debug_println(gfx_error_label(e));
        }
    }
    let st = present_scanout_damage(display, damage_rect);
    if trace {
        let _ = debug_println(if st == STATUS_OK {
            crate::markers::GPUD_CHAIN_SCANOUT_OK
        } else {
            crate::markers::GPUD_CHAIN_SCANOUT_FAIL
        });
    }
    st
}
/// Human-readable reason for a present-chain hop failure (G3 exec). Static
/// strings only — no alloc on gpud's bump heap.
fn gfx_error_label(e: GfxError) -> &'static str {
    match e {
        GfxError::DeviceNotFound => "gpud: chain reason: device not found",
        GfxError::MmioFault => "gpud: chain reason: mmio fault",
        GfxError::CommandRejected => "gpud: chain reason: command rejected",
        GfxError::ResourceExhausted => "gpud: chain reason: resource exhausted (bump heap?)",
        GfxError::Unsupported => "gpud: chain reason: unsupported command",
        GfxError::InvalidArgument => "gpud: chain reason: invalid argument",
    }
}
/// Extract bounding damage rect from ALL command types.
fn damage_rect_from_cb(cb: &CommittedBuffer, display_w: u32, display_h: u32) -> Rect {
    let mut min_x = display_w;
    let mut min_y = display_h;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut found = false;
    for cmd in cb.commands() {
        let (x, y, w, h) = match cmd {
            Command::BlitSurface { dst_x, dst_y, width, height, .. } => {
                (*dst_x, *dst_y, *width, *height)
            }
            // Absolute blits that target the display plane (e.g. the chat layer
            // composite, sidebar/button blur-cache restores) MUST contribute to
            // the present damage, or their region is written to the backing but
            // never transferred/flushed to the host. Convert the absolute dst row
            // back to screen-relative; ignore blits aimed elsewhere (atlas/cache).
            Command::BlitAbsolute { dst_x, dst_y_abs, width, height, .. } => {
                if *dst_y_abs >= DISPLAY_PLANE_ROW && *dst_y_abs < DISPLAY_PLANE_ROW + display_h {
                    (*dst_x, dst_y_abs - DISPLAY_PLANE_ROW, *width, *height)
                } else {
                    continue;
                }
            }
            Command::FillSdfRoundedRect { rect, .. } => (rect.x, rect.y, rect.width, rect.height),
            Command::FillSdfGradient { rect, .. } => (rect.x, rect.y, rect.width, rect.height),
            Command::CompositeLayer {
                width,
                height,
                dst_x,
                dst_y,
                shadow_blur,
                shadow_offset_y,
                ..
            } => {
                // Damage the layer rect plus its shadow halo (blur + offset).
                let pad = *shadow_blur + shadow_offset_y.unsigned_abs();
                let x0 = dst_x.saturating_sub(pad);
                let y0 = dst_y.saturating_sub(pad);
                let x1 = (dst_x + width).saturating_add(pad);
                let y1 = (dst_y + height).saturating_add(pad);
                (x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
            }
            Command::DropShadow { rect, blur, offset_x, offset_y, .. } => {
                // The painted halo extends past the casting rect by blur,
                // shifted by the offset — damage the full extent (clamped).
                let pad = *blur as i32;
                let x0 = (rect.x as i32 + offset_x - pad).max(0) as u32;
                let y0 = (rect.y as i32 + offset_y - pad).max(0) as u32;
                let x1 = ((rect.x + rect.width) as i32 + offset_x + pad).max(0) as u32;
                let y1 = ((rect.y + rect.height) as i32 + offset_y + pad).max(0) as u32;
                (x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
            }
            Command::BlurBackdrop { rect, .. } => (rect.x, rect.y, rect.width, rect.height),
            Command::BlendCursor { x, y, width, height } => (*x, *y, *width, *height),
            _ => continue,
        };
        let ex = x.saturating_add(w);
        let ey = y.saturating_add(h);
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(ex);
        max_y = max_y.max(ey);
        found = true;
    }
    if found {
        // Clamp to the display plane — halo-style commands (DropShadow) may
        // extend past the screen edges.
        let min_x = min_x.min(display_w);
        let min_y = min_y.min(display_h);
        let max_x = max_x.min(display_w);
        let max_y = max_y.min(display_h);
        Rect {
            x: min_x,
            y: min_y,
            width: max_x.saturating_sub(min_x).max(1),
            height: max_y.saturating_sub(min_y).max(1),
        }
    } else {
        Rect { x: 0, y: 0, width: display_w, height: display_h }
    }
}
fn present_scanout_damage(display: &mut dyn Display, rect: Rect) -> u8 {
    match display.scan_out(rect) {
        Ok(()) => STATUS_OK,
        Err(e) => {
            let _ = debug_println("gpud: present scanout damage FAIL");
            match e {
                GfxError::InvalidArgument => {
                    let _ = debug_println("gpud: scanout InvalidArgument (no scanout resource?)");
                }
                GfxError::ResourceExhausted => {
                    let _ = debug_println("gpud: scanout ResourceExhausted");
                }
                _ => {}
            }
            STATUS_DEVICE_ERROR
        }
    }
}
pub(crate) fn handle_present_damage(display: &mut dyn Display, frame: &[u8]) -> u8 {
    let Some((x, y, width, height)) = nexus_display_proto::decode_damage_frame(frame) else {
        return STATUS_MALFORMED;
    };
    present_scanout_damage(display, Rect { x, y, width, height })
}
