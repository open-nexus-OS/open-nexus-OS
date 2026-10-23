// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! BOUNDARY: this file is the FLOATING app-window surface rendering + geometry
//! slice of the app-surface service (a pure move out of `app_window.rs`, no
//! behavior change): acquire/release the atlas band, blit the surface rows out
//! of the app's VMO under the WM title bar (ADR-0042 damage-blit), the
//! live-resize title overlay, and the window's atlas-band geometry. It MUST NOT
//! grow window-chrome/sizing/decoration logic — that is the `ui/widgets/window`
//! widget's job (see `app_window.rs` header). Chrome + layout live in the widget
//! + scene graph; this only moves surface bytes into the band.
//! OWNERS: @ui @runtime
//! STATUS: Experimental (TASK-0080D R1)
//! API_STABILITY: Unstable
//! ADR: docs/adr/0042-cross-process-surface-transport.md

use super::app_window::nexus_abi_cap_close;
use super::*;
use crate::surface_regions::SurfaceRegions;

impl DisplayServerRuntime {
    /// Acquire the window's atlas regions + show it: its BASE, its STORE (RFC-0109: a region of
    /// its own, the store's used width, placed wherever it fits) and — where the CPU executor
    /// composes — its blur cache. `None`: no room (the window cannot show). `Some(false)`: shown
    /// without its store, which only a create may accept (`may_drop_store`: the client is told
    /// and paints its scroll layers flat); a re-mount needs the store the client paints into.
    pub(super) fn open_app_window(&mut self, idx: usize, may_drop_store: bool) -> Option<bool> {
        let mut store_placed = true;
        if !self.apps[idx].win.is_mounted() {
            let w = self.apps[idx].win.w;
            let h = self.apps[idx].win.h;
            let client = self.apps[idx]
                .surface_id
                .and_then(|id| self.client_surfaces.get_by_id(id))
                .copied();
            // The base: never shorter than the frame it opens in.
            let base_rows = client.map_or(h, |c| u32::from(c.height)).max(h);
            let Some(content) = self.atlas_alloc.alloc(w, base_rows) else {
                let _ = debug_println(&alloc::format!(
                    "WINDOWD: surface open FAIL atlas (need={}x{} rows_remaining={})",
                    w,
                    base_rows,
                    self.atlas_alloc.rows_remaining()
                ));
                return None;
            };
            let store = match client.filter(|c| c.rows > c.height) {
                Some(c) => {
                    let (sw, sh) = (u32::from(c.store_w), u32::from(c.rows - c.height));
                    let placed = self.atlas_alloc.alloc(sw, sh);
                    if placed.is_none() && !may_drop_store {
                        self.atlas_alloc.free(content);
                        let _ = debug_println(&alloc::format!(
                            "WINDOWD: surface open FAIL atlas (store={sw}x{sh})"
                        ));
                        return None;
                    }
                    store_placed = placed.is_some();
                    placed
                }
                None => None,
            };
            // The allocator recycles freed regions, and `render_app_surface`
            // only ever blits `title_h + band rows` — an allocation taller
            // than what the client fills (frame taller than the negotiated
            // band, mid-re-create) COMPOSITED the previous tenant's pixels
            // as a garbage slab at the window bottom. Zero the whole region
            // once at mount; transparent rows composite as nothing.
            self.zero_atlas_region(&content);
            // The BLUR band stays VISIBLE-sized (the glass clamps to w×h anyway;
            // a tall blur band would starve the atlas). Only a surface that
            // DECLARED desktop/fullscreen presentation skips it (opaque page
            // base, R1 layer path). A freeform window the WM maximized keeps
            // its blur band: its page base stays translucent (the app keys
            // `base_alpha` on the declared mode), so dropping the band here
            // rendered a fullscreened window as unblurred tracing paper over
            // the raw desktop.
            let blur = if self.app_presentation(idx).full_screen || self.gpu_composes {
                None // a GPU that composes blurs and caches the glass itself
            } else {
                self.atlas_alloc.alloc(w, h) // best-effort
            };
            self.apps[idx].win.mount((content, store), blur);
        }
        self.apps[idx].win.visible = true;
        let wid = crate::window_scene::WindowId::App(idx as u8);
        let presentation = self.app_presentation(idx);
        // The surface's DECLARED level resolves its z-band (overlay = above
        // all floating windows); an overlay never steals window focus.
        self.windows.set_role(wid, presentation.role);
        if presentation.role == crate::window_scene::WindowRole::Overlay {
            self.windows.show_unfocused(wid);
        } else {
            self.show_window(wid);
        }
        self.apps[idx].win.surface_dirty = true;
        self.apps[idx].surface_damage = super::surface_layers::DamageRows::FULL; // (re)shown
        let rect = self.app_window_rect(idx);
        self.queue_dirty_rect(rect);
        Some(store_placed)
    }

    /// Reset everything derived from THE SURFACE when a client (re)creates one: its declared
    /// layers (RFC-0109) — regions, scroll layers, washes. All of it is surface-local, so none
    /// of it may survive the surface it describes — a resize is a destroy + create, and the stale
    /// half is exactly what painted the file manager's panes at their old size in the corner of a
    /// maximized window. Fail-closed: a surface has no layers until it declares some.
    pub(super) fn reset_surface_state(&mut self, idx: usize) {
        self.apps[idx].layers.clear();
    }

    /// Zero every pixel of a freshly allocated atlas region (best-effort:
    /// without a framebuffer there is nothing to clear — or to garbage-read).
    /// One-shot at mount; bounded by the region size.
    fn zero_atlas_region(&mut self, surface: &crate::atlas::AtlasSurface) {
        let Some(handle) = self.framebuffer else { return };
        let stride = self.mode.stride as usize;
        let row_bytes = surface.width as usize * 4;
        if self.band_scratch.len() < row_bytes {
            return;
        }
        self.band_scratch[..row_bytes].fill(0);
        let col_off = surface.x as usize * 4;
        for r in 0..surface.height {
            let dst = (surface.abs_row + r) as usize * stride + col_off;
            let _ = vmo_write(handle, dst, &self.band_scratch[..row_bytes]);
        }
    }

    pub(super) fn close_app_window(&mut self, idx: usize) {
        let vacated = self.app_window_rect(idx);
        self.apps[idx].starting = false;
        self.drop_solid(idx);
        self.apps[idx].win.visible = false;
        self.hide_window(crate::window_scene::WindowId::App(idx as u8));
        self.apps[idx].win.end_drag();
        // A closed window gates nobody (TASK-0074 D4) and holds no tile (TASK-0066).
        self.apps[idx].reset_wm_state();
        self.release_app_surface_band(idx);
        self.sync_surface_focus_to_imed();
        // The backdrop blur is destination-so-far: any window whose cached
        // blur was captured while the closed window overlapped it keeps a
        // GHOST of it under the glass. Invalidate every intersecting window's
        // blur cache (GPU re-blur next composite — cheap).
        self.invalidate_overlapping_blur(vacated, idx);
        // W4 slot recycling ("öffnen öffnen schließen … super effizient"): a
        // user-close ENDS the binding — destroy the surface registration
        // (frees the registry entry + the app's VMO capability), release the
        // event channel (cap + nonce table entry), and reset the slot so the
        // NEXT launch reuses it. Without this every close leaked the slot —
        // four opens+closes exhausted the window quota — and the surface/VMO
        // registry grew forever. The parked app process itself is the open
        // reaper (#29); it costs zero CPU (its frame pulses are withheld and
        // its presents hit the stale-id guard).
        if let Some(id) = self.apps[idx].surface_id.take() {
            if let Ok(vmo_slot) = self.client_surfaces.destroy(id) {
                let _ = nexus_abi_cap_close(vmo_slot);
            }
        }
        // RFC-0086: the window left the set — the shell drops its tile marker.
        self.push_window_set();
        #[cfg(nexus_env = "os")]
        if let Some(ch) = self.apps[idx].event_channel.take() {
            if let Some(pos) =
                self.event_channels[..self.event_channels_len].iter().position(|e| e.1 == ch)
            {
                // Compact the nonce table (order-preserving shift).
                for i in pos..self.event_channels_len - 1 {
                    self.event_channels[i] = self.event_channels[i + 1];
                }
                self.event_channels_len -= 1;
            }
            // What waited for the closed window's client goes with its channel (ADR-0075).
            self.forget_client_channel(ch);
            let _ = nexus_abi_cap_close(ch);
        }
        self.apps[idx].frame_pulse_pending = false;
        self.apps[idx].surface_damage = super::surface_layers::DamageRows::FULL;
        self.reset_surface_state(idx);
        // The slot's transform returns to the identity: every present carries every slot's
        // (a free slot's as the identity), so the next tenant never inherits a faded-out state.
        self.apps[idx].transform = WinTransform::IDENTITY;
        self.apps[idx].pending_wm = None;
        self.apps[idx].drag_offset = None;
        self.owe_frame();
        let _ = debug_println(&alloc::format!("WINDOWD: app window closed slot={idx}"));
    }

    /// Invalidate the cached backdrop blur of every OTHER visible window
    /// intersecting `rect` — their destination-so-far blur may hold a ghost
    /// of content that just vanished/moved there (close, minimize).
    pub(super) fn invalidate_overlapping_blur(&mut self, rect: DamageRect, skip_idx: usize) {
        for i in 0..self.apps.len() {
            if i == skip_idx || !self.apps[i].win.visible {
                continue;
            }
            let r = self.app_window_rect(i);
            let intersects = rect.x < r.x + r.width
                && rect.x + rect.width > r.x
                && rect.y < r.y + r.height
                && rect.y + rect.height > r.y;
            if intersects {
                self.apps[i].win.blur_valid = false;
            }
        }
    }

    /// Re-mount every z-visible, non-minimized app window that lost its atlas
    /// band to occlusion residency (fullscreen released it) — idempotent, and
    /// a no-op while a fullscreen window still covers the stack. Called on
    /// leave-fullscreen and after band releases (rows may have freed up).
    pub(super) fn ensure_visible_bands(&mut self) {
        if self.windows.fullscreen_active().is_some() {
            return; // still covered — occluded windows stay bandless
        }
        for idx in 0..self.apps.len() {
            let wid = crate::window_scene::WindowId::App(idx as u8);
            if self.apps[idx].surface_id.is_some()
                && self.apps[idx].win.visible
                && self.windows.is_visible(wid)
                && !self.apps[idx].win.is_mounted()
            {
                let _ = self.open_app_window(idx, false);
            }
        }
    }

    /// The atlas's deepest row ever reached and its rows in use (TASK-0333 6b-2 P0: what a
    /// lazy atlas would hold committed).
    pub(crate) fn atlas_rows(&self) -> (u32, u32) {
        (self.atlas_alloc.peak_rows(), self.atlas_alloc.used_rows())
    }

    /// Free the app window's atlas band(s) WITHOUT touching window state
    /// (visibility, fullscreen, position). Used by the resize/fullscreen
    /// re-create (protocol SURFACE_DESTROY): the band must be reclaimed before
    /// the new surface allocates one, but the window keeps its geometry + mode so
    /// the re-created surface resumes in place. `close_app_window` hides first,
    /// then calls this.
    pub(super) fn release_app_surface_band(&mut self, idx: usize) {
        let rect = self.app_window_rect(idx);
        for region in self.apps[idx].win.unmount().into_iter().flatten().flatten() {
            self.atlas_alloc.free(region);
        }
        self.queue_dirty_rect(rect);
    }

    /// Copies the rows the app's presents named out of its VMO into the window's atlas region —
    /// the ADR-0042 damage-blit, its base's rows and its store's (RFC-0109: a band's new rows,
    /// a patch). Bounded by the surface's rows validated at create.
    pub(super) fn render_app_surface(&mut self, idx: usize) -> Result<(), WindowdError> {
        let Some(handle) = self.framebuffer else {
            return Ok(());
        };
        let Some(base) = self.apps[idx].win.atlas else {
            return Ok(());
        };
        let store = self.apps[idx].win.store;
        // BY ID: `get()` (first live) is ambiguous once several surfaces coexist.
        let Some(client) =
            self.apps[idx].surface_id.and_then(|id| self.client_surfaces.get_by_id(id)).copied()
        else {
            return Ok(());
        };
        let damage = core::mem::replace(
            &mut self.apps[idx].surface_damage,
            super::surface_layers::DamageRows::NONE,
        );
        let regions = SurfaceRegions::of(&client, base, store);
        self.copy_surface_rows(handle, &client, &regions, &damage)
    }

    /// The copy itself — one `vmo_read` and one `vmo_write` per row of each named span — shared
    /// by the windows and the desktop (TASK-0339: timed into `work(copy_rows=`). A base row
    /// lands in the base's region, a store row's used columns in the store's (RFC-0109).
    pub(super) fn copy_surface_rows(
        &mut self,
        handle: Handle,
        client: &crate::client_surface::ClientSurface,
        regions: &SurfaceRegions,
        damage: &super::surface_layers::DamageRows,
    ) -> Result<(), WindowdError> {
        let stride = self.mode.stride as usize;
        if self.band_scratch.len() < stride {
            return Err(WindowdError::BufferLengthMismatch);
        }
        let src_stride = usize::from(client.width) * 4;
        let mut spans = [(0u32, 0u32); 4];
        let n = damage.spans(regions.rows, &mut spans);
        let copy_t0 = nexus_abi::nsec().unwrap_or(0);
        let mut copied = 0u64;
        for &(y0, y1) in &spans[..n] {
            for ly in y0..y1 {
                let Some((row, col, width)) = regions.place(ly) else { continue };
                let row_bytes = width as usize * 4;
                let buf = &mut self.band_scratch[..row_bytes];
                #[cfg(nexus_env = "os")]
                if nexus_abi::vmo_read(client.vmo_slot, ly as usize * src_stride, buf).is_err() {
                    return Err(WindowdError::BufferLengthMismatch);
                }
                #[cfg(not(nexus_env = "os"))]
                let _ = (src_stride, &buf);
                let dst = row as usize * stride + col as usize * 4;
                vmo_write(handle, dst, &self.band_scratch[..row_bytes])
                    .map_err(|_| WindowdError::BufferLengthMismatch)?;
                copied += 1;
            }
        }
        if copied > 0 {
            crate::atlas::note_atlas_content_write();
        }
        crate::compositor::frame_causes::work_copy(
            copied,
            nexus_abi::nsec().unwrap_or(copy_t0).saturating_sub(copy_t0) / 1000,
        );
        Ok(())
    }

    pub(super) fn app_window_rect(&self, idx: usize) -> DamageRect {
        self.apps[idx].win.damage_rect(self.mode.width, self.mode.height)
    }
}
