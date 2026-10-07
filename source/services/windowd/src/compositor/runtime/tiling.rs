// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: windowd compositor runtime — window TILING (TASK-0066, the desktop default model):
//! applies `zones` to app windows through the ONE geometry path (`apply_window_frame`),
//! remembers the pre-tile frame for Return, handles the drag release at edges/corners, the
//! verb `CONTROL_WIN_ZONE` (menu + keyboard chords land here), arrangements over the z-order,
//! and the reflow on a work-area change. Deny = a named reason, every marker registered.
//! windowd draws nothing for it — the menu is the kit's, the glyphs are the kit's.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `zones` inline + `tests/tile_zones.rs` (geometry); lanes for the wiring

use super::*;
use crate::window_scene::WindowId;
use crate::zones::{self, Arrangement, TileDeny, Zone, ZoneRequest};

impl DisplayServerRuntime {
    /// The tiling area of slot `idx`: `(top, work_h)` — a tiled window is still a FLOATING
    /// window, so it starts below the status bar and ends above the taskbar.
    fn tile_area(&self, idx: usize) -> (i32, u32) {
        (self.full_surface_frame(idx).0, self.work_area_h())
    }

    /// Why slot `idx` cannot be tiled right now, if it cannot.
    fn tile_deny(&self, idx: usize) -> Option<TileDeny> {
        if self.apps[idx].surface_id.is_none() {
            return Some(TileDeny::NoWindow);
        }
        if !self.apps[idx].intent_resizable {
            return Some(TileDeny::NotResizable);
        }
        None
    }

    fn say_deny(&self, reason: TileDeny) {
        let _ = debug_println(&crate::markers::wm_tile_deny_marker(reason.as_str()));
    }

    /// Tiles slot `idx` into `zone`: the first tile remembers the floating frame (Return's
    /// target), the frame goes through `apply_window_frame`, the feed learns the zone.
    pub(super) fn apply_zone(&mut self, idx: usize, zone: Zone) -> bool {
        if let Some(reason) = self.tile_deny(idx) {
            self.say_deny(reason);
            return false;
        }
        let id = WindowId::App(idx as u8);
        if self.windows.is_fullscreen(id) {
            // Leave fullscreen first: a tile is a floating frame.
            self.toggle_fullscreen(id);
        }
        if self.apps[idx].pre_tile.is_none() {
            let f = self.apps[idx].win.frame();
            self.apps[idx].pre_tile = Some((f.x, f.y, f.w, f.h));
        }
        let (top, work_h) = self.tile_area(idx);
        let (x, y, w, h) = zones::zone_frame(zone, self.mode.width, work_h, top, self.tile_margin);
        self.apply_window_frame(id, x, y, w, h);
        self.push_app_content_rect(idx);
        self.apps[idx].zone = zone;
        let _ = debug_println(&crate::markers::wm_tile_marker(zone.name(), Self::window_name(id)));
        self.push_window_set();
        true
    }

    /// Back to the frame the window had before its first tile; `pointer` = the pointer x and
    /// the title grab offset when a drag pulls the window off its tile (the frame then stays
    /// under the pointer), `None` for the menu/chord Return.
    pub(super) fn return_window(&mut self, idx: usize, pointer: Option<(i32, i32)>) -> bool {
        let Some(pre) = self.apps[idx].pre_tile.take() else {
            return false;
        };
        let id = WindowId::App(idx as u8);
        let tiled_w = self.apps[idx].win.w;
        let (x, y, w, h) = match pointer {
            Some((cx, grab_dx)) => {
                zones::return_frame_under_pointer(pre, cx, grab_dx, tiled_w, self.mode.width)
            }
            None => pre,
        };
        self.apps[idx].zone = Zone::None;
        self.apply_window_frame(id, x, y, w, h);
        self.push_app_content_rect(idx);
        let _ = debug_println(&crate::markers::wm_return_marker(Self::window_name(id)));
        if !self.tile_proof_said {
            // A tile and its Return: the frame bookkeeping proved itself once.
            self.tile_proof_said = true;
            let _ = debug_println(crate::markers::SELFTEST_UI_V7_TILE_OK_MARKER);
        }
        self.push_window_set();
        true
    }

    /// While a title-bar drag moves, tell the SHELL which zone a release would take — the
    /// desktop surface draws the preview (`OP_SURFACE_TILE_PREVIEW` → `device.tilePreview`);
    /// windowd draws nothing. One push per change; `None` clears it.
    pub(super) fn push_tile_preview(&mut self, idx: usize, pointer: Option<(i32, i32)>) {
        let code = match pointer {
            Some((cx, cy)) if self.tile_edges => {
                let (_, work_h) = self.tile_area(idx);
                zones::release_zone_at(cx, cy, self.mode.width, work_h).map_or(0, Zone::code)
            }
            _ => 0,
        };
        if code == self.tile_preview_sent {
            return;
        }
        self.tile_preview_sent = code;
        let frame = nexus_display_proto::surface_windows::encode_surface_tile_preview(code);
        self.send_desktop_frame(&frame);
    }

    /// A title-bar drag RELEASED with the pointer at `(cx, cy)`: an edge/corner tiles the
    /// window (`ui.tile.edges`); returns true when the release was consumed.
    pub(super) fn apply_release_tile(&mut self, idx: usize, cx: i32, cy: i32) -> bool {
        self.push_tile_preview(idx, None);
        let (_, work_h) = self.tile_area(idx);
        let Some(zone) = zones::release_zone_at(cx, cy, self.mode.width, work_h) else {
            // Rare and user-driven: where releases land (the corner band's witness).
            let _ = debug_println(&alloc::format!("windowd: wm release at ({cx},{cy})"));
            return false;
        };
        if !self.tile_edges {
            self.say_deny(TileDeny::EdgesOff);
            return false;
        }
        self.apply_zone(idx, zone)
    }

    /// A title-bar drag BEGINS on a tiled window: it comes off its tile at its pre-tile size,
    /// under the pointer (`grab_dx` = pointer x minus the tiled frame's x).
    pub(super) fn untile_for_drag(&mut self, idx: usize, cx: i32) {
        if self.apps[idx].zone != Zone::None {
            let grab_dx = cx - self.apps[idx].win.x;
            self.return_window(idx, Some((cx, grab_dx)));
        }
    }

    /// The verb's low 4 bits for slot `idx` (menu tiles and keyboard chords both end here).
    pub(super) fn apply_zone_code(&mut self, idx: usize, code: u8) {
        let Some(request) = zones::request_from_code(code) else {
            let _ = debug_println("WINDOWD: control win zone (unknown code)");
            return;
        };
        match request {
            ZoneRequest::Tile(zone) => {
                self.apply_zone(idx, zone);
            }
            ZoneRequest::Return => {
                if !self.return_window(idx, None) {
                    // Nothing to return to: the window never tiled — the whole work area it is.
                    self.apply_zone(idx, Zone::None);
                }
            }
            ZoneRequest::Arrange(kind) => self.arrange(idx, kind),
        }
    }

    /// Tiles the requesting window and the next on-screen app windows in z-order into the
    /// arrangement's zones (the requester takes the first zone).
    fn arrange(&mut self, idx: usize, kind: Arrangement) {
        let (hit, hit_n) = self.windows.hit_order(USE_DESKTOP_SHELL);
        let mut order: [usize; crate::window_scene::MAX_APP_WINDOWS] =
            [usize::MAX; crate::window_scene::MAX_APP_WINDOWS];
        let mut n = 0usize;
        order[n] = idx;
        n += 1;
        for &wid in &hit[..hit_n] {
            let WindowId::App(i) = wid else { continue };
            let i = i as usize;
            if i == idx || n >= order.len() || self.apps[i].surface_id.is_none() {
                continue;
            }
            if self.windows.is_minimized(wid) {
                continue;
            }
            order[n] = i;
            n += 1;
        }
        let mut tiled = 0u8;
        for (slot, zone) in order[..n].iter().zip(kind.zones().iter()) {
            if self.apply_zone(*slot, *zone) {
                tiled += 1;
            }
        }
        let _ = debug_println(&crate::markers::wm_arrange_marker(kind.name(), tiled));
    }

    /// A keyboard chord (inputd's one-shot fact): the FOCUSED app window takes the zone —
    /// `ui.tile.chords` gates it, the desktop never tiles.
    pub(super) fn apply_chord(&mut self, code: u8) {
        if !self.tile_chords {
            self.say_deny(TileDeny::ChordsOff);
            return;
        }
        // The focused APP window, else the topmost on-screen one (a surface
        // re-create after a tile can park focus on the desktop for a frame).
        let app_of = |id: Option<WindowId>| match id {
            Some(WindowId::App(i)) => Some(usize::from(i)),
            _ => None,
        };
        let Some(idx) = app_of(self.windows.focused()).or_else(|| app_of(self.windows.top()))
        else {
            self.say_deny(TileDeny::NoFocus);
            return;
        };
        let i = idx as u8;
        if self.apps[idx].surface_id.is_none() {
            self.say_deny(TileDeny::NoFocus);
            return;
        }
        let zone = zones::request_from_code(code).map_or("unknown", |r| match r {
            ZoneRequest::Tile(z) => z.name(),
            ZoneRequest::Return => "return",
            ZoneRequest::Arrange(a) => a.name(),
        });
        let _ = debug_println(&crate::markers::wm_chord_marker(
            zone,
            Self::window_name(WindowId::App(i)),
        ));
        self.apply_zone_code(idx, code);
    }

    /// The work area changed (shell mode, display mode): every tiled window takes its zone's
    /// new frame — the ONE geometry path again.
    pub(super) fn reflow_tiled(&mut self) {
        for idx in 0..self.apps.len() {
            let zone = self.apps[idx].zone;
            if zone == Zone::None || self.apps[idx].surface_id.is_none() {
                continue;
            }
            let (top, work_h) = self.tile_area(idx);
            let (x, y, w, h) =
                zones::zone_frame(zone, self.mode.width, work_h, top, self.tile_margin);
            self.apply_window_frame(WindowId::App(idx as u8), x, y, w, h);
            self.push_app_content_rect(idx);
        }
    }
}
