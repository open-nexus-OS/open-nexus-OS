// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

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
            Err(status) => wire::encode_surface_ack(wire::OP_SURFACE_PRESENT, status, seq),
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
                self.close_app_window();
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
            let blur = self.atlas_alloc.alloc(w, h); // best-effort
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
        for ly in 0..win_h {
            let row_bytes = win_w as usize * 4;
            let row = &mut self.band_scratch[0..stride];
            row[..row_bytes].fill(0);
            if ly < APP_TITLE_H {
                crate::compositor::shell_window::draw_title_bar_row(
                    ly,
                    row,
                    win_w,
                    "App",
                    APP_TITLE_H,
                    APP_CLOSE_W,
                    title_hover,
                    corner_radius,
                    tk,
                )?;
            } else {
                let body_y = ly - APP_TITLE_H;
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
