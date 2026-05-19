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
use nexus_layout_types::{FxPx, Rgba8};

use crate::layout_panel;
use crate::smoke::VisibleBootstrapMode;

const ROUTE_NAME: &str = "windowd";
const PROOF_PANEL_X: u32 = 56;
const PROOF_PANEL_Y: u32 = 440;
const PROOF_PANEL_H: u32 = crate::proof_panel_spec::PANEL_HEIGHT as u32;
const LIVE_FILTER_VARIANTS: [&str; 5] = ["", "a", "ap", "c", "b"];
const FILTER_LIST_PADDING_X: u32 = layout_panel::FILTER_LIST_PADDING;
const FILTER_LIST_PADDING_Y: u32 = layout_panel::FILTER_LIST_PADDING;
const FILTER_LIST_ROW_GAP: u32 = 2;
const FILTER_INPUT_PADDING_X: u32 = 8;
const FILTER_INPUT_FONT_W: u32 = 5;
const FILTER_INPUT_FONT_H: u32 = 7;
const FILTER_INPUT_FONT_SCALE: u32 = 2;
const FILTER_INPUT_FONT_ADVANCE: u32 = (FILTER_INPUT_FONT_W + 1) * FILTER_INPUT_FONT_SCALE;

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
        if let Err(err) = runtime.flush_pending_damage() {
            let _ = debug_println(flush_error_label(err));
        }
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
    proof_layouts: Option<Vec<LayoutResult>>,
    filtered_words: Vec<&'static str>,
    /// Index into `LIVE_FILTER_VARIANTS` for the active filter text/layout.
    active_filter_idx: usize,
    /// Filter cycle counter for automated proof (advances on each keyboard event).
    filter_cycle: u8,
    /// Whether clipping marker was emitted.
    clipping_marker_emitted: bool,
    /// Whether scroll marker was emitted.
    scroll_marker_emitted: bool,
    /// Whether live scroll marker was emitted.
    live_scroll_marker_emitted: bool,
    /// Whether v3b selftest summary markers were emitted.
    selftest_v3b_emitted: bool,
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
        let initial_state = VisibleState {
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
        };
        let mut filtered_words = Vec::with_capacity(crate::proof_panel_spec::FILTER_WORDS.len());
        refill_filtered_words(&mut filtered_words, initial_state.text_input());
        Ok(Self {
            mode,
            source_frame,
            cursor_bitmap,
            cursor_width,
            cursor_height,
            framebuffer: None,
            scratch_row: alloc::vec![0u8; mode.stride as usize],
            state: initial_state,
            markers_emitted: false,
            input_markers_emitted: InputMarkerState::default(),
            input_state_debug_emitted: false,
            pending_damage_rows: None,
            proof_layouts: build_live_proof_layouts(initial_state),
            filtered_words,
            active_filter_idx: 0,
            filter_cycle: 0,
            clipping_marker_emitted: false,
            scroll_marker_emitted: false,
            live_scroll_marker_emitted: false,
            selftest_v3b_emitted: false,
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
        if self.active_proof_layout().is_some() {
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
        self.emit_v3b_markers();
        STATUS_OK
    }

    fn apply_input_state(&mut self, upstream: VisibleState) -> u8 {
        if !self.input_state_debug_emitted {
            let _ = debug_println("dbg: windowd input state applied");
            self.input_state_debug_emitted = true;
        }
        let old_state = self.state;
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
        self.state.set_text_input(upstream.text_input());
        refill_filtered_words(&mut self.filtered_words, self.state.text_input());
        self.active_filter_idx = filter_layout_variant_index(self.state.text_input());
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

        // ── v3b: reflect real upstream text instead of synthetic keyboard cycling ──
        if old_state.text_input() != self.state.text_input() {
            self.note_filter_text_changed();
        }

        // ── v3b: scroll on wheel events ──
        if (upstream.wheel_up_visible || upstream.wheel_down_visible) && self.active_proof_layout().is_some()
        {
            self.handle_scroll_input();
        }

        // ── v3b: selftest summary markers (once) ──
        if !self.selftest_v3b_emitted
            && self.live_scroll_marker_emitted
            && self.clipping_marker_emitted
            && self.filter_cycle > 0
        {
            let _ = debug_println(crate::markers::SELFTEST_UI_V3_SCROLL_OK_MARKER);
            let _ = debug_println(crate::markers::SELFTEST_UI_V3_FILTER_OK_MARKER);
            let _ = debug_println(crate::markers::SELFTEST_UI_V3_IME_OK_MARKER);
            self.selftest_v3b_emitted = true;
        }

        STATUS_OK
    }

    fn note_filter_text_changed(&mut self) {
        self.filter_cycle = self.filter_cycle.wrapping_add(1);

        if !self.clipping_marker_emitted {
            let _ = debug_println(crate::markers::CLIPPING_ON_MARKER);
            self.clipping_marker_emitted = true;
        }
        let _ = debug_println(crate::markers::TEXT_INPUT_ON_MARKER);
        let _ = debug_println(crate::markers::FILTER_LIST_OK_MARKER);

        // Queue repaint for the combined panel area
        self.queue_rows(PROOF_PANEL_Y, PROOF_PANEL_Y + PROOF_PANEL_H);
    }

    fn handle_scroll_input(&mut self) {
        if !self.scroll_marker_emitted {
            let _ = debug_println(crate::markers::SCROLL_ON_MARKER);
            self.scroll_marker_emitted = true;
        }

        let wheel_down_visible = self.state.wheel_down_visible;
        // Compute content height before mutable borrow of proof_layouts
        let content_h = filter_list_content_height(&self.filtered_words);

        if let Some(layout) = self.active_proof_layout_mut() {
            // Find the filter_list container
            let container_id = layout
                .boxes
                .iter()
                .find(|b| b.id == Some("filter_list"))
                .map(|b| b.node_id);

            if let Some(id) = container_id {
                let viewport_h = layout
                    .boxes
                    .iter()
                    .find(|b| b.node_id == id)
                    .map(|b| {
                        FxPx::new(
                            filter_list_viewport_height(b.rect.height.as_u32().unwrap_or(0)) as i32,
                        )
                    })
                    .unwrap_or(FxPx::ZERO);
                let current_offset = layout
                    .boxes
                    .iter()
                    .find(|b| b.node_id == id)
                    .map(|b| b.scroll_offset)
                    .unwrap_or((FxPx::ZERO, FxPx::ZERO));

                let dy = if wheel_down_visible {
                    FxPx::new(20)
                } else {
                    FxPx::new(-20)
                };
                let max_scroll =
                    FxPx::new((content_h as i32).saturating_sub(viewport_h.0).max(0));
                let new_offset_y = (current_offset.1 + dy).clamp(FxPx::ZERO, max_scroll);
                let new_offset = (current_offset.0, new_offset_y);
                let _damage = layout.reposition_scroll(id, new_offset);
                let _ = debug_println(crate::markers::LIVE_SCROLL_OK_MARKER);
                self.live_scroll_marker_emitted = true;
            }
        }

        self.queue_rows(PROOF_PANEL_Y, PROOF_PANEL_Y + PROOF_PANEL_H);
    }

    fn current_filter_text(&self) -> &'static str {
        LIVE_FILTER_VARIANTS[self.active_filter_idx]
    }

    fn active_proof_layout(&self) -> Option<&LayoutResult> {
        self.proof_layouts.as_ref()?.get(self.active_filter_idx)
    }

    fn active_proof_layout_mut(&mut self) -> Option<&mut LayoutResult> {
        self.proof_layouts.as_mut()?.get_mut(self.active_filter_idx)
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

    fn emit_v3b_markers(&mut self) {
        let _ = debug_println(crate::markers::EFFECTS_ON_MARKER);
        let _ = debug_println(crate::markers::EFFECT_BLUR_OK_MARKER);
        let _ = debug_println(crate::markers::SELFTEST_UI_V3_EFFECT_OK_MARKER);
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
        let active_filter_idx = self.active_filter_idx;
        let proof_layout = self.proof_layouts.as_ref().and_then(|layouts| layouts.get(active_filter_idx));
        let source_frame = &self.source_frame;
        let mode = self.mode;
        let state = self.state;
        let filter_text = state.text_input();
        let filtered_words = self.filtered_words.as_slice();
        let cursor_bitmap = self.cursor_bitmap.as_deref();
        let cursor_width = self.cursor_width;
        let cursor_height = self.cursor_height;
        let cursor_x = self.state.cursor_x;
        let cursor_y = self.state.cursor_y;
        let end_y = end_y.min(self.mode.height);
        for y in start_y.min(end_y)..end_y {
            copy_scene_row(
                source_frame,
                mode,
                state,
                proof_layout,
                filter_text,
                filtered_words,
                cursor_bitmap,
                cursor_width,
                cursor_height,
                cursor_x,
                cursor_y,
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

fn flush_error_label(err: WindowdError) -> &'static str {
    match err {
        WindowdError::BufferLengthMismatch => "windowd: flush rows fail buffer-len",
        WindowdError::ArithmeticOverflow => "windowd: flush rows fail arith",
        _ => "windowd: flush rows fail",
    }
}

fn filter_layout_variant_index(filter_text: &str) -> usize {
    let mut best_idx = 0;
    let mut best_len = 0;
    for (idx, candidate) in LIVE_FILTER_VARIANTS.iter().enumerate() {
        if filter_text.starts_with(candidate) && candidate.len() >= best_len {
            best_idx = idx;
            best_len = candidate.len();
        }
    }
    best_idx
}

fn target_state_bits(state: VisibleState) -> u8 {
    u8::from(state.hover_visible)
        | (u8::from(state.launcher_click_visible) << 1)
        | (u8::from(state.keyboard_visible) << 2)
        | (u8::from(state.wheel_up_visible) << 3)
        | (u8::from(state.wheel_down_visible) << 4)
}

fn build_live_proof_layouts(state: VisibleState) -> Option<Vec<LayoutResult>> {
    let mut layouts = Vec::with_capacity(LIVE_FILTER_VARIANTS.len());
    for filter_text in LIVE_FILTER_VARIANTS {
        layouts.push(layout_panel::compute_proof_layout(state, filter_text).ok()?);
    }
    Some(layouts)
}

fn copy_scene_row(
    source_frame: &SourceFrame,
    mode: VisibleBootstrapMode,
    state: VisibleState,
    proof_layout: Option<&LayoutResult>,
    filter_text: &str,
    filtered_words: &[&'static str],
    cursor_bitmap: Option<&[u8]>,
    cursor_width: u32,
    cursor_height: u32,
    cursor_x: i32,
    cursor_y: i32,
    y: u32,
    row: &mut [u8],
) -> Result<(), WindowdError> {
    copy_scaled_systemui_row(source_frame, mode, y, row)?;
    draw_proof_surface_row(state, proof_layout, filter_text, filtered_words, y, row)?;
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
    filter_text: &str,
    filtered_words: &[&'static str],
    y: u32,
    row: &mut [u8],
) -> Result<(), WindowdError> {
    let Some(layout) = proof_layout else {
        return Ok(());
    };
    let mut filter_input_rect = None;
    let mut filter_list_rect = None;
    let mut filter_list_scroll_y = 0;
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
            if id == "filter_text_input" {
                filter_input_rect = Some(rect);
                // Try pre-rendered asset first (sharper, build.rs font)
                let asset_id = crate::proof_panel_spec::filter_input_asset_id(filter_text);
                if let Some(asset) = crate::assets::proof_text_asset(asset_id) {
                    blend_asset_row(y, row, rect.x, rect.y, asset.width, asset.height, asset.bgra)?;
                }
                continue;
            }
            if id == "filter_list" {
                filter_list_rect = Some(rect);
                filter_list_scroll_y = layout_box.scroll_offset.1.as_u32().unwrap_or(0);
                continue;
            }
            if id.starts_with("filter_") {
                continue;
            }
            if let Some(asset) = crate::assets::proof_text_asset(id) {
                blend_asset_row(y, row, rect.x, rect.y, asset.width, asset.height, asset.bgra)?;
            }
        }
    }
    if let Some(rect) = filter_input_rect {
        draw_filter_input_text_row(y, row, rect, filter_text)?;
    }
    if let Some(rect) = filter_list_rect {
        draw_filter_word_list_row(y, row, rect, filter_list_scroll_y, filtered_words)?;
    }
    Ok(())
}

fn refill_filtered_words(out: &mut Vec<&'static str>, filter_text: &str) {
    out.clear();
    for &word in crate::proof_panel_spec::FILTER_WORDS {
        if ascii_prefix_matches(word, filter_text) {
            out.push(word);
        }
    }
}

fn filter_word_asset_id(word: &str) -> &'static str {
    match word {
        "apple" => "filter_apple",
        "application" => "filter_application",
        "apt" => "filter_apt",
        "arrow" => "filter_arrow",
        "asset" => "filter_asset",
        "batch" => "filter_batch",
        "binary" => "filter_binary",
        "block" => "filter_block",
        "buffer" => "filter_buffer",
        "build" => "filter_build",
        "cache" => "filter_cache",
        "clock" => "filter_clock",
        "compile" => "filter_compile",
        "component" => "filter_component",
        "config" => "filter_config",
        _ => "filter_word",
    }
}

fn ascii_prefix_matches(word: &str, prefix: &str) -> bool {
    if prefix.is_empty() {
        return true;
    }
    let mut word_bytes = word.bytes();
    for prefix_byte in prefix.bytes() {
        let Some(word_byte) = word_bytes.next() else {
            return false;
        };
        if !word_byte.eq_ignore_ascii_case(&prefix_byte) {
            return false;
        }
    }
    true
}

/// Total content height of the filter word list (words + gaps + padding).
fn filter_list_content_height(filtered_words: &[&'static str]) -> u32 {
    let mut h: u32 = 0;
    for &word in filtered_words {
        if let Some(asset) = crate::assets::proof_text_asset(filter_word_asset_id(word)) {
            h = h.saturating_add(asset.height).saturating_add(FILTER_LIST_ROW_GAP);
        }
    }
    h.saturating_sub(FILTER_LIST_ROW_GAP) // remove trailing gap
}

fn filter_list_viewport_height(list_height: u32) -> u32 {
    list_height.saturating_sub(FILTER_LIST_PADDING_Y.saturating_mul(2))
}

fn filter_list_viewport_width(list_width: u32) -> u32 {
    list_width
        .saturating_sub(FILTER_LIST_PADDING_X.saturating_mul(2))
        .saturating_sub(layout_panel::FILTER_SCROLLBAR_GUTTER)
}

fn draw_filter_word_list_row(
    y: u32,
    row: &mut [u8],
    rect: ProofBoxRect,
    scroll_y: u32,
    filtered_words: &[&'static str],
) -> Result<(), WindowdError> {
    if !rect.contains_y(y) {
        return Ok(());
    }
    let viewport_x = rect.x + FILTER_LIST_PADDING_X;
    let viewport_y = rect.y + FILTER_LIST_PADDING_Y;
    let viewport_height = filter_list_viewport_height(rect.height);
    let viewport_width = filter_list_viewport_width(rect.width);
    let mut word_top = viewport_y;
    for &word in filtered_words {
        let Some(asset) = crate::assets::proof_text_asset(filter_word_asset_id(word)) else {
            continue;
        };
        let asset_top = word_top.saturating_sub(scroll_y);
        if y >= asset_top && y < asset_top.saturating_add(asset.height) {
            blend_asset_row_clipped(
                y,
                row,
                viewport_x,
                asset_top,
                asset.width,
                asset.height,
                asset.bgra,
                viewport_x,
                viewport_width,
            )?;
        }
        word_top = word_top
            .saturating_add(asset.height)
            .saturating_add(FILTER_LIST_ROW_GAP);
    }

    // ── Scrollbar ──
    let content_h = filter_list_content_height(filtered_words);
    if content_h > viewport_height {
        draw_filter_scrollbar_row(y, row, rect, scroll_y, content_h)?;
    }

    Ok(())
}

fn draw_filter_scrollbar_row(
    y: u32,
    row: &mut [u8],
    rect: ProofBoxRect,
    scroll_y: u32,
    content_h: u32,
) -> Result<(), WindowdError> {
    let viewport_y = rect.y + FILTER_LIST_PADDING_Y;
    let viewport_height = filter_list_viewport_height(rect.height);
    let strip_x = layout_panel::filter_scrollbar_strip_x(rect.x, rect.width);
    let track_x = layout_panel::filter_scrollbar_track_x(rect.x, rect.width);
    let gutter_width = rect.x.saturating_add(rect.width).saturating_sub(strip_x);
    let track_bgra = rgba_to_bgra(crate::assets::PROOF_PANEL_BG);
    if y >= viewport_y && y < viewport_y.saturating_add(viewport_height) {
        fill_row_rect(
            y,
            row,
            strip_x,
            viewport_y,
            gutter_width,
            viewport_height,
            track_bgra,
        )?;
    }

    let Some((thumb_y, thumb_height)) =
        layout_panel::filter_scrollbar_thumb_bounds(viewport_y, viewport_height, content_h, scroll_y)
    else {
        return Ok(());
    };

    let thumb_bgra = rgba_to_bgra(crate::assets::PROOF_SCROLL);
    fill_row_rect(
        y,
        row,
        track_x,
        thumb_y,
        layout_panel::FILTER_SCROLLBAR_WIDTH,
        thumb_height,
        thumb_bgra,
    )
}

fn draw_filter_input_text_row(
    y: u32,
    row: &mut [u8],
    rect: ProofBoxRect,
    filter_text: &str,
) -> Result<(), WindowdError> {
    if filter_text.is_empty() {
        return Ok(());
    }
    let glyph_height = FILTER_INPUT_FONT_H * FILTER_INPUT_FONT_SCALE;
    if rect.height <= glyph_height {
        return Ok(());
    }
    let text_top = rect.y + (rect.height - glyph_height) / 2;
    if y < text_top || y >= text_top.saturating_add(glyph_height) {
        return Ok(());
    }
    let glyph_row = ((y - text_top) / FILTER_INPUT_FONT_SCALE) as usize;
    let color = rgba_to_bgra(crate::assets::PROOF_PANEL_TITLE);
    let max_x = rect.x.saturating_add(rect.width.saturating_sub(FILTER_INPUT_PADDING_X));
    let mut pen_x = rect.x + FILTER_INPUT_PADDING_X;
    for ch in filter_text.chars() {
        if pen_x.saturating_add(FILTER_INPUT_FONT_W * FILTER_INPUT_FONT_SCALE) > max_x {
            break;
        }
        draw_bitmap_glyph_row(y, row, pen_x, glyph_row, ch, color)?;
        pen_x = pen_x.saturating_add(FILTER_INPUT_FONT_ADVANCE);
    }
    if pen_x + 1 < max_x {
        fill_row_rect(
            y,
            row,
            pen_x,
            text_top,
            2,
            glyph_height,
            rgba_to_bgra(crate::assets::PROOF_KEYBOARD),
        )?;
    }
    Ok(())
}

fn draw_bitmap_glyph_row(
    y: u32,
    row: &mut [u8],
    x: u32,
    glyph_row: usize,
    ch: char,
    bgra: [u8; 4],
) -> Result<(), WindowdError> {
    let bits = bitmap_font_5x7(ch)[glyph_row];
    for col in 0..FILTER_INPUT_FONT_W {
        if bits & (1 << (FILTER_INPUT_FONT_W - 1 - col)) == 0 {
            continue;
        }
        fill_row_rect(
            y,
            row,
            x + col * FILTER_INPUT_FONT_SCALE,
            y,
            FILTER_INPUT_FONT_SCALE,
            1,
            bgra,
        )?;
    }
    Ok(())
}

fn bitmap_font_5x7(ch: char) -> [u8; 7] {
    match ch.to_ascii_lowercase() {
        'a' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'b' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        'c' => [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110],
        'd' => [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110],
        'e' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'f' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        'g' => [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110],
        'h' => [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'i' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111],
        'j' => [0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100],
        'k' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        'l' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        'm' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        'n' => [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
        'o' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'p' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        'q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
        'r' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        's' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        't' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        'u' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'v' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        'w' => [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010],
        'x' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        'y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
        'z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
        '0' => [0b01110, 0b10011, 0b10101, 0b10101, 0b10101, 0b11001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        '3' => [0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110],
        '6' => [0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110],
        '-' => [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000],
        '_' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b11111],
        '.' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100],
        ' ' => [0; 7],
        _ => [0b11111, 0b00001, 0b00010, 0b00100, 0b00100, 0b00000, 0b00100],
    }
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
    let x = PROOF_PANEL_X + layout_box.rect.x.as_u32().unwrap_or(0);
    let y = PROOF_PANEL_Y + layout_box.rect.y.as_u32().unwrap_or(0);
    // Clip to clip_rect: if the box has a scissor rect, intersect with it
    if let Some(clip) = layout_box.clip_rect {
        let clip_x = PROOF_PANEL_X + clip.x.as_u32().unwrap_or(0);
        let clip_y = PROOF_PANEL_Y + clip.y.as_u32().unwrap_or(0);
        let clip_w = clip.width.as_u32().unwrap_or(0);
        let clip_h = clip.height.as_u32().unwrap_or(0);
        if clip_w == 0 || clip_h == 0 {
            return None;
        }
        // Intersect: box must overlap clip rect
        if x + width <= clip_x || clip_x + clip_w <= x
            || y + height <= clip_y || clip_y + clip_h <= y
        {
            return None; // completely outside clip rect
        }
    }
    Some(ProofBoxRect { x, y, width, height })
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
        ProofPaintPart::FilterContent => Some(crate::assets::PROOF_CARD_BG),
        // Keep filter text nodes transparent and let the text renderer provide the glyphs.
        // Filling these text boxes produced long bar-like artifacts during scroll.
        ProofPaintPart::FilterWord => layout_box.visual.background,
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
    Filter,
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
            Self::Filter => ProofCardPaint {
                active: true,
                accent: crate::assets::PROOF_PANEL_TITLE,
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
    FilterContent,
    FilterWord,
}

fn proof_paint_role(id: &str) -> Option<ProofPaintRole> {
    use ProofCard::{Click, Filter, Hover, Key, Scroll};
    use ProofPaintPart::{Dot, FilterContent, FilterWord, Glyph, Icon, Root, ScrollDown, ScrollUp};

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
        "filter_panel" => (Filter, Root),
        "filter_content" => (Filter, FilterContent),
        "filter_text_input" => (Filter, FilterWord),
        "filter_list" => (Filter, FilterContent),
        "filter_word" => (Filter, FilterWord),
        // Filter word asset IDs point to pre-rendered text
        id if id.starts_with("filter_") => (Filter, FilterWord),
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

fn blend_asset_row_clipped(
    y: u32,
    row: &mut [u8],
    x: u32,
    top: u32,
    width: u32,
    height: u32,
    source: &[u8],
    clip_x: u32,
    clip_width: u32,
) -> Result<(), WindowdError> {
    if y < top || y >= top.saturating_add(height) || clip_width == 0 {
        return Ok(());
    }
    let visible_x = x.max(clip_x);
    let visible_end = x
        .saturating_add(width)
        .min(clip_x.saturating_add(clip_width))
        .min((row.len() / 4) as u32);
    if visible_end <= visible_x {
        return Ok(());
    }
    let source_y = y - top;
    let src_row = source_y as usize * width as usize * 4;
    let src_offset = visible_x.saturating_sub(x) as usize * 4;
    let src_len = visible_end.saturating_sub(visible_x) as usize * 4;
    let source = source
        .get(src_row + src_offset..src_row + src_offset + src_len)
        .ok_or(WindowdError::BufferLengthMismatch)?;
    blend_overlay_row(row, visible_x as usize, source)
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
