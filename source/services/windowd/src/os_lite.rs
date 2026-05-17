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
    LAUNCHER_CLICK_VISIBLE_OK_MARKER, LAYOUT_ENGINE_ON_MARKER, PRESENT_QUEUED_MARKER,
    PRESENT_SCHEDULER_ON_MARKER, PRESENT_VISIBLE_MARKER, READY_MARKER,
    SYSTEMUI_FIRST_FRAME_VISIBLE_MARKER, TEXT_WRAPPING_ON_MARKER, VISIBLE_BACKEND_MARKER,
    WHEEL_VISIBLE_MARKER,
};
use nexus_layout::LayoutResult;
use nexus_layout_types::Rgba8;

use crate::layout_panel;
use crate::smoke::VisibleBootstrapMode;

const ROUTE_NAME: &str = "windowd";
const PROOF_PANEL_X: u32 = 56;
const PROOF_PANEL_Y: u32 = 440;
const PROOF_PANEL_W: u32 = crate::proof_panel_spec::PANEL_WIDTH as u32;
const PROOF_PANEL_H: u32 = crate::proof_panel_spec::PANEL_HEIGHT as u32;

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
    input_state_debug_emitted: bool,
    pending_damage_rows: Option<(u32, u32)>,
    proof_layout: Option<LayoutResult>,
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
            input_state_debug_emitted: false,
            pending_damage_rows: None,
            proof_layout: layout_panel::compute_proof_layout(VisibleState {
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
            })
            .ok(),
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
        if self.proof_layout.is_some() {
            let _ = debug_println(LAYOUT_ENGINE_ON_MARKER);
            let _ = debug_println(TEXT_WRAPPING_ON_MARKER);
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
        if !self.input_state_debug_emitted {
            let _ = debug_println("dbg: windowd input state applied");
            self.input_state_debug_emitted = true;
        }
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
        self.state.hover_visible = upstream.hover_visible;
        self.state.focus_visible = upstream.focus_visible;
        self.state.launcher_click_visible = upstream.launcher_click_visible;
        self.state.keyboard_visible = upstream.keyboard_visible;
        self.state.wheel_up_visible = upstream.wheel_up_visible;
        self.state.wheel_down_visible = upstream.wheel_down_visible;
        self.state.cursor_x = upstream.cursor_x;
        self.state.cursor_y = upstream.cursor_y;
        let new_targets = target_state_bits(self.state);
        if new_targets != old_targets {
            // Target state only changes paint, not geometry. Keep the live path
            // allocation-free for OS services running on the bump allocator.
            self.queue_rows(PROOF_PANEL_Y, PROOF_PANEL_Y + PROOF_PANEL_H);
        }
        self.queue_cursor_damage(
            old_cursor_x,
            old_cursor_y,
            self.state.cursor_x,
            self.state.cursor_y,
        );
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
                self.proof_layout.as_ref(),
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
        let old_rows = cursor_row_range(
            old_cursor_y - crate::assets::CURSOR_HOTSPOT_Y,
            self.cursor_height,
            self.mode.height,
        );
        let new_rows = cursor_row_range(
            new_cursor_y - crate::assets::CURSOR_HOTSPOT_Y,
            self.cursor_height,
            self.mode.height,
        );
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
        | (u8::from(state.launcher_click_visible) << 1)
        | (u8::from(state.keyboard_visible) << 2)
        | (u8::from(state.wheel_up_visible) << 3)
        | (u8::from(state.wheel_down_visible) << 4)
}

fn copy_scene_row(
    source_frame: &SourceFrame,
    mode: VisibleBootstrapMode,
    state: VisibleState,
    proof_layout: Option<&LayoutResult>,
    cursor_bitmap: Option<&[u8]>,
    cursor_width: u32,
    cursor_height: u32,
    cursor_x: i32,
    cursor_y: i32,
    y: u32,
    row: &mut [u8],
) -> Result<(), WindowdError> {
    copy_scaled_systemui_row(source_frame, mode, y, row)?;
    draw_proof_surface_row(state, proof_layout, y, row)?;
    if let Some(cursor) = cursor_bitmap {
        blend_cursor_row(
            row,
            y,
            cursor,
            cursor_width,
            cursor_height,
            cursor_x - crate::assets::CURSOR_HOTSPOT_X,
            cursor_y - crate::assets::CURSOR_HOTSPOT_Y,
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
        let dst = (x as usize).checked_mul(4).ok_or(WindowdError::ArithmeticOverflow)?;
        row[dst..dst + 4].copy_from_slice(
            frame.pixels.get(src..src + 4).ok_or(WindowdError::BufferLengthMismatch)?,
        );
    }
    Ok(())
}

fn checked_stride(width: u32) -> Result<u32, WindowdError> {
    let bytes = width.checked_mul(4).ok_or(WindowdError::ArithmeticOverflow)?;
    bytes.checked_add(63).ok_or(WindowdError::ArithmeticOverflow).map(|v| v / 64 * 64)
}

fn draw_proof_surface_row(
    state: VisibleState,
    proof_layout: Option<&LayoutResult>,
    y: u32,
    row: &mut [u8],
) -> Result<(), WindowdError> {
    let Some(layout) = proof_layout else {
        return Ok(());
    };
    for layout_box in &layout.boxes {
        let Some(rect) = proof_box_rect(layout_box) else {
            continue;
        };
        if !rect.contains_y(y) {
            continue;
        }
        let paint_role = layout_box.id.and_then(proof_paint_role);
        draw_layout_box_row(state, y, row, layout_box, rect, paint_role)?;
        if let Some(id) = layout_box.id {
            if let Some(asset) = crate::assets::proof_text_asset(id) {
                blend_asset_row(y, row, rect.x, rect.y, asset.width, asset.height, asset.bgra)?;
            }
        }
    }
    Ok(())
}

fn draw_layout_box_row(
    state: VisibleState,
    y: u32,
    row: &mut [u8],
    layout_box: &nexus_layout::LayoutBox,
    rect: ProofBoxRect,
    paint_role: Option<ProofPaintRole>,
) -> Result<(), WindowdError> {
    match &layout_box.visual.shape {
        nexus_layout_types::ShapeKind::Rect => {
            if let Some(background) = proof_box_background(layout_box, state, paint_role) {
                fill_row_rect(
                    y,
                    row,
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    rgba_to_bgra(background),
                )?;
            }
            if let Some((border_width, border_color)) =
                proof_box_border(layout_box, state, paint_role)
            {
                stroke_row_rect_width(
                    y,
                    row,
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    border_width,
                    rgba_to_bgra(border_color),
                )?;
            }
        }
        nexus_layout_types::ShapeKind::Circle => {
            if let Some(background) = proof_box_background(layout_box, state, paint_role) {
                fill_circle_row(
                    y,
                    row,
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    rgba_to_bgra(background),
                )?;
            }
            if let Some((border_width, border_color)) =
                proof_box_border(layout_box, state, paint_role)
            {
                stroke_circle_row(
                    y,
                    row,
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    border_width,
                    rgba_to_bgra(border_color),
                )?;
            }
        }
        nexus_layout_types::ShapeKind::TriangleUp => {
            if let Some(background) = proof_box_background(layout_box, state, paint_role) {
                fill_triangle_row(
                    y,
                    row,
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    true,
                    rgba_to_bgra(background),
                )?;
            }
        }
        nexus_layout_types::ShapeKind::TriangleDown => {
            if let Some(background) = proof_box_background(layout_box, state, paint_role) {
                fill_triangle_row(
                    y,
                    row,
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    false,
                    rgba_to_bgra(background),
                )?;
            }
        }
        nexus_layout_types::ShapeKind::Path(path) => {
            let color = proof_box_border(layout_box, state, paint_role)
                .map(|(_, color)| rgba_to_bgra(color))
                .or_else(|| proof_box_background(layout_box, state, paint_role).map(rgba_to_bgra))
                .unwrap_or([0xff, 0xff, 0xff, 0xff]);
            draw_path_row(y, row, rect.x, rect.y, rect.width, rect.height, path, color)?;
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct ProofBoxRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl ProofBoxRect {
    fn contains_y(self, y: u32) -> bool {
        y >= self.y && y < self.y.saturating_add(self.height)
    }
}

fn proof_box_rect(layout_box: &nexus_layout::LayoutBox) -> Option<ProofBoxRect> {
    let width = layout_box.rect.width.as_u32().unwrap_or(0);
    let height = layout_box.rect.height.as_u32().unwrap_or(0);
    if width == 0 || height == 0 {
        return None;
    }
    Some(ProofBoxRect {
        x: PROOF_PANEL_X + layout_box.rect.x.as_u32().unwrap_or(0),
        y: PROOF_PANEL_Y + layout_box.rect.y.as_u32().unwrap_or(0),
        width,
        height,
    })
}

fn proof_box_background(
    layout_box: &nexus_layout::LayoutBox,
    state: VisibleState,
    paint_role: Option<ProofPaintRole>,
) -> Option<Rgba8> {
    let Some(role) = paint_role else {
        return layout_box.visual.background;
    };
    let card = role.card.paint(state);
    match role.part {
        ProofPaintPart::Root => Some(if card.active {
            crate::assets::PROOF_CARD_ACTIVE_BG
        } else {
            crate::assets::PROOF_CARD_BG
        }),
        ProofPaintPart::Icon => Some(card.accent),
        ProofPaintPart::Dot => Some(if card.active {
            crate::assets::PROOF_ICON_FG
        } else {
            crate::assets::PROOF_CARD_BG
        }),
        ProofPaintPart::Glyph => Some(if card.active {
            crate::assets::PROOF_ICON_FG
        } else {
            crate::assets::PROOF_CARD_BORDER
        }),
        ProofPaintPart::ScrollUp => {
            Some(if state.wheel_up_visible { crate::assets::PROOF_ICON_FG } else { card.accent })
        }
        ProofPaintPart::ScrollDown => Some(if state.wheel_down_visible {
            crate::assets::PROOF_ICON_FG
        } else {
            crate::assets::PROOF_CARD_BORDER
        }),
    }
}

fn proof_box_border(
    layout_box: &nexus_layout::LayoutBox,
    state: VisibleState,
    paint_role: Option<ProofPaintRole>,
) -> Option<(u32, Rgba8)> {
    let border = layout_box.visual.border.top?;
    let width = border.width.as_u32().unwrap_or(1);
    let color = match paint_role {
        Some(ProofPaintRole { card, part: ProofPaintPart::Root | ProofPaintPart::Icon }) => {
            let paint = card.paint(state);
            if paint.active {
                paint.accent
            } else {
                crate::assets::PROOF_CARD_BORDER
            }
        }
        _ => border.color,
    };
    Some((width, color))
}

#[derive(Clone, Copy)]
struct ProofCardPaint {
    active: bool,
    accent: Rgba8,
}

#[derive(Clone, Copy)]
struct ProofPaintRole {
    card: ProofCard,
    part: ProofPaintPart,
}

#[derive(Clone, Copy)]
enum ProofCard {
    Hover,
    Click,
    Scroll,
    Key,
}

impl ProofCard {
    fn paint(self, state: VisibleState) -> ProofCardPaint {
        match self {
            Self::Hover => {
                ProofCardPaint { active: state.hover_visible, accent: crate::assets::PROOF_HOVER }
            }
            Self::Click => ProofCardPaint {
                active: state.launcher_click_visible,
                accent: crate::assets::PROOF_CLICK,
            },
            Self::Scroll => ProofCardPaint {
                active: state.wheel_up_visible || state.wheel_down_visible,
                accent: crate::assets::PROOF_SCROLL,
            },
            Self::Key => ProofCardPaint {
                active: state.keyboard_visible,
                accent: crate::assets::PROOF_KEYBOARD,
            },
        }
    }
}

#[derive(Clone, Copy)]
enum ProofPaintPart {
    Root,
    Icon,
    Dot,
    Glyph,
    ScrollUp,
    ScrollDown,
}

fn proof_paint_role(id: &str) -> Option<ProofPaintRole> {
    use ProofCard::{Click, Hover, Key, Scroll};
    use ProofPaintPart::{Dot, Glyph, Icon, Root, ScrollDown, ScrollUp};

    let (card, part) = match id {
        "card_hover" => (Hover, Root),
        "card_hover_icon" => (Hover, Icon),
        "card_hover_dot" => (Hover, Dot),
        "card_hover_glyph" => (Hover, Glyph),
        "card_click" => (Click, Root),
        "card_click_icon" => (Click, Icon),
        "card_click_dot" => (Click, Dot),
        "card_click_glyph" => (Click, Glyph),
        "card_scroll" => (Scroll, Root),
        "card_scroll_icon" => (Scroll, Icon),
        "card_scroll_dot" => (Scroll, Dot),
        "card_scroll_up" => (Scroll, ScrollUp),
        "card_scroll_down" => (Scroll, ScrollDown),
        "card_key" => (Key, Root),
        "card_key_icon" => (Key, Icon),
        "card_key_dot" => (Key, Dot),
        "card_key_glyph" => (Key, Glyph),
        _ => return None,
    };
    Some(ProofPaintRole { card, part })
}

fn blend_asset_row(
    y: u32,
    row: &mut [u8],
    x: u32,
    top: u32,
    width: u32,
    height: u32,
    source: &[u8],
) -> Result<(), WindowdError> {
    if y < top || y >= top.saturating_add(height) {
        return Ok(());
    }
    let source_y = y - top;
    let src_row = source_y as usize * width as usize * 4;
    let source = source
        .get(src_row..src_row + width as usize * 4)
        .ok_or(WindowdError::BufferLengthMismatch)?;
    blend_overlay_row(row, x as usize, source)
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

fn fill_circle_row(
    y: u32,
    row: &mut [u8],
    x: u32,
    rect_y: u32,
    width: u32,
    height: u32,
    bgra: [u8; 4],
) -> Result<(), WindowdError> {
    if width == 0 || height == 0 || y < rect_y || y >= rect_y.saturating_add(height) {
        return Ok(());
    }
    let local_y = y - rect_y;
    let ry = height as i64;
    let rx = width as i64;
    let cy = 2 * local_y as i64 + 1 - ry;
    let row_pixels = row.len() / 4;
    for px in 0..width {
        let cx = 2 * px as i64 + 1 - rx;
        if cx * cx * ry * ry + cy * cy * rx * rx <= rx * rx * ry * ry {
            let dst_x = x.saturating_add(px);
            if dst_x >= row_pixels as u32 {
                break;
            }
            let idx = dst_x as usize * 4;
            row[idx..idx + 4].copy_from_slice(&bgra);
        }
    }
    Ok(())
}

fn stroke_circle_row(
    y: u32,
    row: &mut [u8],
    x: u32,
    rect_y: u32,
    width: u32,
    height: u32,
    stroke: u32,
    bgra: [u8; 4],
) -> Result<(), WindowdError> {
    if width == 0 || height == 0 || stroke == 0 || y < rect_y || y >= rect_y.saturating_add(height)
    {
        return Ok(());
    }
    let local_y = y - rect_y;
    let ry = height as i64;
    let rx = width as i64;
    let cy = 2 * local_y as i64 + 1 - ry;
    let outer = rx * rx * ry * ry;
    let inner_rx = (width.saturating_sub(2 * stroke)) as i64;
    let inner_ry = (height.saturating_sub(2 * stroke)) as i64;
    let row_pixels = row.len() / 4;
    for px in 0..width {
        let cx = 2 * px as i64 + 1 - rx;
        let inside_outer = cx * cx * ry * ry + cy * cy * rx * rx <= outer;
        let inside_inner = inner_rx > 0
            && inner_ry > 0
            && cx * cx * inner_ry * inner_ry + cy * cy * inner_rx * inner_rx
                <= inner_rx * inner_rx * inner_ry * inner_ry;
        if inside_outer && !inside_inner {
            let dst_x = x.saturating_add(px);
            if dst_x >= row_pixels as u32 {
                break;
            }
            let idx = dst_x as usize * 4;
            row[idx..idx + 4].copy_from_slice(&bgra);
        }
    }
    Ok(())
}

fn fill_triangle_row(
    y: u32,
    row: &mut [u8],
    x: u32,
    rect_y: u32,
    width: u32,
    height: u32,
    up: bool,
    bgra: [u8; 4],
) -> Result<(), WindowdError> {
    if width == 0 || height == 0 || y < rect_y || y >= rect_y.saturating_add(height) {
        return Ok(());
    }
    let local_y = y - rect_y;
    let progress = if up { height.saturating_sub(local_y + 1) } else { local_y };
    let span = ((progress + 1) * width).max(height) / height.max(1);
    let span = span.max(1).min(width);
    let start = x + (width.saturating_sub(span)) / 2;
    fill_row_rect(y, row, start, y, span, 1, bgra)
}

fn draw_path_row(
    y: u32,
    row: &mut [u8],
    x: u32,
    rect_y: u32,
    width: u32,
    height: u32,
    path: &nexus_layout_types::PathShape,
    bgra: [u8; 4],
) -> Result<(), WindowdError> {
    if width == 0
        || height == 0
        || path.points.len() < 2
        || y < rect_y
        || y >= rect_y.saturating_add(height)
    {
        return Ok(());
    }
    for segment in path.points.windows(2) {
        draw_line_segment_row(y, row, x, rect_y, width, height, segment[0], segment[1], bgra)?;
    }
    if path.closed {
        draw_line_segment_row(
            y,
            row,
            x,
            rect_y,
            width,
            height,
            *path.points.last().unwrap_or(&nexus_layout_types::PathPoint::new(0, 0)),
            path.points[0],
            bgra,
        )?;
    }
    Ok(())
}

fn draw_line_segment_row(
    y: u32,
    row: &mut [u8],
    x: u32,
    rect_y: u32,
    width: u32,
    height: u32,
    start: nexus_layout_types::PathPoint,
    end: nexus_layout_types::PathPoint,
    bgra: [u8; 4],
) -> Result<(), WindowdError> {
    let x0 = x + (u32::from(start.x_milli) * width) / 1000;
    let y0 = rect_y + (u32::from(start.y_milli) * height) / 1000;
    let x1 = x + (u32::from(end.x_milli) * width) / 1000;
    let y1 = rect_y + (u32::from(end.y_milli) * height) / 1000;
    let min_y = y0.min(y1);
    let max_y = y0.max(y1);
    if y < min_y || y > max_y {
        return Ok(());
    }
    if y0 == y1 {
        let start_x = x0.min(x1);
        let span = x0.max(x1).saturating_sub(start_x).saturating_add(1);
        return fill_row_rect(y, row, start_x, y, span, 1, bgra);
    }
    let dy = y1 as i64 - y0 as i64;
    let dx = x1 as i64 - x0 as i64;
    let relative = y as i64 - y0 as i64;
    let px = x0 as i64 + dx * relative / dy;
    let px = px.max(0) as u32;
    fill_row_rect(y, row, px.saturating_sub(1), y, 3, 1, bgra)
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
    stroke_row_rect_width(y, row, x, rect_y, width, height, 2, bgra)
}

fn stroke_row_rect_width(
    y: u32,
    row: &mut [u8],
    x: u32,
    rect_y: u32,
    width: u32,
    height: u32,
    stroke: u32,
    bgra: [u8; 4],
) -> Result<(), WindowdError> {
    if width == 0 || height == 0 || stroke == 0 {
        return Ok(());
    }
    let stroke = stroke.min(width).min(height);
    fill_row_rect(y, row, x, rect_y, width, stroke, bgra)?;
    fill_row_rect(y, row, x, rect_y + height.saturating_sub(stroke), width, stroke, bgra)?;
    fill_row_rect(y, row, x, rect_y, stroke, height, bgra)?;
    fill_row_rect(y, row, x + width.saturating_sub(stroke), rect_y, stroke, height, bgra)
}

fn rgba_to_bgra(color: nexus_layout_types::Rgba8) -> [u8; 4] {
    [color.b, color.g, color.r, color.a]
}

fn blend_overlay_row(row: &mut [u8], x: usize, source: &[u8]) -> Result<(), WindowdError> {
    let row_pixels = row.len() / 4;
    for (col, pixel) in source.chunks_exact(4).enumerate() {
        let dst_col = x.saturating_add(col);
        if dst_col >= row_pixels {
            break;
        }
        let alpha = pixel[3];
        if alpha == 0 {
            continue;
        }
        let dst = dst_col.checked_mul(4).ok_or(WindowdError::ArithmeticOverflow)?;
        if alpha == 255 {
            row[dst..dst + 4].copy_from_slice(pixel);
            continue;
        }
        let alpha = u32::from(alpha);
        let inv = 255u32.saturating_sub(alpha);
        for channel in 0..3 {
            row[dst + channel] = ((u32::from(pixel[channel]) * alpha
                + u32::from(row[dst + channel]) * inv)
                / 255) as u8;
        }
        row[dst + 3] = 255;
    }
    Ok(())
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
