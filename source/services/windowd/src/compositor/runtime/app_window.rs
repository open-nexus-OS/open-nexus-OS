// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! BOUNDARY: this file is the SERVICE side of the app surface — VMO/atlas
//! registration, present/ack flow control, the damage-blit, and the event
//! channel (theme/rect pushes). It MUST NOT grow window-chrome/sizing/resize/
//! decoration logic — that is the `ui/widgets/window` widget's job (the frame
//! it currently opens is the legacy `ShellWindow`, being retired, see
//! windows-as-widgets.md). Keep this to: move surface bytes, route input,
//! push geometry/theme. Chrome + layout live in the widget + scene graph.
//!
//! CONTEXT: windowd compositor runtime — the ADR-0042 cross-process app
//! window (TASK-0080D R1): `SURFACE_CREATE` registers the app's surface VMO
//! (capability moved with the message, gpud-attach pattern) and opens a
//! fifth `ShellWindow`; `SURFACE_PRESENT` marks the body dirty and acks the
//! seq; the render path blits the surface rows out of the app's VMO
//! (`vmo_read`, syscall 47) under windowd's own title bar. Apps get pixels
//! and events — never scene-graph or atlas access.
//! OWNERS: @ui @runtime
//! STATUS: Experimental (TASK-0080D R1)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: bookkeeping host-tested in `crate::client_surface`; the
//! blit is proven via QEMU markers (`WINDOWD: surface …`).
//! ADR: docs/adr/0042-cross-process-surface-transport.md

use super::*;
use nexus_display_proto::client_surface as wire;

/// Window bounds: the pool reserve + `ShellWindow` frame are sized for the
/// LARGEST allowed surface; smaller surfaces render into the top-left of the
/// body. (`crate::client_surface` enforces the surface-size bounds.)
pub(crate) const APP_WIN_MAX_W: u32 = crate::client_surface::MAX_SURFACE_W as u32;
pub(crate) const APP_WIN_MAX_H: u32 =
    crate::client_surface::MAX_SURFACE_H as u32 + APP_TITLE_H;
pub(crate) const APP_TITLE_H: u32 = 32;
pub(crate) const APP_CLOSE_W: u32 = 40;

impl DisplayServerRuntime {
    /// `SURFACE_CREATE`: validate + register the surface, retain the moved
    /// VMO capability, open the app window. Returns the ack frame.
    pub(crate) fn handle_surface_create(
        &mut self,
        frame: &[u8],
        vmo_slot: Option<u32>,
    ) -> [u8; wire::SURFACE_ACK_FRAME_LEN] {
        let Some((width, height, format)) = wire::decode_surface_create(frame) else {
            return wire::encode_surface_ack(
                wire::OP_SURFACE_CREATE,
                wire::SURFACE_STATUS_MALFORMED,
                0,
            );
        };
        let Some(vmo_slot) = vmo_slot else {
            // The VMO capability MUST ride with the create message.
            let _ = debug_println("WINDOWD: surface create FAIL (no vmo cap)");
            return wire::encode_surface_ack(
                wire::OP_SURFACE_CREATE,
                wire::SURFACE_STATUS_MALFORMED,
                0,
            );
        };
        match self.client_surfaces.create(width, height, format, vmo_slot) {
            Ok(id) => {
                // P3.1 (windows-as-widgets): size the window FRAME to the actual
                // surface content (+ the title bar), via `window::frame`, BEFORE
                // the atlas band is allocated. The frame/shadow track the
                // content instead of a fixed window max — no full-screen band
                // for a small window, no oversized shadow. Chrome per intent: a
                // `plain` surface drops the title bar (`chrome = intent ⟂ policy`).
                // Chrome is INTENT-driven (⟂ policy) — fullscreen changes the
                // FRAME, never the chrome. A titled app keeps its title bar
                // (min/max/close) both floating AND maximized (the □ = MAXIMIZE);
                // only an intent-chromeless surface (plain / desktop / fullscreen
                // intent — a shell or single-app-OS launcher) goes edge-to-edge.
                self.app_win.title_h = self.app_title_h();
                if self.windows.is_fullscreen(crate::window_scene::WindowId::AppClient) {
                    // Maximize / kiosk-fullscreen: cover the whole display. Titled
                    // apps keep the title bar at the top (content = display −
                    // title, drawn by `render_app_surface`); chromeless surfaces
                    // fill edge-to-edge. Content-sizing here would re-float it.
                    self.app_win.set_frame(0, 0, self.mode.width, self.mode.height);
                } else {
                    let content_h = u32::from(height).saturating_add(self.app_win.title_h);
                    self.app_win.set_frame(
                        self.app_win.x,
                        self.app_win.y,
                        u32::from(width),
                        content_h,
                    );
                }
                if !self.open_app_window() {
                    // Atlas exhausted: roll the registration back fail-closed.
                    let _ = self.client_surfaces.destroy(id);
                    let _ = nexus_abi_cap_close(vmo_slot);
                    return wire::encode_surface_ack(
                        wire::OP_SURFACE_CREATE,
                        wire::SURFACE_STATUS_QUOTA,
                        0,
                    );
                }
                let _ = debug_println(&alloc::format!(
                    "WINDOWD: surface created id={id} {width}x{height}"
                ));
                wire::encode_surface_ack(wire::OP_SURFACE_CREATE, wire::SURFACE_STATUS_OK, id)
            }
            Err(status) => {
                let _ = nexus_abi_cap_close(vmo_slot);
                let _ = debug_println(&alloc::format!(
                    "WINDOWD: surface create FAIL (status={status})"
                ));
                wire::encode_surface_ack(wire::OP_SURFACE_CREATE, status, 0)
            }
        }
    }

    /// `SURFACE_PRESENT`: validate seq + damage, mark the window body dirty
    /// (the render path blits from the VMO), queue the damage. Acks the seq.
    pub(crate) fn handle_surface_present(
        &mut self,
        frame: &[u8],
    ) -> [u8; wire::SURFACE_ACK_FRAME_LEN] {
        let Some((surface_id, seq, rects, count)) = wire::decode_surface_present(frame) else {
            return wire::encode_surface_ack(
                wire::OP_SURFACE_PRESENT,
                wire::SURFACE_STATUS_MALFORMED,
                0,
            );
        };
        match self.client_surfaces.present(surface_id, seq, &rects[..count]) {
            Ok((_, _, _)) => {
                // v1: bounded full-body blit on the next render; the damage
                // list bounds the QUEUED screen region (blit-by-rect is the
                // recorded optimization — ADR-0042).
                self.app_win.surface_dirty = true;
                let rect = self.app_window_rect();
                self.queue_dirty_rect(rect);
                let _ = debug_println(&alloc::format!(
                    "WINDOWD: surface presented id={surface_id} seq={seq}"
                ));
                wire::encode_surface_ack(wire::OP_SURFACE_PRESENT, wire::SURFACE_STATUS_OK, seq)
            }
            Err(status) => {
                // A rejected present is otherwise silent — name the status +
                // seq so a seq/surface mismatch is diagnosable (bounded).
                if self.app_present_reject_markers < 8 {
                    self.app_present_reject_markers += 1;
                    let _ = debug_println(&alloc::format!(
                        "WINDOWD: FAIL surface present rejected id={surface_id} seq={seq} status={status}"
                    ));
                }
                wire::encode_surface_ack(wire::OP_SURFACE_PRESENT, status, seq)
            }
        }
    }

    /// `SURFACE_DESTROY`: drop the registration, release the VMO capability,
    /// close the window (ADR-0037 residency: closed app holds no surface).
    pub(crate) fn handle_surface_destroy(
        &mut self,
        frame: &[u8],
    ) -> [u8; wire::SURFACE_ACK_FRAME_LEN] {
        let Some(surface_id) = wire::decode_surface_destroy(frame) else {
            return wire::encode_surface_ack(
                wire::OP_SURFACE_DESTROY,
                wire::SURFACE_STATUS_MALFORMED,
                0,
            );
        };
        match self.client_surfaces.destroy(surface_id) {
            Ok(vmo_slot) => {
                let _ = nexus_abi_cap_close(vmo_slot);
                // A protocol SURFACE_DESTROY is the app dropping its surface —
                // during a resize/fullscreen negotiation it re-creates one
                // immediately. Free the atlas band ONLY; do NOT hide the window,
                // which would clear its fullscreen flag (see `window_scene::hide`)
                // and re-add the title bar on the re-create (atlas over-alloc, the
                // "fullscreen makes everything vanish" bug). The user-close path
                // (× button) calls `close_app_window` directly, not this.
                self.release_app_surface_band();
                let _ = debug_println(&alloc::format!(
                    "WINDOWD: surface destroyed id={surface_id}"
                ));
                wire::encode_surface_ack(
                    wire::OP_SURFACE_DESTROY,
                    wire::SURFACE_STATUS_OK,
                    surface_id,
                )
            }
            Err(status) => wire::encode_surface_ack(wire::OP_SURFACE_DESTROY, status, surface_id),
        }
    }

    /// Acquire atlas surfaces + show the window (mirrors `open_dsl_demo`).
    fn open_app_window(&mut self) -> bool {
        if !self.app_win.is_mounted() {
            let w = self.app_win.w;
            let h = self.app_win.h;
            let Some(content) = self.atlas_alloc.alloc(w, h) else {
                let _ = debug_println(&alloc::format!(
                    "WINDOWD: surface open FAIL atlas (need={}x{} rows_remaining={})",
                    w,
                    h,
                    self.atlas_alloc.rows_remaining()
                ));
                return false;
            };
            // A desktop/full-screen surface uses the R1 layer path
            // (`BackdropCache::None`), so it needs NO per-window blur band — a
            // full-screen blur band would starve the atlas. Windowed apps keep
            // the cached-blur band.
            let blur = if self.app_is_desktop_surface() {
                None
            } else {
                self.atlas_alloc.alloc(w, h) // best-effort
            };
            self.app_win.mount(content, blur);
        }
        self.app_win.visible = true;
        self.show_window(crate::window_scene::WindowId::AppClient);
        self.app_win.surface_dirty = true;
        let rect = self.app_window_rect();
        self.queue_dirty_rect(rect);
        true
    }

    pub(super) fn close_app_window(&mut self) {
        self.app_win.visible = false;
        self.hide_window(crate::window_scene::WindowId::AppClient);
        self.app_win.end_drag();
        self.release_app_surface_band();
    }

    /// Free the app window's atlas band(s) WITHOUT touching window state
    /// (visibility, fullscreen, position). Used by the resize/fullscreen
    /// re-create (protocol SURFACE_DESTROY): the band must be reclaimed before
    /// the new surface allocates one, but the window keeps its geometry + mode so
    /// the re-created surface resumes in place. `close_app_window` hides first,
    /// then calls this.
    pub(super) fn release_app_surface_band(&mut self) {
        let rect = self.app_window_rect();
        if let Some((content, blur)) = self.app_win.unmount() {
            self.atlas_alloc.free(content);
            if let Some(blur) = blur {
                self.atlas_alloc.free(blur);
            }
        }
        self.queue_dirty_rect(rect);
    }

    /// Blits the app surface out of its VMO into the window's atlas band:
    /// title bar drawn by windowd (server-side decoration), body rows read
    /// via `vmo_read` — the ADR-0042 damage-blit. Bounded by the surface
    /// dims validated at create.
    pub(super) fn render_app_surface(&mut self) -> Result<(), WindowdError> {
        let Some(handle) = self.framebuffer else {
            return Ok(());
        };
        let Some(surface) = self.app_win.atlas else {
            return Ok(());
        };
        let Some(client) = self.client_surfaces.get().copied() else {
            return Ok(());
        };
        let stride = self.mode.stride as usize;
        if self.band_scratch.len() < stride {
            return Err(WindowdError::BufferLengthMismatch);
        }
        let abs_row = surface.abs_row;
        let col_off = surface.x as usize * 4;
        let win_w = self.app_win.w.min(surface.width);
        let win_h = self.app_win.h.min(surface.height);
        let body_w = (client.width as u32).min(win_w);
        let body_row_bytes = body_w as usize * 4;
        let src_stride = client.width as usize * 4;
        let title_hover = self.app_win.title_hover;
        let corner_radius =
            if self.windows.is_fullscreen(crate::window_scene::WindowId::AppClient) {
                0
            } else {
                dsl_mount::DSL_RADIUS
            };
        let tk = self.theme();
        // Chromeless when `title_h == 0` (a `plain`/desktop-style surface, e.g.
        // the shell): the title-bar block never runs and the body fills from
        // row 0. A normal window keeps `APP_TITLE_H` — the WM-drawn frame.
        let title_h = self.app_win.title_h;
        for ly in 0..win_h {
            let row_bytes = win_w as usize * 4;
            let row = &mut self.band_scratch[0..stride];
            row[..row_bytes].fill(0);
            if ly < title_h {
                // Chrome (bar + title text + real icon controls `[– □ ×]` +
                // hover) is RASTERIZED into the band here, only when the band is
                // dirty (create/resize/hover/theme) — NOT per frame. It then
                // composites as ONE cached surface (`composite_glass`). This is
                // the glyph/chrome-CACHE pattern: never emit per-glyph vector
                // tiles every present (that floods gpud's non-freeing heap →
                // `alloc-fail svc=gpud`, the resize crash). The scene graph
                // renders this as a Surface (blit), not vector text.
                crate::compositor::shell_window::draw_title_bar_row(
                    ly,
                    row,
                    win_w,
                    "App",
                    title_h,
                    APP_CLOSE_W,
                    title_hover,
                    corner_radius,
                    tk,
                )?;
            } else {
                let body_y = ly - title_h;
                if body_y < client.height as u32 {
                    // The damage-blit: one surface row out of the app's VMO.
                    #[cfg(nexus_env = "os")]
                    {
                        let src_off = body_y as usize * src_stride;
                        if nexus_abi::vmo_read(
                            client.vmo_slot,
                            src_off,
                            &mut row[..body_row_bytes],
                        )
                        .is_err()
                        {
                            return Err(WindowdError::BufferLengthMismatch);
                        }
                    }
                } else {
                    // Below the app surface (max-size frame): glass tint.
                    crate::compositor::desktop_layer::write_tint_span(
                        row,
                        0,
                        win_w,
                        crate::theme::with_alpha(
                            tk.glass_tint,
                            crate::compositor::desktop_layer::TINT[3],
                        ),
                    );
                }
            }
            let dst = (abs_row + ly) as usize * stride + col_off;
            vmo_write(handle, dst, &self.band_scratch[..win_w as usize * 4])
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
        }
        Ok(())
    }

    pub(super) fn app_window_rect(&self) -> DamageRect {
        self.app_win.damage_rect(self.mode.width, self.mode.height)
    }

    /// Window intent (`OP_SURFACE_INTENT`, sent before create): store the
    /// style/level/mode and answer the composed **content rect** the app sizes
    /// its surface VMO to (the WM owns geometry — no display-mode query). Under
    /// the v1 Desktop policy a `desktop`/`fullscreen` surface fills the display;
    /// otherwise it gets the default window body size. Reply rides the app event
    /// channel; if it is not attached yet the app's bounded wait falls back.
    pub(crate) fn handle_surface_intent(&mut self, frame: &[u8]) {
        let Some((style, level, mode, _resizable)) = wire::decode_surface_intent(frame) else {
            return;
        };
        self.app_intent_style = style;
        self.app_intent_level = level;
        self.app_intent_mode = mode;
        let (rw, rh) = if level == wire::WIN_LEVEL_DESKTOP || mode == wire::WIN_MODE_FULLSCREEN {
            (self.mode.width as u16, self.mode.height as u16)
        } else {
            (
                self.app_win.w as u16,
                self.app_win.h.saturating_sub(self.app_win.title_h) as u16,
            )
        };
        let rect = wire::encode_surface_rect(0, 0, rw, rh);
        let _ = self.send_app_frame(&rect);
        let _ = debug_println(&alloc::format!(
            "WINDOWD: surface intent style={style} level={level} mode={mode} -> {rw}x{rh}"
        ));
    }

    /// True when the app declared a desktop/full-screen surface (chromeless,
    /// screen-sized, no per-window blur band).
    fn app_is_desktop_surface(&self) -> bool {
        self.app_intent_level == wire::WIN_LEVEL_DESKTOP
            || self.app_intent_mode == wire::WIN_MODE_FULLSCREEN
            // A window the user toggled to fullscreen uses the R1 layer path too
            // (edge-to-edge, no cached-blur band — a display-sized blur band
            // would starve the atlas). This is an ATLAS-BUDGET decision (skip the
            // blur band); it does NOT imply chromeless — see `app_title_h`.
            || self.windows.is_fullscreen(crate::window_scene::WindowId::AppClient)
    }

    /// The app-client title-bar height per INTENT — `chrome ⟂ policy`. A titled
    /// app keeps `APP_TITLE_H` (the min/max/close bar) whether floating OR
    /// maximized; a `plain` / desktop / fullscreen-intent surface (a shell or a
    /// single-app-OS launcher) is chromeless (0). Maximizing changes the FRAME,
    /// never the chrome, so this is the SSOT for both the create branch and the
    /// content-rect push.
    pub(super) fn app_title_h(&self) -> u32 {
        let chromeless = self.app_intent_style == wire::WIN_STYLE_PLAIN
            || self.app_intent_level == wire::WIN_LEVEL_DESKTOP
            || self.app_intent_mode == wire::WIN_MODE_FULLSCREEN;
        if chromeless {
            0
        } else {
            APP_TITLE_H
        }
    }

    /// R1 layer seam: store the app's material-tagged glass regions
    /// (`OP_SURFACE_LAYERS`, surface-local) and repaint the window so the new
    /// glass composites. A malformed frame is ignored (the app keeps its prior
    /// layers). No reply — the next present reflects it.
    pub(crate) fn handle_surface_layers(&mut self, frame: &[u8]) {
        let mut out = [wire::LayerDesc::default(); wire::MAX_SURFACE_LAYERS];
        let Some(n) = wire::decode_surface_layers(frame, &mut out) else {
            return;
        };
        self.app_layers = out;
        self.app_layer_count = n;
        self.app_win.surface_dirty = true;
        let rect = self.app_window_rect();
        self.queue_dirty_rect(rect);
        let _ = debug_println(&alloc::format!("WINDOWD: app layers={n}"));
    }

    /// Stores the app's dedicated event channel (SEND cap slot moved with an
    /// `OP_SURFACE_EVENTS` frame, execd-attached). A relaunch replaces the
    /// channel — the previous cap is closed, never leaked.
    #[allow(unused_variables)]
    pub(crate) fn attach_app_event_channel(&mut self, send_slot: Option<u32>) {
        #[cfg(nexus_env = "os")]
        {
            let Some(slot) = send_slot else {
                let _ = debug_println("WINDOWD: FAIL app event channel (no cap)");
                return;
            };
            if let Some(old) = self.app_event_channel.replace(slot) {
                let _ = nexus_abi_cap_close(old);
            }
            let _ = debug_println("WINDOWD: app event channel attached");
            // Push the active theme mode NOW (before the app mounts) so it
            // renders with the same tokens as the compositor (dark desktop ⇒
            // dark app). On a live toggle, `push_app_theme` is re-sent.
            self.push_app_theme();
        }
    }

    /// Sends the active theme mode to the app on its event channel (`chrome =
    /// intent ⟂ policy` for colours too: the WM owns the theme, apps follow).
    pub(crate) fn push_app_theme(&mut self) {
        use nexus_display_proto::client_surface as wire;
        let mode = match self.theme_mode {
            crate::theme::ThemeMode::Dark => wire::THEME_DARK,
            crate::theme::ThemeMode::Light => wire::THEME_LIGHT,
        };
        let frame = wire::encode_surface_theme(mode);
        let _ = self.send_app_frame(&frame);
    }

    /// Sends one app-bound frame (input event or surface ack) on the
    /// dedicated event channel. Returns false when no channel is attached
    /// (caller falls back to the shared response endpoint) — a SEND failure
    /// on an attached channel is reported, not silently dropped.
    #[allow(unused_variables)]
    pub(crate) fn send_app_frame(&mut self, frame: &[u8]) -> bool {
        #[cfg(nexus_env = "os")]
        {
            let Some(slot) = self.app_event_channel else { return false };
            let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
            match nexus_abi::ipc_send_v1(slot, &hdr, frame, nexus_abi::IPC_SYS_NONBLOCK, 0) {
                Ok(_) => true,
                Err(_) => {
                    // The channel exists but is full/broken: report it and
                    // claim delivery — falling back to the shared endpoint
                    // would reintroduce the ack race this channel removes.
                    let _ = debug_println("WINDOWD: FAIL app event send");
                    true
                }
            }
        }
        #[cfg(not(nexus_env = "os"))]
        {
            let _ = frame;
            false
        }
    }

    /// Routes a body tap to the surface's owning app process (R3) over the
    /// DEDICATED event channel (the shared response endpoint raced with
    /// inputd's ack drain — a tap there could be consumed by any receiver).
    /// Best-effort non-blocking — input must never stall the compositor.
    /// Markers are honest: `routed` prints only on a delivered send.
    pub(crate) fn send_app_input(&mut self, local_x: i32, local_y: i32) {
        #[cfg(nexus_env = "os")]
        {
            let Some(client) = self.client_surfaces.get() else { return };
            let (x, y) = (local_x.max(0) as u16, local_y.max(0) as u16);
            let frame = nexus_display_proto::client_surface::encode_surface_input(
                client.id,
                nexus_display_proto::client_surface::INPUT_KIND_TAP,
                x,
                y,
            );
            let Some(slot) = self.app_event_channel else {
                let _ = debug_println("WINDOWD: FAIL surface input (no event channel)");
                return;
            };
            let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
            match nexus_abi::ipc_send_v1(slot, &hdr, &frame, nexus_abi::IPC_SYS_NONBLOCK, 0) {
                Ok(_) => {
                    let _ = debug_println("WINDOWD: surface input routed");
                }
                Err(_) => {
                    let _ = debug_println("WINDOWD: FAIL surface input send");
                }
            }
        }
        #[cfg(not(nexus_env = "os"))]
        {
            let _ = (local_x, local_y);
        }
    }
}

/// Thin cap-close shim so the handlers above read cleanly on host builds
/// (where `cap_close` does not exist).
#[cfg(nexus_env = "os")]
fn nexus_abi_cap_close(slot: u32) -> core::result::Result<(), ()> {
    nexus_abi::cap_close(slot).map_err(|_| ())
}

#[cfg(not(nexus_env = "os"))]
fn nexus_abi_cap_close(_slot: u32) -> core::result::Result<(), ()> {
    Ok(())
}
