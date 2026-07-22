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
pub(crate) const APP_WIN_MAX_H: u32 = crate::client_surface::MAX_SURFACE_H as u32 + APP_TITLE_H;
pub(crate) const APP_TITLE_H: u32 = 32;
/// Rounded-corner radius of the floating app window's glass frame.
pub(crate) const APP_WIN_RADIUS: u32 = 12;
pub(crate) const APP_CLOSE_W: u32 = 40;

impl DisplayServerRuntime {
    /// `SURFACE_CREATE`: validate + register the surface, retain the moved
    /// VMO capability, open the app window. Returns the ack frame.
    pub(crate) fn handle_surface_create(
        &mut self,
        frame: &[u8],
        vmo_slot: Option<u32>,
        sender_sid: u64,
    ) -> [u8; wire::SURFACE_ACK_FRAME_LEN] {
        let Some((
            width,
            height,
            format,
            style,
            level,
            mode,
            resizable,
            nonce,
            content_h,
            header_h,
            footer_h,
        )) = wire::decode_surface_create(frame)
        else {
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
        // Declarative routing at CREATE: the connecting app's presentation
        // (its carried intent ⟂ policy) decides the role slot — resolved from
        // THIS frame's values, never from stored state. A DESKTOP-role
        // surface (shell/greeter) gets its OWN slot — id, event channel,
        // full-screen band — fully separate from the floating `app_win`, so
        // the shell and an app window coexist (the singleton collision made
        // "counter startet nicht").
        let presentation = crate::surface_presentation::WindowPresentation::resolve(
            style,
            level,
            mode,
            resizable,
            self.windowing_policy,
        );
        if presentation.role == crate::window_scene::WindowRole::Desktop {
            return self.create_desktop_surface(width, height, format, vmo_slot, nonce);
        }
        // Multi-window (RFC-0065): the create binds to the slot ALREADY holding
        // this client's event channel (a resize re-create must resume in place,
        // keeping geometry + fullscreen), else to a free slot. All slots taken
        // ⇒ honest QUOTA — never hijack another app's window (the singleton-era
        // behavior that made "nur ein Programm gleichzeitig").
        #[cfg(nexus_env = "os")]
        let bound_idx = self
            .event_channel_for(nonce)
            .and_then(|ch| self.apps.iter().position(|a| a.event_channel == Some(ch)));
        // FRESH launch (vs a resize/fullscreen re-create resuming its slot):
        // only a fresh window plays the open transition (Track C3).
        #[cfg(nexus_env = "os")]
        let fresh_launch = bound_idx.is_none();
        #[cfg(not(nexus_env = "os"))]
        let fresh_launch = true;
        #[cfg(nexus_env = "os")]
        let slot_idx = bound_idx.or_else(|| self.free_app_index());
        #[cfg(not(nexus_env = "os"))]
        let slot_idx = self.free_app_index();
        let Some(idx) = slot_idx else {
            let _ = nexus_abi_cap_close(vmo_slot);
            let _ = debug_println("WINDOWD: surface create FAIL (window slots exhausted)");
            return wire::encode_surface_ack(
                wire::OP_SURFACE_CREATE,
                wire::SURFACE_STATUS_QUOTA,
                0,
            );
        };
        // The launch surfaced: stop the wait ring (one waiter done).
        if fresh_launch {
            self.end_cursor_wait();
            // A NEW window follows its declared intent — clear any stale WM
            // mode override left in the reused slot.
            self.apps[idx].wm_mode = None;
        }
        let wid = crate::window_scene::WindowId::App(idx as u8);
        // The declared intent rides ATOMICALLY on the create frame (the old
        // separate pre-create OP_SURFACE_INTENT raced concurrent connects).
        // It binds to THIS window's slot only.
        self.apps[idx].intent_style = style;
        self.apps[idx].intent_level = level;
        self.apps[idx].intent_mode = mode;
        self.apps[idx].intent_resizable = resizable;
        self.apps[idx].owner_sid = sender_sid;
        // WebRender compositor-scroll band geometry (rides atomically on CREATE
        // so the tall band is alloc'd with the right size). `content_h == 0` ⇒
        // the surface is NOT scrollable — byte-identical to the pre-scroll path.
        // A scrollable surface gets a per-slot scroll id (`slot_index + 1`,
        // bounded by `MAX_SCROLL_IDS`); windowd owns its scroll position.
        self.apps[idx].content_h = u32::from(content_h);
        self.apps[idx].header_h = u32::from(header_h);
        self.apps[idx].footer_h = u32::from(footer_h);
        self.apps[idx].scroll_rows = 0;
        self.apps[idx].scroll_momentum =
            animation::ScrollMomentum::new(animation::ScrollConfig::default());
        self.apps[idx].scroll_last_ns = 0;
        self.apps[idx].scroll_id =
            if content_h > 0 && (idx + 1) <= MAX_SCROLL_IDS { (idx as u32) + 1 } else { 0 };
        match self.client_surfaces.create(width, height, format, vmo_slot) {
            Ok(id) => {
                self.apps[idx].surface_id = Some(id);
                #[cfg(nexus_env = "os")]
                if let Some(ch) = self.event_channel_for(nonce) {
                    self.apps[idx].event_channel = Some(ch);
                } else {
                    let _ = debug_println("WINDOWD: FAIL surface bind (no channel for nonce)");
                }
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
                self.apps[idx].win.title_h = self.app_title_h(idx);
                if self.app_presentation(idx).docked_bottom {
                    // OVERLAY dock (OSK): bottom edge, full width, no chrome.
                    let h = u32::from(height);
                    #[allow(clippy::cast_possible_wrap)]
                    self.apps[idx].win.set_frame(
                        0,
                        self.mode.height.saturating_sub(h) as i32,
                        self.mode.width,
                        h,
                    );
                } else if self.app_presentation(idx).full_screen || self.windows.is_fullscreen(wid)
                {
                    // Full-screen presentation (declared desktop level /
                    // fullscreen mode — the shell/greeter base or a kiosk app) or
                    // the transient user-maximize: cover the WORK AREA. The
                    // shell/greeter (desktop level) still spans the whole
                    // display; a normal fullscreen APP window stops above the
                    // desktop taskbar (tablet: bottom edge) and sits BEHIND the
                    // shell top bar (full height at the top — the bar
                    // composites above it and stays usable).
                    let h = if self.apps[idx].intent_level == wire::WIN_LEVEL_DESKTOP {
                        self.mode.height
                    } else {
                        self.work_area_h()
                    };
                    self.apps[idx].win.set_frame(0, 0, self.mode.width, h);
                } else {
                    let content_h = u32::from(height).saturating_add(self.apps[idx].win.title_h);
                    let (wx, wy) = (self.apps[idx].win.x, self.apps[idx].win.y);
                    self.apps[idx].win.set_frame(wx, wy, u32::from(width), content_h);
                }
                if !self.open_app_window(idx) {
                    // Atlas exhausted: roll the registration back fail-closed
                    // AND hide the window — the destroy path deliberately
                    // keeps `visible=true` for re-creates, so a failed
                    // re-open would otherwise leave a ZOMBIE (advertised in
                    // the hit/composite stack, no surface, no band: eats
                    // clicks, renders nothing).
                    let _ = self.client_surfaces.destroy(id);
                    let _ = nexus_abi_cap_close(vmo_slot);
                    self.apps[idx].surface_id = None;
                    self.apps[idx].win.visible = false;
                    self.windows.hide(wid);
                    let _ =
                        nexus_abi::debug_write(b"windowd: surface re-open QUOTA (window hidden)\n");
                    return wire::encode_surface_ack(
                        wire::OP_SURFACE_CREATE,
                        wire::SURFACE_STATUS_QUOTA,
                        0,
                    );
                }
                // Track C3: a FRESH floating window fades+scales in (the
                // decided enter motion); re-creates resume without replay.
                if fresh_launch
                    && !self.app_presentation(idx).full_screen
                    && !self.windows.is_fullscreen(wid)
                {
                    self.start_open_transition(idx);
                }
                // The freshly created band matches the frame again — the
                // live-resize title overlay (if any) retires here (TASK #23).
                self.update_app_title_overlay(idx);
                // A RE-CREATE (mode switch / resize) can shrink the frame:
                // repaint the whole scene so vacated area doesn't keep the
                // old band's pixels (fullscreen→freeform left stale rows).
                if !fresh_launch {
                    self.queue_full_frame_damage();
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

    /// Sends a frame on the event channel bound to `nonce` — the CREATE-ack
    /// route: the create frame carries the client's nonce, so the reply
    /// reaches the CREATING client even when it is not the floating window
    /// (the old `send_app_frame` ack path sent DESKTOP create-acks to the
    /// floating channel — None for the first app → the greeter app-host hung
    /// in `recv_ack` forever and its event loop never armed: dead buttons).
    pub(crate) fn send_frame_for_nonce(&mut self, nonce: u64, frame: &[u8]) -> bool {
        #[cfg(nexus_env = "os")]
        {
            let Some(slot) = self.event_channel_for(nonce) else { return false };
            let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
            match nexus_abi::ipc_send_v1(slot, &hdr, frame, nexus_abi::IPC_SYS_NONBLOCK, 0) {
                Ok(_) => true,
                Err(_) => {
                    let _ = debug_println("WINDOWD: FAIL nonce event send");
                    true // the channel exists — do not fall back to the shared endpoint
                }
            }
        }
        #[cfg(not(nexus_env = "os"))]
        {
            let _ = (nonce, frame);
            false
        }
    }

    /// Sends a frame on the channel of the surface's OWNER — the PRESENT/
    /// DESTROY-ack route: the desktop surface has its own channel; everything
    /// else is the floating window's.
    pub(crate) fn send_surface_frame(&mut self, surface_id: u32, frame: &[u8]) -> bool {
        #[cfg(nexus_env = "os")]
        {
            if self.desktop_surface_id == Some(surface_id) {
                let Some(slot) = self.desktop_channel else { return false };
                let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
                match nexus_abi::ipc_send_v1(slot, &hdr, frame, nexus_abi::IPC_SYS_NONBLOCK, 0) {
                    Ok(_) => return true,
                    Err(_) => {
                        let _ = debug_println("WINDOWD: FAIL desktop event send");
                        return true; // channel exists — no shared-endpoint fallback
                    }
                }
            }
            let Some(idx) = self.app_index_by_surface(surface_id) else { return false };
            self.send_app_frame(idx, frame)
        }
        #[cfg(not(nexus_env = "os"))]
        {
            let _ = (surface_id, frame);
            false
        }
    }

    /// `SURFACE_PRESENT`: validate seq + damage, mark the window body dirty
    /// (the render path blits from the VMO), queue the damage. Acks the seq.
    /// Invalidate the CACHED glass backdrop of every visible app window
    /// (except `skip`) whose frame intersects `rect`. The cache is a
    /// snapshot of what was BEHIND the window when it was built — content
    /// changing underneath (another window presenting, the desktop
    /// repainting, a drag) leaves it stale, which showed as "blur shows the
    /// wallpaper, not the actual background". Over-invalidation is safe:
    /// the next present just re-blurs that region live.
    pub(crate) fn invalidate_blur_over(&mut self, rect: DamageRect, skip: Option<usize>) {
        for i in 0..self.apps.len() {
            if Some(i) == skip || !self.apps[i].win.visible || self.apps[i].win.blur_valid == false
            {
                continue;
            }
            let f = self.app_window_rect(i);
            let x0 = rect.x.max(f.x);
            let y0 = rect.y.max(f.y);
            let x1 = (rect.x + rect.width).min(f.x + f.width);
            let y1 = (rect.y + rect.height).min(f.y + f.height);
            if x1 > x0 && y1 > y0 {
                self.apps[i].win.blur_valid = false;
            }
        }
    }

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
                // recorded optimization — ADR-0042). Routed BY ID: a desktop
                // present repaints the base layer; a floating present the window.
                if self.desktop_surface_id == Some(surface_id) {
                    self.desktop_dirty = true;
                    // Damage discipline (the retained-plane perf contract):
                    // honor the client's damage rects — union rows for the
                    // band blit, per-rect screen damage for the composite.
                    // Full-frame work only when the client declared none.
                    if count == 0 {
                        self.desktop_dirty_rows = (0, u32::MAX);
                        self.queue_full_frame_damage();
                        let (w, h) = (self.mode.width, self.mode.height);
                        self.invalidate_blur_over(
                            DamageRect { x: 0, y: 0, width: w, height: h },
                            None,
                        );
                    } else {
                        for r in &rects[..count] {
                            let y0 = r.y as u32;
                            let y1 = y0.saturating_add(r.height as u32);
                            let (s0, s1) = self.desktop_dirty_rows;
                            self.desktop_dirty_rows = (s0.min(y0), s1.max(y1));
                            // The desktop is full-screen at the origin:
                            // surface-local == display coordinates.
                            let dr = DamageRect {
                                x: r.x as u32,
                                y: r.y as u32,
                                width: r.width as u32,
                                height: r.height as u32,
                            };
                            self.queue_dirty_rect(dr);
                            // Windows above the repainted desktop region hold
                            // a stale backdrop snapshot — re-blur them.
                            self.invalidate_blur_over(dr, None);
                        }
                    }
                } else if let Some(idx) = self.app_index_by_surface(surface_id) {
                    // Damage-bounded blit (ADR-0042 / the 120Hz contract):
                    // honor the client's damage rects — union their ROW span
                    // for the band blit and queue only that screen band. A
                    // present with no rects, or a scrollable (banded) surface
                    // (band rows ≠ visible rows), stays a FULL re-blit.
                    let bounded: Option<(u32, u32)> = if self.apps[idx].scroll_id == 0 && count > 0
                    {
                        let mut rows: Option<(u32, u32)> = None;
                        for r in &rects[..count] {
                            let y0 = r.y as u32;
                            let y1 = y0.saturating_add(r.height as u32);
                            rows = Some(match rows {
                                Some((s0, s1)) => (s0.min(y0), s1.max(y1)),
                                None => (y0, y1),
                            });
                        }
                        rows
                    } else {
                        None
                    };
                    // Merge with a still-pending blit; FULL (None) wins.
                    self.apps[idx].surface_dirty_rows = if self.apps[idx].win.surface_dirty {
                        match (self.apps[idx].surface_dirty_rows, bounded) {
                            (Some((a0, a1)), Some((b0, b1))) => Some((a0.min(b0), a1.max(b1))),
                            _ => None,
                        }
                    } else {
                        bounded
                    };
                    self.apps[idx].win.surface_dirty = true;
                    let rect = self.app_window_rect(idx);
                    // Other glass windows overlapping this one see it through
                    // their backdrop — their cached blur is now stale.
                    self.invalidate_blur_over(rect, Some(idx));
                    match self.apps[idx].surface_dirty_rows {
                        // Bounded: damage only the presented body rows on
                        // screen (body starts under the WM title bar).
                        Some((y0, y1)) => {
                            let top = rect
                                .y
                                .saturating_add(self.apps[idx].win.title_h)
                                .saturating_add(y0);
                            let bottom = (rect.y + rect.height).min(top.saturating_add(y1 - y0));
                            if bottom > top {
                                self.queue_dirty_rect(DamageRect {
                                    x: rect.x,
                                    y: top,
                                    width: rect.width,
                                    height: bottom - top,
                                });
                            }
                        }
                        None => self.queue_dirty_rect(rect),
                    }
                } else {
                    // Stale surface (e.g. the retired greeter's after the
                    // shell replaced the desktop slot): ack, queue nothing —
                    // damaging the floating window for a foreign id painted
                    // the wrong surface.
                    let _ = debug_println(&alloc::format!(
                        "WINDOWD: surface present stale id={surface_id} (ignored)"
                    ));
                }
                // Bounded proof marker: the first few presents show the chain
                // is live; per-present formatting at hover/animation rates
                // floods the UART and leaks on the non-freeing bump heap.
                if self.app_present_markers < 8 {
                    self.app_present_markers += 1;
                    let _ = debug_println(&alloc::format!(
                        "WINDOWD: surface presented id={surface_id} seq={seq}"
                    ));
                }
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
                // A dying surface takes its text focus with it — and the OSK
                // band must follow (RFC-0075: never a keyboard without a
                // field; e.g. the greeter is destroyed at session start).
                if self.text_focus.map(|f| f.surface_id) == Some(surface_id) {
                    self.text_focus = None;
                    self.update_osk_visibility();
                }
                if self.desktop_surface_id == Some(surface_id) {
                    // The desktop surface dropped (shell exit / re-create): free
                    // its band + hide the base layer until a new one connects.
                    self.desktop_surface_id = None;
                    if let Some(band) = self.desktop_band.take() {
                        self.atlas_alloc.free(band);
                    }
                    self.hide_window(crate::window_scene::WindowId::Desktop);
                    self.queue_full_frame_damage();
                    let _ = debug_println(&alloc::format!(
                        "WINDOWD: desktop surface destroyed id={surface_id}"
                    ));
                    return wire::encode_surface_ack(
                        wire::OP_SURFACE_DESTROY,
                        wire::SURFACE_STATUS_OK,
                        surface_id,
                    );
                }
                if let Some(idx) = self.app_index_by_surface(surface_id) {
                    self.apps[idx].surface_id = None;
                    // A protocol SURFACE_DESTROY is the app dropping its surface —
                    // during a resize/fullscreen negotiation it re-creates one
                    // immediately (same slot via its event channel). Free the atlas
                    // band ONLY; do NOT hide the window, which would clear its
                    // fullscreen flag (see `window_scene::hide`) and re-add the
                    // title bar on the re-create (atlas over-alloc, the "fullscreen
                    // makes everything vanish" bug). The user-close path (× button)
                    // calls `close_app_window` directly, not this.
                    self.release_app_surface_band(idx);
                    // Rows freed: occluded windows waiting on occlusion
                    // residency may re-mount now (no-op under fullscreen).
                    self.ensure_visible_bands();
                }
                let _ =
                    debug_println(&alloc::format!("WINDOWD: surface destroyed id={surface_id}"));
                wire::encode_surface_ack(
                    wire::OP_SURFACE_DESTROY,
                    wire::SURFACE_STATUS_OK,
                    surface_id,
                )
            }
            Err(status) => wire::encode_surface_ack(wire::OP_SURFACE_DESTROY, status, surface_id),
        }
    }

    /// The app surface's resolved presentation — declared intent ⟂ the
    /// environment's windowing policy, via the ONE host-tested resolver
    /// (`surface_presentation`). All compositing decisions (chrome, z-band,
    /// full-screen, resize) read THIS; nothing re-derives from raw intent tags.
    pub(super) fn app_presentation(
        &self,
        idx: usize,
    ) -> crate::surface_presentation::WindowPresentation {
        crate::surface_presentation::WindowPresentation::resolve(
            self.apps[idx].intent_style,
            self.apps[idx].intent_level,
            // WM override (app-menu mode switch) wins over the declared
            // intent; a fresh launch cleared it back to the intent.
            self.apps[idx].wm_mode.unwrap_or(self.apps[idx].intent_mode),
            self.apps[idx].intent_resizable,
            self.windowing_policy,
        )
    }

    /// The app-client title-bar height — a pure client of the declarative
    /// presentation: chrome resolved from `intent ⟂ policy` (a titled app keeps
    /// `APP_TITLE_H` floating OR maximized; plain/desktop/fullscreen intent and
    /// the Kiosk policy are chromeless). Maximizing changes the FRAME, never the
    /// chrome. SSOT for the create branch and the content-rect push.
    pub(super) fn app_title_h(&self, idx: usize) -> u32 {
        if self.app_presentation(idx).has_chrome {
            APP_TITLE_H
        } else {
            0
        }
    }

    /// R1 layer seam: store the app's material-tagged glass regions
    /// (`OP_SURFACE_LAYERS`, surface-local) and repaint the window so the new
    /// glass composites. A malformed frame is ignored (the app keeps its prior
    /// layers). No reply — the next present reflects it.
    pub(crate) fn handle_surface_layers(&mut self, frame: &[u8]) {
        let mut out = [wire::LayerDesc::default(); wire::MAX_SURFACE_LAYERS];
        let Some((surface_id, n)) = wire::decode_surface_layers(frame, &mut out) else {
            return;
        };
        // BY ID: the desktop shell's glass regions composite over the
        // wallpaper (Desktop arm); a floating app's over its window band.
        if self.desktop_surface_id == Some(surface_id) {
            self.desktop_layers = out;
            self.desktop_layer_count = n;
            self.desktop_dirty = true;
            self.desktop_dirty_rows = (0, u32::MAX);
            self.queue_full_frame_damage();
            let _ = debug_println(&alloc::format!("WINDOWD: desktop layers={n}"));
        } else if let Some(idx) = self.app_index_by_surface(surface_id) {
            self.apps[idx].layers = out;
            self.apps[idx].layer_count = n;
            self.apps[idx].win.surface_dirty = true;
            self.apps[idx].surface_dirty_rows = None; // layer set changed: full
            let rect = self.app_window_rect(idx);
            self.queue_dirty_rect(rect);
            let _ = debug_println(&alloc::format!("WINDOWD: app layers={n}"));
        }
    }

    /// Stores the app's dedicated event channel (SEND cap slot moved with an
    /// `OP_SURFACE_EVENTS` frame, execd-attached). A relaunch replaces the
    /// channel — the previous cap is closed, never leaked.
    #[allow(unused_variables)]
    /// The event channel bound to `nonce`, if attached. Non-consuming — a
    /// resize RE-create binds the same nonce again.
    #[cfg(nexus_env = "os")]
    pub(super) fn event_channel_for(&self, nonce: u64) -> Option<u32> {
        self.event_channels[..self.event_channels_len].iter().find(|e| e.0 == nonce).map(|e| e.1)
    }

    pub(crate) fn attach_app_event_channel(&mut self, send_slot: Option<u32>, nonce: Option<u64>) {
        #[cfg(not(nexus_env = "os"))]
        let _ = nonce;
        #[cfg(nexus_env = "os")]
        {
            let Some(slot) = send_slot else {
                let _ = debug_println("WINDOWD: FAIL app event channel (no cap)");
                return;
            };
            let Some(nonce) = nonce else {
                let _ = debug_println("WINDOWD: FAIL app event channel (no nonce)");
                let _ = nexus_abi_cap_close(slot);
                return;
            };
            // Bind nonce → channel (replace same nonce; LRU-replace when full).
            if let Some(e) =
                self.event_channels[..self.event_channels_len].iter_mut().find(|e| e.0 == nonce)
            {
                let _ = nexus_abi_cap_close(e.1);
                e.1 = slot;
            } else if self.event_channels_len < self.event_channels.len() {
                self.event_channels[self.event_channels_len] = (nonce, slot);
                self.event_channels_len += 1;
            } else {
                // Bounded: replace the OLDEST entry (index 0), shift left.
                let _ = nexus_abi_cap_close(self.event_channels[0].1);
                self.event_channels.rotate_left(1);
                let last = self.event_channels.len() - 1;
                self.event_channels[last] = (nonce, slot);
            }
            // Nonce logged: desktop-bind-race triage matches attach vs deferred bind.
            let _ = debug_println(&alloc::format!(
                "WINDOWD: app event channel attached nonce={nonce:#x}"
            ));
            // Push theme + shell profile + region NOW (before the app
            // mounts) so it renders with the compositor's tokens, the right
            // `ui/platform/<profile>/` arms and correct clock/locale data
            // from the first frame (see `region.rs::send_attach_pushes`).
            self.send_attach_pushes(slot);
            // Complete a desktop bind that raced ahead of this attach (if any).
            self.complete_deferred_desktop_bind(nonce, slot);
        }
    }

    /// Active shell profile as its wire tag (from the SystemUI shell config SSOT).
    pub(crate) fn shell_profile_wire(&self) -> u8 {
        use nexus_display_proto::client_surface as wire;
        match self.shell_config.profile_id.as_str() {
            "tablet" => wire::PROFILE_TABLET,
            "phone" => wire::PROFILE_PHONE,
            "tv" => wire::PROFILE_TV,
            _ => wire::PROFILE_DESKTOP,
        }
    }

    /// Sends the active shell profile to EVERY connected app-host (floating
    /// window, desktop shell, pending channels) — the live Desktop/Tablet
    /// switch; apps re-mount so their platform override arms re-select.
    pub(crate) fn push_app_profile(&mut self) {
        use nexus_display_proto::client_surface as wire;
        let frame = wire::encode_surface_profile(self.shell_profile_wire());
        #[cfg(nexus_env = "os")]
        {
            let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
            if let Some(slot) = self.desktop_channel {
                let _ = nexus_abi::ipc_send_v1(slot, &hdr, &frame, nexus_abi::IPC_SYS_NONBLOCK, 0);
            }
            for e in &self.event_channels[..self.event_channels_len] {
                let _ = nexus_abi::ipc_send_v1(e.1, &hdr, &frame, nexus_abi::IPC_SYS_NONBLOCK, 0);
            }
        }
    }

    /// Sends the active theme mode to the app on its event channel (`chrome =
    /// intent ⟂ policy` for colours too: the WM owns the theme, apps follow).
    /// The packed theme byte for `OP_SURFACE_THEME` pushes: low nibble =
    /// mode, high nibble = the accent-palette index (`pack_theme`).
    pub(crate) fn theme_wire_byte(&self) -> u8 {
        use nexus_display_proto::client_surface as wire;
        let mode = match self.theme_mode {
            crate::theme::ThemeMode::Dark => wire::THEME_DARK,
            crate::theme::ThemeMode::Light => wire::THEME_LIGHT,
        };
        wire::pack_theme(mode, self.theme_accent)
    }

    pub(crate) fn push_app_theme(&mut self) {
        use nexus_display_proto::client_surface as wire;
        let frame = wire::encode_surface_theme(self.theme_wire_byte());
        // Live re-theme reaches EVERY connected app-host: every window's channel
        // is in `event_channels` (nonce-bound), the desktop's is separate.
        #[cfg(nexus_env = "os")]
        {
            let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
            if let Some(slot) = self.desktop_channel {
                let _ = nexus_abi::ipc_send_v1(slot, &hdr, &frame, nexus_abi::IPC_SYS_NONBLOCK, 0);
            }
            for e in &self.event_channels[..self.event_channels_len] {
                let _ = nexus_abi::ipc_send_v1(e.1, &hdr, &frame, nexus_abi::IPC_SYS_NONBLOCK, 0);
            }
        }
    }

    /// Sends one app-bound frame (input event or surface ack) on the
    /// dedicated event channel. Returns false when no channel is attached
    /// (caller falls back to the shared response endpoint) — a SEND failure
    /// on an attached channel is reported, not silently dropped.
    #[allow(unused_variables)]
    pub(crate) fn send_app_frame(&mut self, idx: usize, frame: &[u8]) -> bool {
        #[cfg(nexus_env = "os")]
        {
            let Some(slot) = self.apps[idx].event_channel else { return false };
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
    /// Routes a wheel notch to an app window (`INPUT_KIND_WHEEL`): the signed
    /// delta rides the wire `y` field UNCLAMPED (`wheel_delta_to_wire` — the
    /// `max(0)` clamp of the pointer kinds would destroy negative deltas).
    /// Single-shot NONBLOCK: dropped wheel under pressure is correct.
    pub(crate) fn send_app_wheel(&mut self, idx: usize, local_x: u16, wire_delta: u16) {
        #[cfg(nexus_env = "os")]
        {
            let Some(client) =
                self.apps[idx].surface_id.and_then(|id| self.client_surfaces.get_by_id(id))
            else {
                return;
            };
            let frame = nexus_display_proto::client_surface::encode_surface_input(
                client.id,
                nexus_display_proto::client_surface::INPUT_KIND_WHEEL,
                local_x,
                wire_delta,
            );
            if let Some(slot) = self.apps[idx].event_channel {
                let _ = send_input_frame(slot, &frame, false);
            }
        }
        #[cfg(not(nexus_env = "os"))]
        {
            let _ = (idx, local_x, wire_delta);
        }
    }

    /// `OP_SURFACE_FRAME_REQ`: arm the one-shot frame pulse for the owning
    /// window (desktop surface included via its own flag-less immediate path:
    /// the desktop is composited every frame anyway, so its pulse sends on
    /// the next flush like any window's).
    pub(crate) fn handle_surface_frame_req(&mut self, frame: &[u8]) {
        let Some(surface_id) = nexus_display_proto::client_surface::decode_surface_frame_req(frame)
        else {
            return;
        };
        if let Some(idx) = self.app_index_by_surface(surface_id) {
            self.apps[idx].frame_pulse_pending = true;
        } else if self.desktop_surface_id == Some(surface_id) {
            self.desktop_frame_pulse = true;
        }
    }

    /// Whether any VISIBLE client is waiting on a frame pulse — while true the
    /// compositor keeps its 120Hz pacer armed: the pulses ARE the vsync the
    /// animating client ticks on (without this, a scroll ease degraded to
    /// the client's coarse recv-timeout fallback — "kann dem Scroll nicht
    /// mit den Augen folgen"). A HIDDEN window's pending request is parked
    /// (see [`Self::flush_frame_pulses`]) and must NOT arm the pacer — a
    /// closed animated window would otherwise keep windowd spinning at 120Hz
    /// forever with nothing to draw.
    pub(crate) fn has_frame_pulse_clients(&self) -> bool {
        self.desktop_frame_pulse
            || self.apps.iter().enumerate().any(|(idx, a)| {
                a.frame_pulse_pending
                    && a.win.visible
                    && self.windows.is_visible(crate::window_scene::WindowId::App(idx as u8))
            })
    }

    /// Send the armed frame pulses (once per composited frame, after the
    /// present work): ONE `OP_SURFACE_FRAME` per requesting client, then the
    /// request clears — the Choreographer one-shot contract. NONBLOCK +
    /// droppable: a lost pulse only delays one physics tick; the client's
    /// timeout fallback keeps motion alive.
    pub(crate) fn flush_frame_pulses(&mut self) {
        #[cfg(nexus_env = "os")]
        {
            for idx in 0..self.apps.len() {
                if !self.apps[idx].frame_pulse_pending {
                    continue;
                }
                // Visibility gate (the Choreographer contract: the COMPOSITOR
                // owns animation pacing): a closed/minimized/hidden window gets
                // NO pulses — the request stays PENDING at zero cost, and the
                // first flush after re-expose resumes the chain. Without this a
                // closed window running a continuous animation (widget breathe)
                // kept rendering+presenting invisibly forever — every
                // open/close stacked a permanent ~20Hz zombie load (the "mouse
                // gets slower the longer I use the system" report).
                if !self.apps[idx].win.visible
                    || !self.windows.is_visible(crate::window_scene::WindowId::App(idx as u8))
                {
                    continue;
                }
                self.apps[idx].frame_pulse_pending = false;
                let Some(id) = self.apps[idx].surface_id else { continue };
                let Some(slot) = self.apps[idx].event_channel else { continue };
                let frame = nexus_display_proto::client_surface::encode_surface_frame(id);
                let _ = send_input_frame(slot, &frame, false);
            }
            if self.desktop_frame_pulse {
                self.desktop_frame_pulse = false;
                if let (Some(id), Some(slot)) = (self.desktop_surface_id, self.desktop_channel) {
                    let frame = nexus_display_proto::client_surface::encode_surface_frame(id);
                    let _ = send_input_frame(slot, &frame, false);
                }
            }
        }
    }

    pub(crate) fn send_app_input(&mut self, idx: usize, local_x: i32, local_y: i32) {
        self.send_app_input_kind(
            idx,
            nexus_display_proto::client_surface::INPUT_KIND_TAP,
            local_x,
            local_y,
        );
    }

    /// `send_app_input` for any input kind. Taps keep their honest routed/FAIL
    /// markers; MOVE/LEAVE are frame-rate hover traffic and stay silent.
    pub(crate) fn send_app_input_kind(&mut self, idx: usize, kind: u8, local_x: i32, local_y: i32) {
        #[cfg(nexus_env = "os")]
        {
            let is_tap = kind == nexus_display_proto::client_surface::INPUT_KIND_TAP;
            let Some(client) =
                self.apps[idx].surface_id.and_then(|id| self.client_surfaces.get_by_id(id))
            else {
                return;
            };
            let (x, y) = (local_x.max(0) as u16, local_y.max(0) as u16);
            let frame =
                nexus_display_proto::client_surface::encode_surface_input(client.id, kind, x, y);
            let Some(slot) = self.apps[idx].event_channel else {
                if is_tap {
                    let _ = debug_println("WINDOWD: FAIL surface input (no event channel)");
                }
                return;
            };
            if send_input_frame(slot, &frame, is_tap) {
                if is_tap {
                    let _ = debug_println("WINDOWD: surface input routed");
                }
            } else if is_tap {
                let _ = debug_println("WINDOWD: FAIL surface input send");
            }
        }
        #[cfg(not(nexus_env = "os"))]
        {
            let _ = (idx, kind, local_x, local_y);
        }
    }
}

/// Sends one input frame. TAPS RETRY BOUNDED (~40ms yield loop): a click is
/// rare and must never be dropped behind hover-MOVE spam when the client's
/// queue is momentarily full (the "nothing is clickable" bug — the user's
/// pre-click mouse motion filled the queue). MOVE/LEAVE stay single-shot
/// NONBLOCK — dropping motion under pressure is correct.
#[cfg(nexus_env = "os")]
pub(super) fn send_input_frame(slot: u32, frame: &[u8], is_tap: bool) -> bool {
    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
    let attempts = if is_tap { 400 } else { 1 };
    for i in 0..attempts {
        match nexus_abi::ipc_send_v1(slot, &hdr, frame, nexus_abi::IPC_SYS_NONBLOCK, 0) {
            Ok(_) => return true,
            Err(nexus_abi::IpcError::QueueFull) if i + 1 < attempts => {
                let _ = nexus_abi::yield_();
            }
            Err(_) => return false,
        }
    }
    false
}

/// Thin cap-close shim so the handlers above read cleanly on host builds
/// (where `cap_close` does not exist).
#[cfg(nexus_env = "os")]
pub(super) fn nexus_abi_cap_close(slot: u32) -> core::result::Result<(), ()> {
    nexus_abi::cap_close(slot).map_err(|_| ())
}

#[cfg(not(nexus_env = "os"))]
pub(super) fn nexus_abi_cap_close(_slot: u32) -> core::result::Result<(), ()> {
    Ok(())
}
