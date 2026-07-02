// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — the login greeter (TASK-0065B):
//! full-screen blurred wallpaper + a centered round user avatar with the
//! user's name, hover feedback, click → sessiond OP_LOGIN → the resolved
//! session shell. Appearance comes from SystemUI's greeter manifest
//! (`systemui::greeter_config()`); WHO can log in comes from sessiond.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Hit-testing/gating host-tested in `crate::interaction`;
//! manifest bounds in `systemui::greeter`; the bake is boot-verified.
//!
//! Rendering model: the greeter is BAKED into Plane 1 (the retained base) —
//! no atlas surfaces, no per-frame effects. The bake is a one-time separable
//! box blur of the wallpaper source (horizontal per row via the shared
//! `blur_row_horizontal`, vertical via a rolling window over those rows) plus
//! a dark dim, then the avatar card (SDF circle + ring + Lucide glyph +
//! bitmap-font name). Hover redraws only the card from a saved backdrop copy.
//! Login exit rewrites Plane 1 from the pristine wallpaper source
//! (`write_current_frame`) and applies the session shell.

use super::*;
use crate::interaction::HitRect;

/// Greeter runtime state: config, the (single) user shown, hover, and the
/// saved blurred backdrop under the avatar card for cheap hover redraws.
pub(super) struct GreeterState {
    /// SystemUI greeter appearance.
    pub cfg: systemui::GreeterConfig,
    /// Login id sent to sessiond on click.
    pub user_id: alloc::string::String,
    /// Name under the avatar.
    pub display_name: alloc::string::String,
    /// Cursor is over the avatar card.
    pub hover: bool,
    /// The avatar card rect (geometry SSOT: `interaction::greeter_avatar_rect`).
    pub card: HitRect,
    /// Blurred backdrop pixels under the card (card.width×card.height×4).
    backdrop: alloc::vec::Vec<u8>,
    /// Reusable card compose buffer (same size as `backdrop`) — allocated
    /// ONCE: the service bump allocator never frees, so a per-hover-redraw
    /// temporary would leak its size on every hover change.
    card_rows: alloc::vec::Vec<u8>,
}

/// Label font metrics (5×7 bitmap font at 2× — the shell-window label style).
const FONT_W: u32 = 5;
const FONT_H: u32 = 7;
const FONT_SCALE: u32 = 2;
const GLYPH_ADVANCE: u32 = FONT_W * FONT_SCALE + 2;

impl DisplayServerRuntime {
    /// True while the login greeter owns the display (shell chrome + all its
    /// affordances are suppressed — the session gate).
    pub(super) fn greeter_active(&self) -> bool {
        self.greeter.is_some()
    }

    /// Shell chrome composites only when the config enables it AND no greeter
    /// owns the display (TASK-0065B session gate).
    pub(super) fn chrome_composited(&self) -> bool {
        self.shell_config.desktop_chrome && self.greeter.is_none()
    }

    /// The greeter's clickable card, when active (input hit-testing).
    pub(crate) fn greeter_hit_rect(&self) -> Option<HitRect> {
        self.greeter.as_ref().map(|g| g.card)
    }

    /// Build + present the login greeter for the first registered user.
    /// Called by the session probe when sessiond reports the greeter state.
    pub(super) fn start_greeter(&mut self, users: &[crate::session_client::SessionUser]) {
        let Some(user) = users.first() else {
            let _ = debug_println("windowd: greeter no users (auto shell)");
            return;
        };
        let cfg = systemui::greeter_config();
        let card = crate::interaction::greeter_avatar_rect(self.mode, cfg.avatar_diameter);
        let mut state = GreeterState {
            cfg,
            user_id: user.id.clone(),
            display_name: user.display_name.clone(),
            hover: false,
            card,
            backdrop: alloc::vec::Vec::new(),
            card_rows: alloc::vec![
                0u8;
                card.width as usize * card.height as usize * 4
            ],
        };
        if let Err(err) = self.bake_greeter_backdrop(&mut state) {
            let _ = debug_println(&alloc::format!(
                "windowd: greeter bake failed err={err:?} (auto shell)"
            ));
            return;
        }
        self.greeter = Some(state);
        if let Err(err) = self.redraw_greeter_card() {
            let _ = debug_println(&alloc::format!("windowd: greeter card failed err={err:?}"));
        }
        // One full-frame damage: the blurred base + card reach the display
        // plane through the normal present path.
        self.queue_gpu_blit_rect(DamageRect {
            x: 0,
            y: 0,
            width: self.mode.width,
            height: self.mode.height,
        });
        let _ = debug_println("windowd: greeter visible");
    }

    /// Pointer-move hover update: redraw only the card on a state change.
    pub(super) fn update_greeter_hover(&mut self, cursor_x: i32, cursor_y: i32) {
        let Some((card, old_hover)) = self.greeter.as_ref().map(|g| (g.card, g.hover)) else {
            return;
        };
        let hover = crate::interaction::hover_over_greeter(card, cursor_x, cursor_y);
        if hover == old_hover {
            return;
        }
        if let Some(greeter) = self.greeter.as_mut() {
            greeter.hover = hover;
        }
        if let Err(err) = self.redraw_greeter_card() {
            let _ = debug_println(&alloc::format!("windowd: greeter hover failed err={err:?}"));
        }
        self.queue_gpu_blit_rect(DamageRect {
            x: card.x,
            y: card.y,
            width: card.width,
            height: card.height,
        });
    }

    /// Avatar click: relay the login to sessiond; on success tear the greeter
    /// down and bring up the resolved session shell.
    pub(super) fn greeter_login_click(&mut self) {
        let Some(user_id) = self.greeter.as_ref().map(|g| g.user_id.clone()) else {
            return;
        };
        match crate::session_client::login(&user_id) {
            Some(product) => self.finish_greeter_login(&product),
            None => {
                let _ = debug_println("windowd: login failed");
            }
        }
    }

    /// Login accepted: restore the pristine base scene into Plane 1, drop the
    /// greeter (chrome affordances come back), apply the session shell.
    fn finish_greeter_login(&mut self, product: &str) {
        self.greeter = None;
        if let Err(err) = self.write_current_frame() {
            let _ = debug_println(&alloc::format!(
                "windowd: greeter exit rewrite failed err={err:?}"
            ));
        }
        self.apply_session_shell(product);
        self.queue_gpu_blit_rect(DamageRect {
            x: 0,
            y: 0,
            width: self.mode.width,
            height: self.mode.height,
        });
    }

    /// One-time bake: separable box blur of the wallpaper source into Plane 1
    /// (+ dark dim), saving the card-region backdrop for hover redraws.
    /// Horizontal pass = the shared `blur_row_horizontal`; vertical pass = a
    /// rolling sum over a ring of those rows (edge-clamped) — O(w·h), no
    /// full-frame temporary.
    fn bake_greeter_backdrop(&mut self, state: &mut GreeterState) -> Result<(), WindowdError> {
        let Some(handle) = self.framebuffer else {
            return Err(WindowdError::BufferLengthMismatch);
        };
        let stride = self.mode.stride as usize;
        let width = self.mode.width as usize;
        let height = self.mode.height;
        if self.band_scratch.len() < stride * ROW_WRITE_CHUNK {
            return Err(WindowdError::BufferLengthMismatch);
        }
        let h_radius = state.cfg.blur_radius;
        let radius = h_radius.max(1) as usize;
        let window = 2 * radius + 1;
        let dim = state.cfg.dim as u32;
        let card = state.card;
        state.backdrop.clear();
        state
            .backdrop
            .resize(card.width as usize * card.height as usize * 4, 0);

        // Ring of horizontally blurred source rows (row index → slot idx%window)
        // + channel sums for the vertical rolling window.
        let mut ring = alloc::vec![0u8; window * stride];
        let mut row_buf = alloc::vec![0u8; stride];
        let mut sums = alloc::vec![0u32; width * 4];
        let source_frame = &self.source_frame;
        let source_x_lut = self.source_x_lut.as_slice();
        let source_y_lut = self.source_y_lut.as_slice();
        let mode = self.mode;
        let render_clip = RenderClip::full(mode.width);
        let max_row = height.saturating_sub(1) as usize;

        // Produce the horizontally blurred scene row `idx` into its ring slot.
        let mut fill_ring_row = |ring: &mut [u8], row_buf: &mut [u8], idx: usize| -> Result<(), WindowdError> {
            let slot = (idx % window) * stride;
            let row = &mut ring[slot..slot + stride];
            copy_scene_row(
                source_frame,
                source_x_lut,
                source_y_lut,
                mode,
                idx as u32,
                render_clip,
                row,
            )?;
            crate::compositor::blur::blur_row_horizontal(row, width * 4, h_radius, row_buf);
            Ok(())
        };
        let add_row = |ring: &[u8], sums: &mut [u32], idx: usize, sign_add: bool| {
            let slot = (idx % window) * stride;
            let row = &ring[slot..slot + width * 4];
            if sign_add {
                for (i, px) in row.iter().enumerate() {
                    sums[i] += *px as u32;
                }
            } else {
                for (i, px) in row.iter().enumerate() {
                    sums[i] = sums[i].saturating_sub(*px as u32);
                }
            }
        };

        // Prefill the window for output row 0: rows -r..=r edge-clamped.
        for idx in 0..=radius.min(max_row) {
            fill_ring_row(&mut ring, &mut row_buf, idx)?;
        }
        add_row(&ring, &mut sums, 0, true); // clamp(-r..0) — row 0 …
        for _ in 0..radius {
            add_row(&ring, &mut sums, 0, true); // … replicated r more times
        }
        for idx in 1..=radius {
            add_row(&ring, &mut sums, idx.min(max_row), true);
        }

        // Stream output rows in bands into Plane 1.
        let band_scratch = &mut self.band_scratch;
        let mut band_start = 0usize;
        while band_start < height as usize {
            let band_end = (band_start + ROW_WRITE_CHUNK).min(height as usize);
            for (row_idx, y) in (band_start..band_end).enumerate() {
                let out = &mut band_scratch[row_idx * stride..row_idx * stride + stride];
                for i in 0..width * 4 {
                    let mut v = sums[i] / window as u32;
                    if i % 4 != 3 {
                        v = v * dim / 255;
                    }
                    out[i] = v.min(255) as u8;
                }
                for px in out[..width * 4].chunks_exact_mut(4) {
                    px[3] = 255;
                }
                // Save the card backdrop while it streams past.
                if (y as u32) >= card.y && (y as u32) < card.y + card.height {
                    let dst_row = (y as u32 - card.y) as usize * card.width as usize * 4;
                    let src = card.x as usize * 4;
                    state.backdrop[dst_row..dst_row + card.width as usize * 4]
                        .copy_from_slice(&out[src..src + card.width as usize * 4]);
                }
                // Advance the rolling window to the next output row.
                if y + 1 < height as usize {
                    add_row(&ring, &mut sums, y.saturating_sub(radius), false);
                    let incoming = (y + 1 + radius).min(max_row);
                    if y + 1 + radius <= max_row {
                        fill_ring_row(&mut ring, &mut row_buf, incoming)?;
                    }
                    add_row(&ring, &mut sums, incoming, true);
                }
            }
            let band_bytes = (band_end - band_start) * stride;
            vmo_write(
                handle,
                RETAINED_OFFSET_BYTES + band_start * stride,
                &band_scratch[..band_bytes],
            )
            .map_err(|_| WindowdError::BufferLengthMismatch)?;
            band_start = band_end;
        }
        Ok(())
    }

    /// (Re)draw the avatar card into Plane 1 from the saved blurred backdrop:
    /// glass disc (hover brightens), ring stroke, `circle-user` glyph, name.
    /// Composes into the state's REUSABLE `card_rows` buffer — the bump heap
    /// never frees, so a per-call temporary would leak on every hover change.
    fn redraw_greeter_card(&mut self) -> Result<(), WindowdError> {
        let Some(handle) = self.framebuffer else {
            return Err(WindowdError::BufferLengthMismatch);
        };
        let stride = self.mode.stride as usize;
        let Some(greeter) = self.greeter.as_mut() else {
            return Ok(());
        };
        // Disjoint field borrows: backdrop/name read, card_rows written.
        let GreeterState { cfg, display_name, hover, card, backdrop, card_rows, .. } = greeter;
        let card = *card;
        let hover = *hover;
        let card_w = card.width;
        let card_h = card.height;
        let row_bytes = card_w as usize * 4;
        if backdrop.len() < card_h as usize * row_bytes
            || card_rows.len() < card_h as usize * row_bytes
        {
            return Err(WindowdError::BufferLengthMismatch);
        }
        // Card-local geometry: circle centered horizontally, label below.
        let d = cfg.avatar_diameter.min(card_w).min(card_h);
        let circle_x = (card_w - d) / 2;
        let circle_y = 0u32;
        let label_top = d + cfg.label_gap;
        let (disc, ring): ([u8; 4], [u8; 4]) = if hover {
            ([96, 96, 104, 235], [255, 255, 255, 235])
        } else {
            ([72, 72, 80, 215], [230, 230, 235, 170])
        };
        let icon = crate::assets::GREETER_AVATAR_ICON_BGRA;
        let icon_dim = crate::assets::GREETER_AVATAR_ICON_DIM;
        let icon_x = circle_x + (d.saturating_sub(icon_dim)) / 2;
        let icon_y = circle_y + (d.saturating_sub(icon_dim)) / 2;
        let name = display_name.as_str();
        let text_w = (name.chars().count() as u32 * GLYPH_ADVANCE).saturating_sub(2);
        let text_x = card_w.saturating_sub(text_w) / 2;
        let label_color = [235u8, 235, 240, 255];

        for ly in 0..card_h {
            let row =
                &mut card_rows[ly as usize * row_bytes..ly as usize * row_bytes + row_bytes];
            row.copy_from_slice(
                &backdrop[ly as usize * row_bytes..ly as usize * row_bytes + row_bytes],
            );
            if ly < d {
                crate::compositor::sdf::fill_sdf_circle_row(
                    ly, row, circle_x, circle_y, d, d, disc,
                )?;
                crate::compositor::sdf::stroke_sdf_circle_row(
                    ly,
                    row,
                    circle_x,
                    circle_y,
                    d,
                    d,
                    cfg.ring_stroke,
                    ring,
                )?;
                if ly >= icon_y && ly < icon_y + icon_dim {
                    crate::compositor::desktop_layer::blend_icon_row(
                        row,
                        icon_x,
                        icon,
                        icon_dim,
                        ly - icon_y,
                        if hover { 255 } else { 225 },
                    );
                }
            }
            draw_greeter_label(ly, row, name, text_x, label_top, label_color);
        }
        // Write the card rows into Plane 1 (row-sized writes bounded by card_h).
        for ly in 0..card_h as usize {
            let dst = RETAINED_OFFSET_BYTES
                + (card.y as usize + ly) * stride
                + card.x as usize * 4;
            vmo_write(handle, dst, &card_rows[ly * row_bytes..ly * row_bytes + row_bytes])
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
        }
        Ok(())
    }
}

/// Card-local name label: 5×7 bitmap font at 2×, same look as the window
/// title labels (`shell_window::draw_label` is private to that module).
fn draw_greeter_label(ly: u32, row: &mut [u8], text: &str, x0: u32, top: u32, color: [u8; 4]) {
    if ly < top || ly >= top + FONT_H * FONT_SCALE {
        return;
    }
    let glyph_row = ((ly - top) / FONT_SCALE).min(FONT_H - 1) as usize;
    let rp = (row.len() / 4) as u32;
    let mut pen_x = x0;
    for ch in text.chars() {
        let bits = crate::bitmap_font::bitmap_font_5x7(ch)[glyph_row];
        for col in 0..FONT_W {
            if bits & (1 << (FONT_W - 1 - col)) != 0 {
                for sx in 0..FONT_SCALE {
                    let px = pen_x + col * FONT_SCALE + sx;
                    if px < rp {
                        let idx = px as usize * 4;
                        row[idx..idx + 4].copy_from_slice(&color);
                    }
                }
            }
        }
        pen_x += GLYPH_ADVANCE;
    }
}
