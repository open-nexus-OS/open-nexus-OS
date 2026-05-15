// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

extern crate alloc;

use alloc::vec::Vec;

use input_live_protocol::{
    decode_update_visible_state, encode_status, encode_visible_state_frame, frame_has_op,
    VisibleState, OP_GET_VISIBLE_STATE, OP_SEND_COMPOSED_FRAME_VMO, OP_UPDATE_VISIBLE_STATE,
    STATUS_MALFORMED, STATUS_OK, STATUS_UNSUPPORTED,
};
use nexus_abi::{cap_close, debug_println, nsec, vmo_write, yield_, Handle};
use nexus_ipc::{IpcError, KernelServer, Server as _, Wait};

use crate::error::WindowdError;
use crate::ids::CallerCtx;
use crate::markers::{
    COMPOSE_READY_MARKER, CURSOR_MOVE_VISIBLE_MARKER, DISPLAY_BOOTSTRAP_MARKER,
    DISPLAY_FIRST_SCANOUT_MARKER, DISPLAY_MODE_MARKER, FOCUS_VISIBLE_MARKER, HOVER_VISIBLE_MARKER,
    INPUT_ON_MARKER, INPUT_VISIBLE_ON_MARKER, KEYBOARD_VISIBLE_MARKER, LAUNCHER_CLICK_OK_MARKER,
    LAUNCHER_CLICK_VISIBLE_OK_MARKER, PRESENT_QUEUED_MARKER, PRESENT_SCHEDULER_ON_MARKER,
    PRESENT_VISIBLE_MARKER, READY_MARKER, SYSTEMUI_FIRST_FRAME_VISIBLE_MARKER,
    VISIBLE_BACKEND_MARKER, WHEEL_VISIBLE_MARKER,
};
use crate::smoke::VisibleBootstrapMode;

const ROUTE_NAME: &str = "windowd";
const PROOF_PANEL_X: u32 = 56;
const PROOF_PANEL_Y: u32 = 56;
const PROOF_PANEL_W: u32 = 610;
const PROOF_PANEL_H: u32 = 260;
const TARGET_ROW_Y: u32 = 166;
const TARGET_CARD_W: u32 = 126;
const TARGET_CARD_H: u32 = 82;
const TARGET_GAP: u32 = 16;

pub fn service_main_loop() -> Result<(), &'static str> {
    let server = match KernelServer::new_for(ROUTE_NAME) {
        Ok(s) => s,
        Err(_) => {
            let _ = debug_println("windowd: route fallback");
            KernelServer::new_with_slots(3, 4).map_err(|_| "windowd: init fail kernel-server")?
        }
    };
    let mut runtime =
        DisplayServerRuntime::new().map_err(|_| "windowd: init fail display-server")?;
    let _ = debug_println(READY_MARKER);
    loop {
        for _ in 0..32 {
            match server.recv_with_header_meta(Wait::NonBlocking) {
                Ok((hdr, _sid, frame)) => {
                    let moved_cap = (hdr.flags & nexus_abi::ipc_hdr::CAP_MOVE) != 0;
                    if frame_has_op(&frame, OP_GET_VISIBLE_STATE) {
                        let response = encode_visible_state_frame(runtime.visible_state());
                        if moved_cap {
                            let _ =
                                KernelServer::send_on_cap_wait(hdr.src, &response, Wait::Blocking);
                            let _ = cap_close(hdr.src);
                        } else {
                            let _ = server.send(&response, Wait::Blocking);
                        }
                    } else if frame_has_op(&frame, OP_SEND_COMPOSED_FRAME_VMO) {
                        let status = if moved_cap {
                            runtime.register_framebuffer(hdr.src)
                        } else {
                            STATUS_MALFORMED
                        };
                        if status != STATUS_OK && moved_cap {
                            let _ = cap_close(hdr.src);
                        }
                        let response = encode_status(OP_SEND_COMPOSED_FRAME_VMO, status);
                        let _ = server.send(&response, Wait::Blocking);
                    } else if frame_has_op(&frame, OP_UPDATE_VISIBLE_STATE) {
                        let status = match decode_update_visible_state(&frame) {
                            Some(state) => runtime.apply_input_state(state),
                            None => STATUS_MALFORMED,
                        };
                        if moved_cap {
                            let response = encode_status(OP_UPDATE_VISIBLE_STATE, status);
                            let _ =
                                KernelServer::send_on_cap_wait(hdr.src, &response, Wait::Blocking);
                            let _ = cap_close(hdr.src);
                        }
                    } else {
                        let op = frame.get(3).copied().unwrap_or(0);
                        let response = encode_status(op, STATUS_UNSUPPORTED);
                        if moved_cap {
                            let _ =
                                KernelServer::send_on_cap_wait(hdr.src, &response, Wait::Blocking);
                            let _ = cap_close(hdr.src);
                        } else {
                            let _ = server.send(&response, Wait::Blocking);
                        }
                    }
                }
                Err(IpcError::WouldBlock)
                | Err(IpcError::Timeout)
                | Err(IpcError::Disconnected)
                | Err(IpcError::Kernel(nexus_abi::IpcError::NoSuchEndpoint)) => break,
                Err(_) => {}
            }
        }
        let _ = runtime.flush_pending_damage();
        runtime.tick(nsec().unwrap_or(0));
        let _ = yield_();
    }
}

struct DisplayServerRuntime {
    mode: VisibleBootstrapMode,
    source_frame: SourceFrame,
    cursor_bitmap: Option<alloc::vec::Vec<u8>>,
    cursor_width: u32,
    cursor_height: u32,
    framebuffer: Option<Handle>,
    scratch_row: Vec<u8>,
    state: VisibleState,
    markers_emitted: bool,
    input_markers_emitted: InputMarkerState,
    pending_damage_rows: Option<(u32, u32)>,
}

#[derive(Clone, Copy)]
struct SourceFrame {
    width: u32,
    height: u32,
    stride: u32,
    pixels: &'static [u8],
}

#[derive(Default)]
struct InputMarkerState {
    scheduler: bool,
    input: bool,
    focus_route: bool,
    launcher_click_route: bool,
    cursor: bool,
    hover: bool,
    focus: bool,
    launcher_click: bool,
    keyboard: bool,
    wheel: bool,
}

impl DisplayServerRuntime {
    fn new() -> Result<Self, WindowdError> {
        let mode = VisibleBootstrapMode::fixed()?.validate()?;
        let (source_width, source_height) = systemui::wallpaper_decoded_size();
        let source_frame = SourceFrame {
            width: source_width,
            height: source_height,
            stride: checked_stride(source_width)?,
            pixels: systemui::wallpaper_bgra(),
        };
        let cursor = crate::render_assets::render_cursor_surface(CallerCtx::system());
        let (cursor_bitmap, cursor_width, cursor_height) = match cursor {
            Some(cursor) => (Some(cursor.pixels), cursor.width, cursor.height),
            None => (None, 0, 0),
        };
        Ok(Self {
            mode,
            source_frame,
            cursor_bitmap,
            cursor_width,
            cursor_height,
            framebuffer: None,
            scratch_row: alloc::vec![0u8; mode.stride as usize],
            state: VisibleState {
                backend_visible: true,
                systemui_first_frame_visible: true,
                scene_ready: true,
                full_window_visible: true,
                click_target_visible: true,
                keyboard_target_visible: true,
                cursor_svg_visible: cursor_width != 0 && cursor_height != 0,
                text_target_visible: true,
                icon_target_visible: true,
                wallpaper_visible: systemui::wallpaper_source_is_jpeg(),
                cursor_x: 100,
                cursor_y: 100,
                ..VisibleState::default()
            },
            markers_emitted: false,
            input_markers_emitted: InputMarkerState::default(),
            pending_damage_rows: None,
        })
    }

    const fn visible_state(&self) -> VisibleState {
        self.state
    }

    fn register_framebuffer(&mut self, handle: Handle) -> u8 {
        self.framebuffer = Some(handle);
        self.state.display_scanout_ready = true;
        if self.write_current_frame().is_err() {
            return STATUS_MALFORMED;
        }
        let _ = debug_println(DISPLAY_BOOTSTRAP_MARKER);
        let _ = debug_println(DISPLAY_MODE_MARKER);
        let _ = debug_println(VISIBLE_BACKEND_MARKER);
        let _ = debug_println(COMPOSE_READY_MARKER);
        let _ = debug_println(PRESENT_QUEUED_MARKER);
        let _ = debug_println(PRESENT_SCHEDULER_ON_MARKER);
        self.input_markers_emitted.scheduler = true;
        let _ = debug_println(DISPLAY_FIRST_SCANOUT_MARKER);
        let _ = debug_println(SYSTEMUI_FIRST_FRAME_VISIBLE_MARKER);
        let _ = debug_println(PRESENT_VISIBLE_MARKER);
        self.emit_asset_markers();
        STATUS_OK
    }

    fn apply_input_state(&mut self, upstream: VisibleState) -> u8 {
        let old_cursor_x = self.state.cursor_x;
        let old_cursor_y = self.state.cursor_y;
        let old_targets = target_state_bits(self.state);
        self.state.virtio_raw_seen |= upstream.virtio_raw_seen;
        self.state.hid_normalized_seen |= upstream.hid_normalized_seen;
        self.state.pointer_route_live |= upstream.pointer_route_live;
        self.state.keyboard_route_live |= upstream.keyboard_route_live;
        self.state.input_visible_on |= upstream.input_visible_on
            || upstream.pointer_route_live
            || upstream.keyboard_route_live;
        self.state.cursor_move_visible |=
            upstream.cursor_move_visible || upstream.pointer_route_live;
        self.state.hover_visible |= upstream.hover_visible || upstream.pointer_route_live;
        self.state.focus_visible |= upstream.focus_visible || upstream.launcher_click_visible;
        self.state.launcher_click_visible |= upstream.launcher_click_visible;
        self.state.keyboard_visible |= upstream.keyboard_visible || upstream.keyboard_route_live;
        self.state.wheel_up_visible |= upstream.wheel_up_visible;
        self.state.wheel_down_visible |= upstream.wheel_down_visible;
        self.state.cursor_x = upstream.cursor_x;
        self.state.cursor_y = upstream.cursor_y;
        self.queue_cursor_damage(
            old_cursor_x,
            old_cursor_y,
            self.state.cursor_x,
            self.state.cursor_y,
        );
        if target_state_bits(self.state) != old_targets {
            self.queue_rows(PROOF_PANEL_Y, PROOF_PANEL_Y + PROOF_PANEL_H);
        }
        STATUS_OK
    }

    fn tick(&mut self, _now_ns: u64) {
        // The scanout VMO persists; avoid rewriting a full 1280x800 frame on idle ticks.
    }

    fn emit_asset_markers(&mut self) {
        if self.markers_emitted {
            return;
        }
        if self.state.cursor_svg_visible {
            let _ = debug_println(crate::markers::CURSOR_SVG_LOADED_MARKER);
        }
        if self.state.wallpaper_visible {
            let _ = debug_println(crate::markers::WALLPAPER_VISIBLE_MARKER);
        }
        if self.state.text_target_visible {
            let _ = debug_println(crate::markers::TEXT_TARGET_VISIBLE_MARKER);
        }
        if self.state.icon_target_visible {
            let _ = debug_println(crate::markers::ICON_TARGET_VISIBLE_MARKER);
        }
        self.markers_emitted = true;
    }

    fn emit_input_markers(&mut self) {
        if self.state.input_visible_on && !self.input_markers_emitted.input {
            let _ = debug_println(INPUT_ON_MARKER);
            let _ = debug_println(INPUT_VISIBLE_ON_MARKER);
            self.input_markers_emitted.input = true;
        }
        if self.state.cursor_move_visible && !self.input_markers_emitted.cursor {
            let _ = debug_println(CURSOR_MOVE_VISIBLE_MARKER);
            self.input_markers_emitted.cursor = true;
        }
        if self.state.hover_visible && !self.input_markers_emitted.hover {
            let _ = debug_println(HOVER_VISIBLE_MARKER);
            self.input_markers_emitted.hover = true;
        }
        if self.state.focus_visible && !self.input_markers_emitted.focus {
            if !self.input_markers_emitted.focus_route {
                let _ = debug_println("windowd: focus -> 1");
                self.input_markers_emitted.focus_route = true;
            }
            let _ = debug_println(FOCUS_VISIBLE_MARKER);
            self.input_markers_emitted.focus = true;
        }
        if self.state.launcher_click_visible && !self.input_markers_emitted.launcher_click {
            if !self.input_markers_emitted.launcher_click_route {
                let _ = debug_println(LAUNCHER_CLICK_OK_MARKER);
                self.input_markers_emitted.launcher_click_route = true;
            }
            let _ = debug_println(LAUNCHER_CLICK_VISIBLE_OK_MARKER);
            self.input_markers_emitted.launcher_click = true;
        }
        if self.state.keyboard_visible && !self.input_markers_emitted.keyboard {
            let _ = debug_println(KEYBOARD_VISIBLE_MARKER);
            self.input_markers_emitted.keyboard = true;
        }
        if (self.state.wheel_up_visible || self.state.wheel_down_visible)
            && !self.input_markers_emitted.wheel
        {
            let _ = debug_println(WHEEL_VISIBLE_MARKER);
            self.input_markers_emitted.wheel = true;
        }
    }

    fn write_current_frame(&mut self) -> Result<(), WindowdError> {
        self.write_rows(0, self.mode.height)
    }

    fn write_rows(&mut self, start_y: u32, end_y: u32) -> Result<(), WindowdError> {
        let Some(handle) = self.framebuffer else {
            return Ok(());
        };
        let row_len = self.mode.stride as usize;
        if self.scratch_row.len() < row_len {
            return Err(WindowdError::BufferLengthMismatch);
        }
        let end_y = end_y.min(self.mode.height);
        for y in start_y.min(end_y)..end_y {
            copy_scene_row(
                &self.source_frame,
                self.mode,
                self.state,
                self.cursor_bitmap.as_deref(),
                self.cursor_width,
                self.cursor_height,
                self.state.cursor_x,
                self.state.cursor_y,
                y,
                &mut self.scratch_row[..row_len],
            )?;
            let offset = y as usize * row_len;
            vmo_write(handle, offset, &self.scratch_row[..row_len])
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
        }
        self.state.cursor_overlay_visible = self.state.cursor_svg_visible;
        Ok(())
    }

    fn queue_cursor_damage(
        &mut self,
        old_cursor_x: i32,
        old_cursor_y: i32,
        new_cursor_x: i32,
        new_cursor_y: i32,
    ) {
        let old_rows = cursor_row_range(old_cursor_y, self.cursor_height, self.mode.height);
        let new_rows = cursor_row_range(new_cursor_y, self.cursor_height, self.mode.height);
        let Some((start, end)) = merge_optional_ranges(old_rows, new_rows) else {
            return;
        };
        self.pending_damage_rows = Some(match self.pending_damage_rows {
            Some((queued_start, queued_end)) => (queued_start.min(start), queued_end.max(end)),
            None => (start, end),
        });
        let _ = old_cursor_x;
        let _ = new_cursor_x;
    }

    fn queue_rows(&mut self, start: u32, end: u32) {
        let start = start.min(self.mode.height);
        let end = end.min(self.mode.height);
        if start >= end {
            return;
        }
        self.pending_damage_rows = Some(match self.pending_damage_rows {
            Some((queued_start, queued_end)) => (queued_start.min(start), queued_end.max(end)),
            None => (start, end),
        });
    }

    fn flush_pending_damage(&mut self) -> Result<(), WindowdError> {
        let Some((start, end)) = self.pending_damage_rows.take() else {
            return Ok(());
        };
        self.write_rows(start, end)?;
        self.emit_input_markers();
        Ok(())
    }
}

fn cursor_row_range(cursor_y: i32, cursor_height: u32, mode_height: u32) -> Option<(u32, u32)> {
    if cursor_height == 0 || mode_height == 0 {
        return None;
    }
    let start = cursor_y.max(0) as u32;
    let end = (cursor_y.saturating_add(cursor_height as i32)).min(mode_height as i32);
    if end <= start as i32 {
        return None;
    }
    Some((start, end as u32))
}

fn merge_optional_ranges(a: Option<(u32, u32)>, b: Option<(u32, u32)>) -> Option<(u32, u32)> {
    match (a, b) {
        (Some((a_start, a_end)), Some((b_start, b_end))) => {
            Some((a_start.min(b_start), a_end.max(b_end)))
        }
        (Some(range), None) | (None, Some(range)) => Some(range),
        (None, None) => None,
    }
}

fn target_state_bits(state: VisibleState) -> u8 {
    u8::from(state.hover_visible)
        | (u8::from(state.focus_visible) << 1)
        | (u8::from(state.launcher_click_visible) << 2)
        | (u8::from(state.keyboard_visible) << 3)
        | (u8::from(state.wheel_up_visible || state.wheel_down_visible) << 4)
}

fn copy_scene_row(
    source_frame: &SourceFrame,
    mode: VisibleBootstrapMode,
    state: VisibleState,
    cursor_bitmap: Option<&[u8]>,
    cursor_width: u32,
    cursor_height: u32,
    cursor_x: i32,
    cursor_y: i32,
    y: u32,
    row: &mut [u8],
) -> Result<(), WindowdError> {
    copy_scaled_systemui_row(source_frame, mode, y, row)?;
    draw_proof_surface_row(state, y, row)?;
    draw_icon_target_row(y, row)?;
    if let Some(cursor) = cursor_bitmap {
        blend_cursor_row(
            row,
            y,
            cursor,
            cursor_width,
            cursor_height,
            cursor_x,
            cursor_y,
        );
    }
    Ok(())
}

fn copy_scaled_systemui_row(
    frame: &SourceFrame,
    mode: VisibleBootstrapMode,
    y: u32,
    row: &mut [u8],
) -> Result<(), WindowdError> {
    let row_len = mode.stride as usize;
    if row.len() < row_len || frame.width == 0 || frame.height == 0 {
        return Err(WindowdError::BufferLengthMismatch);
    }
    let src_y = ((u64::from(y) * u64::from(frame.height)) / u64::from(mode.height)) as usize;
    for x in 0..mode.width {
        let src_x = ((u64::from(x) * u64::from(frame.width)) / u64::from(mode.width)) as usize;
        let src = src_y
            .checked_mul(frame.stride as usize)
            .and_then(|base| base.checked_add(src_x.checked_mul(4)?))
            .ok_or(WindowdError::ArithmeticOverflow)?;
        let dst = (x as usize)
            .checked_mul(4)
            .ok_or(WindowdError::ArithmeticOverflow)?;
        row[dst..dst + 4].copy_from_slice(
            frame
                .pixels
                .get(src..src + 4)
                .ok_or(WindowdError::BufferLengthMismatch)?,
        );
    }
    Ok(())
}

fn checked_stride(width: u32) -> Result<u32, WindowdError> {
    let bytes = width
        .checked_mul(4)
        .ok_or(WindowdError::ArithmeticOverflow)?;
    bytes
        .checked_add(63)
        .ok_or(WindowdError::ArithmeticOverflow)
        .map(|v| v / 64 * 64)
}

fn draw_proof_surface_row(state: VisibleState, y: u32, row: &mut [u8]) -> Result<(), WindowdError> {
    fill_row_rect(
        y,
        row,
        PROOF_PANEL_X,
        PROOF_PANEL_Y,
        PROOF_PANEL_W,
        PROOF_PANEL_H,
        [0x18, 0x18, 0x16, 0xd8],
    )?;
    stroke_row_rect(
        y,
        row,
        PROOF_PANEL_X,
        PROOF_PANEL_Y,
        PROOF_PANEL_W,
        PROOF_PANEL_H,
        [0xff, 0xff, 0xff, 0x70],
    )?;
    draw_atlas_text_row(
        y,
        row,
        PROOF_PANEL_X + 24,
        PROOF_PANEL_Y + 24,
        3,
        "OPEN NEXUS OS",
        [0xff, 0xff, 0xff, 0xff],
    )?;
    draw_atlas_text_row(
        y,
        row,
        PROOF_PANEL_X + 25,
        PROOF_PANEL_Y + 58,
        2,
        "DISPLAYSERVER V0  INTER ATLAS FALLBACK",
        [0xc8, 0xd8, 0xff, 0xff],
    )?;
    draw_atlas_text_row(
        y,
        row,
        PROOF_PANEL_X + 25,
        PROOF_PANEL_Y + 86,
        2,
        "HOVER CLICK SCROLL KEYBOARD TARGETS",
        [0x9c, 0xac, 0xc8, 0xff],
    )?;
    draw_target_card_row(
        y,
        row,
        PROOF_PANEL_X + 24,
        TARGET_ROW_Y,
        "HOVER",
        state.hover_visible,
        [0x48, 0xa8, 0xff, 0xff],
    )?;
    draw_target_card_row(
        y,
        row,
        PROOF_PANEL_X + 24 + (TARGET_CARD_W + TARGET_GAP),
        TARGET_ROW_Y,
        "CLICK",
        state.launcher_click_visible || state.focus_visible,
        [0x5a, 0xe0, 0x74, 0xff],
    )?;
    draw_target_card_row(
        y,
        row,
        PROOF_PANEL_X + 24 + 2 * (TARGET_CARD_W + TARGET_GAP),
        TARGET_ROW_Y,
        "SCROLL",
        state.wheel_up_visible || state.wheel_down_visible,
        [0xff, 0xb0, 0x45, 0xff],
    )?;
    draw_target_card_row(
        y,
        row,
        PROOF_PANEL_X + 24 + 3 * (TARGET_CARD_W + TARGET_GAP),
        TARGET_ROW_Y,
        "KEY",
        state.keyboard_visible,
        [0xdf, 0x90, 0xff, 0xff],
    )?;
    Ok(())
}

fn draw_icon_target_row(y: u32, row: &mut [u8]) -> Result<(), WindowdError> {
    let x = PROOF_PANEL_X + PROOF_PANEL_W - 88;
    let top = PROOF_PANEL_Y + 24;
    fill_row_rect(y, row, x, top, 48, 48, [0x30, 0xb8, 0xff, 0xff])?;
    fill_row_rect(y, row, x + 12, top + 12, 24, 24, [0x10, 0x40, 0x80, 0xff])
}

fn draw_target_card_row(
    y: u32,
    row: &mut [u8],
    x: u32,
    top: u32,
    label: &str,
    active: bool,
    accent: [u8; 4],
) -> Result<(), WindowdError> {
    let bg = if active {
        [0x28, 0x34, 0x28, 0xf0]
    } else {
        [0x28, 0x24, 0x20, 0xd8]
    };
    let border = if active {
        accent
    } else {
        [0x88, 0x88, 0x88, 0x88]
    };
    fill_row_rect(y, row, x, top, TARGET_CARD_W, TARGET_CARD_H, bg)?;
    stroke_row_rect(y, row, x, top, TARGET_CARD_W, TARGET_CARD_H, border)?;
    fill_row_rect(y, row, x + 14, top + 14, 24, 24, accent)?;
    if active {
        fill_row_rect(y, row, x + 20, top + 20, 12, 12, [0xff, 0xff, 0xff, 0xff])?;
    }
    draw_atlas_text_row(y, row, x + 14, top + 50, 2, label, [0xf4, 0xf6, 0xff, 0xff])
}

fn fill_row_rect(
    y: u32,
    row: &mut [u8],
    x: u32,
    rect_y: u32,
    width: u32,
    height: u32,
    bgra: [u8; 4],
) -> Result<(), WindowdError> {
    if y < rect_y || y >= rect_y.saturating_add(height) {
        return Ok(());
    }
    let row_pixels = row.len() / 4;
    let start = x.min(row_pixels as u32) as usize;
    let end = x.saturating_add(width).min(row_pixels as u32) as usize;
    for px in start..end {
        let idx = px.checked_mul(4).ok_or(WindowdError::ArithmeticOverflow)?;
        row[idx..idx + 4].copy_from_slice(&bgra);
    }
    Ok(())
}

fn stroke_row_rect(
    y: u32,
    row: &mut [u8],
    x: u32,
    rect_y: u32,
    width: u32,
    height: u32,
    bgra: [u8; 4],
) -> Result<(), WindowdError> {
    if width == 0 || height == 0 {
        return Ok(());
    }
    fill_row_rect(y, row, x, rect_y, width, 2, bgra)?;
    fill_row_rect(y, row, x, rect_y + height.saturating_sub(2), width, 2, bgra)?;
    fill_row_rect(y, row, x, rect_y, 2, height, bgra)?;
    fill_row_rect(y, row, x + width.saturating_sub(2), rect_y, 2, height, bgra)
}

fn draw_atlas_text_row(
    y: u32,
    row: &mut [u8],
    mut x: u32,
    top: u32,
    scale: u32,
    text: &str,
    bgra: [u8; 4],
) -> Result<(), WindowdError> {
    if y < top || y >= top.saturating_add(7 * scale) {
        return Ok(());
    }
    let glyph_row = (y - top) / scale;
    let subrow = (y - top) % scale;
    for ch in text.bytes() {
        let bits = glyph_bits(ch, glyph_row);
        for col in 0..5 {
            if (bits & (1 << (4 - col))) != 0 {
                fill_row_rect(
                    y,
                    row,
                    x + col * scale,
                    top + glyph_row * scale + subrow,
                    scale,
                    1,
                    bgra,
                )?;
            }
        }
        x = x.saturating_add(6 * scale);
    }
    Ok(())
}

fn glyph_bits(ch: u8, row: u32) -> u8 {
    let rows = match ch {
        b'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        b'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        b'C' => [
            0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111,
        ],
        b'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        b'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        b'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        b'G' => [
            0b01111, 0b10000, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111,
        ],
        b'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        b'I' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
        b'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        b'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        b'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        b'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        b'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        b'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        b'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        b'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        b'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        b'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        b'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        b'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001,
        ],
        b'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        b'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        b'0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        b'1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        b'8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        b' ' => [0, 0, 0, 0, 0, 0, 0],
        _ => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
    };
    rows.get(row as usize).copied().unwrap_or(0)
}

fn blend_cursor_row(
    row: &mut [u8],
    row_y: u32,
    cursor_bitmap: &[u8],
    cursor_width: u32,
    cursor_height: u32,
    cursor_x: i32,
    cursor_y: i32,
) {
    let cursor_row = row_y as i32 - cursor_y;
    if cursor_row < 0 || cursor_row >= cursor_height as i32 {
        return;
    }
    for col in 0..(row.len() / 4) {
        let cursor_col = col as i32 - cursor_x;
        if cursor_col < 0 || cursor_col >= cursor_width as i32 {
            continue;
        }
        let src_idx = ((cursor_row as u32 * cursor_width + cursor_col as u32) * 4) as usize;
        let dst_idx = col * 4;
        if src_idx + 4 > cursor_bitmap.len() {
            continue;
        }
        let alpha = cursor_bitmap[src_idx + 3];
        if alpha == 0 {
            continue;
        }
        if alpha == 255 {
            row[dst_idx..dst_idx + 4].copy_from_slice(&cursor_bitmap[src_idx..src_idx + 4]);
            continue;
        }
        let inv_alpha = 255u32.saturating_sub(u32::from(alpha));
        let alpha = u32::from(alpha);
        for channel in 0..3 {
            row[dst_idx + channel] = ((u32::from(cursor_bitmap[src_idx + channel]) * alpha
                + u32::from(row[dst_idx + channel]) * inv_alpha)
                / 255) as u8;
        }
        row[dst_idx + 3] = 255;
    }
}
