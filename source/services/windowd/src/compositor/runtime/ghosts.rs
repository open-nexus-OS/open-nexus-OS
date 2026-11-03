// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: windowd compositor runtime — the GHOSTS of a restart (RFC-0112 Phase 3, TASK-0348
//! P9). At its start — after the framebuffer grant and its GPU context, before its first frame —
//! a windowd asks gpud for the app surfaces the compositor before it composed
//! (`OP_SURFACE_ADOPT`): each comes back with its object and the place its body had, and becomes
//! a ghost — a window slot whose RETIRING surface is the adopted one (`client_surface::adopt`),
//! framed where the body was, in the old draw order; the desktop's object becomes the desktop's
//! retiring surface. The frames show the apps' last pixels at once, so a restart never shows an
//! empty desktop. An owner's rejoin takes its ghost over (`app_window::handle_surface_create` →
//! `take_ghost_slot`): its new surface presents into the ghost's slot and the ghost retires the
//! usual way. execd names the apps it told (`OP_APPS_ALIVE`, execd's kernel id alone): a ghost
//! of any other owner closes. Never a clock.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the pick host-tested (`crate::ghost_pick`), the ghost surface
//!   (`crate::client_surface`), the wire (`nexus_display_proto::surface_adopt`); the many-windows
//!   lane's `windowd: restarted (ghosts=` after its restart chord

use super::surface_buffers::Owner;
use super::*;
use crate::client_surface::SurfaceBuffer;
use crate::ghost_pick::{self, Ghost};
use crate::window_scene::WindowId;
use nexus_display_proto::surface_adopt::{self as wire, Adopted};
use nexus_display_proto::surface_restart::{decode_apps_alive, APPS_ALIVE_MAX};

impl DisplayServerRuntime {
    /// At start: the objects gpud kept from the compositor before, adopted as ghosts. Says
    /// `windowd: restarted (ghosts=N)` when a compositor ran before this one.
    #[cfg(nexus_env = "os")]
    pub(crate) fn adopt_ghosts(&mut self) {
        use nexus_ipc::exchange::{recv_response_matching, send_request};
        let route = nexus_service_topology::slots::windowd::GPUD;
        let mut entries: Vec<Adopted> = Vec::new();
        let mut restarted = false;
        loop {
            if send_request(route.send, &wire::encode_adopt_request()).is_err() {
                return;
            }
            let mut frame = [0u8; 512];
            let Ok((n, moved)) = recv_response_matching(route.recv, &mut frame, wire::is_adopted)
            else {
                return;
            };
            if let Some(cap) = moved {
                let _ = nexus_abi::cap_close(cap);
            }
            let mut page = [NONE; wire::ADOPT_PAGE_MAX];
            let n = n.min(frame.len());
            let Some((was_restart, remaining, count)) =
                wire::decode_adopted(&frame[..n], &mut page)
            else {
                let _ = debug_println("windowd: FAIL adopt (a page refused)");
                return;
            };
            restarted |= was_restart;
            if entries.try_reserve(count).is_err() {
                break;
            }
            entries.extend_from_slice(&page[..count]);
            if remaining == 0 || count == 0 {
                break;
            }
        }
        // Back to front, as the last frame drew them: each ghost shown above the one before.
        entries.sort_unstable_by_key(|a| a.z);
        let ghosts = entries.iter().filter(|a| self.adopt_ghost(a)).count();
        if restarted {
            let _ = debug_println(&alloc::format!("windowd: restarted (ghosts={ghosts})"));
        }
    }

    #[cfg(nexus_env = "os")]
    /// One adopted object as a ghost: the desktop's retiring surface, or a window slot framed
    /// where the body was. `false`: refused — its object goes back to gpud.
    fn adopt_ghost(&mut self, a: &Adopted) -> bool {
        let buffer = SurfaceBuffer {
            source: a.buffer,
            stride: a.stride,
            width: a.width,
            rows: a.rows,
            slots: a.slots,
            cache_block: a.cache_block,
        };
        let Ok(id) = self.client_surfaces.adopt((a.w, a.h), buffer, a.slot) else {
            self.free_surface_buffer(a.buffer);
            return false;
        };
        if a.window_key == super::slots::DESKTOP_KEY {
            // One desktop ghost: the base layer under every window, the shell's last frame.
            if self.buffers.desktop_retiring.is_some() {
                self.drop_surface(id);
                return false;
            }
            self.buffers.desktop_retiring = Some(id);
            self.buffers.desktop_retiring_layers = super::surface_layers::SurfaceLayers::EMPTY;
            self.buffers.desktop_retiring_owner = a.owner;
            self.show_window(WindowId::Desktop);
            self.queue_full_frame_damage();
            return true;
        }
        let Some(idx) = self.take_app_slot() else {
            self.drop_surface(id);
            return false;
        };
        let slot = &mut self.apps[idx];
        slot.retiring = Some(id);
        slot.retiring_layers = super::surface_layers::SurfaceLayers::EMPTY;
        slot.owner_sid = a.owner;
        slot.ghost = true;
        slot.placed = true;
        slot.win.set_frame(i32::from(a.dst_x), i32::from(a.dst_y), u32::from(a.w), u32::from(a.h));
        slot.win.visible = true;
        self.show_window(WindowId::App(idx as u16));
        let rect = self.app_window_rect(idx);
        self.queue_dirty_rect(rect);
        true
    }

    /// The ghost `owner_sid`'s new surface of `size` takes over, if it has one (`ghost_pick`):
    /// the slot, no longer a ghost — the create binds to it and the ghost's surface retires
    /// when the new one presents.
    pub(super) fn take_ghost_slot(&mut self, owner_sid: u64, size: (u16, u16)) -> Option<usize> {
        let mut ghosts: Vec<Ghost> = Vec::new();
        for (slot, a) in self.apps.iter().enumerate() {
            if a.ghost && a.retiring.is_some() && ghosts.try_reserve(1).is_ok() {
                let z = self.windows.z_of(WindowId::App(slot as u16));
                ghosts.push(Ghost { slot, owner: a.owner_sid, size: (a.win.w, a.win.h), z });
            }
        }
        let idx = ghost_pick::pick(&ghosts, owner_sid, size)?;
        self.apps[idx].ghost = false;
        let _ = debug_println(&alloc::format!("windowd: ghost taken over (slot={idx})"));
        Some(idx)
    }

    #[cfg(nexus_env = "os")]
    /// execd's `OP_APPS_ALIVE` — the apps it told the compositor is back: the ghosts of every
    /// other owner close (their apps are gone). execd's kernel id alone is heard.
    pub(crate) fn handle_apps_alive(&mut self, frame: &[u8], sender_sid: u64) {
        if sender_sid == 0 || sender_sid != nexus_abi::service_id_from_name(b"execd") {
            let _ = debug_println("windowd: apps alive denied (not execd)");
            return;
        }
        let mut owners = [0u64; APPS_ALIVE_MAX];
        let Some(n) = decode_apps_alive(frame, &mut owners) else {
            let _ = debug_println("windowd: FAIL apps alive (malformed)");
            return;
        };
        let closed = self.sweep_ghosts(&owners[..n]);
        let _ = debug_println(&alloc::format!("windowd: ghosts swept (closed={closed} alive={n})"));
    }

    #[cfg(nexus_env = "os")]
    /// Every ghost whose owner is not among `alive` closes; how many did.
    fn sweep_ghosts(&mut self, alive: &[u64]) -> usize {
        let mut closed = 0;
        for idx in 0..self.apps.len() {
            if self.apps[idx].ghost && ghost_pick::is_gone(self.apps[idx].owner_sid, alive) {
                self.apps[idx].ghost = false;
                self.close_app_window(idx, false);
                closed += 1;
            }
        }
        let desktop_ghost =
            self.desktop_surface_id.is_none() && self.buffers.desktop_retiring.is_some();
        if desktop_ghost && ghost_pick::is_gone(self.buffers.desktop_retiring_owner, alive) {
            self.end_retiring(Owner::Desktop);
            self.queue_full_frame_damage();
            closed += 1;
        }
        closed
    }
}

/// An entry before the decode fills it.
#[cfg(nexus_env = "os")]
const NONE: Adopted = Adopted {
    buffer: 0,
    owner: 0,
    width: 0,
    rows: 0,
    stride: 0,
    slots: 0,
    cache_block: 0,
    dst_x: 0,
    dst_y: 0,
    w: 0,
    h: 0,
    slot: 0,
    z: 0,
    window_key: 0,
};
