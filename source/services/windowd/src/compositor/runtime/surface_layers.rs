// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: windowd compositor runtime — a client surface's declared layers (RFC-0109) taken
//! in: the STRUCTURE (`OP_SURFACE_TREE`, kept for the surface's next present when it fits the
//! surface) and the MOTION (`OP_SURFACE_MOTION`, in effect at once), each from the surface's own
//! client only; and a present's damage — its named rows for the copy, onto the screen where they
//! show. The state and the geometry are `crate::layer_geometry` (host-tested); the encode is
//! `surface_layers_encode`. windowd runs no scroll physics and decides no hover (ADR-0076).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the geometry in `crate::layer_geometry`; the handlers on the QEMU lanes.

use super::*;
use crate::layer_geometry::{owns, store_damage};
pub(crate) use crate::layer_geometry::{DamageRows, SurfaceLayers, TreeChange};
use nexus_display_proto::surface_layers::{self as proto, SurfaceMotion, SurfaceTree};

/// Which surface a frame names.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Owner {
    Desktop,
    App(usize),
}

impl DisplayServerRuntime {
    /// The surface a frame names, when its sender owns it — identity is the kernel's sender id
    /// captured at the surface's create, never the payload's surface id alone.
    fn layers_owner(&self, surface_id: u32, sender_sid: u64) -> Option<Owner> {
        let (owner, owner_sid) = if self.desktop_surface_id == Some(surface_id) {
            (Owner::Desktop, self.desktop_owner_sid)
        } else {
            let idx = self.app_index_by_surface(surface_id)?;
            (Owner::App(idx), self.apps[idx].owner_sid)
        };
        owns(owner_sid, sender_sid).then_some(owner)
    }

    fn surface_layers_mut(&mut self, owner: Owner) -> &mut SurfaceLayers {
        match owner {
            Owner::Desktop => &mut self.desktop_layers,
            Owner::App(idx) => &mut self.apps[idx].layers,
        }
    }

    /// `OP_SURFACE_TREE`: the surface's structure, kept for its next present when it fits the
    /// surface (RFC-0109); refused whole otherwise — the last valid structure stays.
    pub(crate) fn handle_surface_tree(&mut self, frame: &[u8], sender_sid: u64) {
        let mut tree = SurfaceTree::EMPTY;
        let Some(surface_id) = proto::decode_surface_tree(frame, &mut tree) else { return };
        let Some(owner) = self.layers_owner(surface_id, sender_sid) else { return };
        let Some(s) = self.client_surfaces.get_by_id(surface_id).copied() else { return };
        let checked = proto::validate_tree(&tree, s.width, s.height, s.rows, s.store_w);
        let layers = self.surface_layers_mut(owner);
        match checked {
            Ok(()) => layers.stage(tree),
            Err(why) => {
                crate::compositor::frame_causes::layers_refused();
                if layers.first_refusal() {
                    let _ = debug_println(&alloc::format!(
                        "windowd: surface tree refused (id={surface_id} reason={why:?})"
                    ));
                }
            }
        }
    }

    /// `OP_SURFACE_MOTION`: the surface's offsets and washes, in effect at once — the next frame
    /// composes them (owed, naming no damage: the planner redraws what moved).
    pub(crate) fn handle_surface_motion(&mut self, frame: &[u8], sender_sid: u64) {
        let mut motion = SurfaceMotion::default();
        let Some(surface_id) = proto::decode_surface_motion(frame, &mut motion) else { return };
        let Some(owner) = self.layers_owner(surface_id, sender_sid) else { return };
        let layers = self.surface_layers_mut(owner);
        // A motion may name a structure still waiting for its present.
        match proto::validate_motion(&motion, &layers.latest_tree()) {
            Ok(()) => {
                layers.motion = motion;
                self.owe_frame();
            }
            Err(why) => {
                crate::compositor::frame_causes::layers_refused();
                if layers.first_refusal() {
                    let _ = debug_println(&alloc::format!(
                        "windowd: surface motion refused (id={surface_id} reason={why:?})"
                    ));
                }
            }
        }
    }

    /// A present's damage (RFC-0109): its named rows into `damage` (what the next frame copies)
    /// and onto the screen — a base row where the surface shows it, a band's rows in its scroll
    /// layer's viewport, the whole surface for a new structure. `idx`: the window's slot (`None`:
    /// the desktop, whose changes also stale every window's blur above them); `screen`: where the
    /// surface's origin and extent are on the display.
    pub(crate) fn present_damage(
        &mut self,
        surface: &crate::client_surface::ClientSurface,
        rects: &[nexus_display_proto::client_surface::DamageRect],
        change: TreeChange,
        idx: Option<usize>,
        screen: (u32, u32, u32, u32),
        damage: &mut DamageRows,
    ) {
        let layers = idx.map_or(self.desktop_layers, |i| self.apps[i].layers);
        let base_h = u32::from(surface.height);
        let queue = |rt: &mut Self, r: (u32, u32, u32, u32)| {
            // Cut at the surface's extent on the screen (a frame smaller than the surface).
            let x1 = (screen.0 + r.0 + r.2).min(screen.0 + screen.2);
            let y1 = (screen.1 + r.1 + r.3).min(screen.1 + screen.3);
            let (x0, y0) = (screen.0 + r.0, screen.1 + r.1);
            if x1 > x0 && y1 > y0 {
                let dr = DamageRect { x: x0, y: y0, width: x1 - x0, height: y1 - y0 };
                rt.queue_dirty_rect(dr);
                if idx.is_none() {
                    rt.invalidate_blur_over(dr, None);
                }
            }
        };
        let whole = (0, 0, screen.2, screen.3);
        if rects.is_empty() || change == TreeChange::Structure {
            if rects.is_empty() {
                *damage = DamageRows::FULL;
            }
            queue(self, whole);
        }
        if let TreeChange::Bands(mask) = change {
            for (k, s) in layers.tree.scrolls().iter().enumerate() {
                if mask & (1 << k) != 0 {
                    queue(
                        self,
                        (u32::from(s.vx), u32::from(s.vy), u32::from(s.vw), u32::from(s.vh)),
                    );
                }
            }
        }
        for r in rects {
            let (x, y, w, h) =
                (u32::from(r.x), u32::from(r.y), u32::from(r.width), u32::from(r.height));
            damage.add(y, y + h);
            if y < base_h {
                queue(self, (x, y, w, (y + h).min(base_h) - y));
            }
            if y + h > base_h {
                let top = y.max(base_h);
                let mut views = [(0, 0, 0, 0); proto::MAX_SCROLL_LAYERS];
                let n = store_damage(&layers, (x, top, w, y + h - top), &mut views);
                for v in &views[..n] {
                    queue(self, *v);
                }
            }
        }
        if idx.is_none() {
            self.desktop_dirty = true;
        }
    }
}
