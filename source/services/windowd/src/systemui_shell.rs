// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Canonical SystemUI shell root for the retained scene graph.
//! All UI frontends (bootstrap shell, launcher, QS, app windows, DSL pages)
//! mount into this single root. No duplicate shell hierarchies.
//! Locked by TASK-0073 through TASK-0120.
//!
//! OWNERS: @ui
//! STATUS: Phase 8 — contract definition, TASK-0063 proof panel + sidebar
//! API_STABILITY: Locked for DSL/SystemUI migration

use crate::scene_graph::{InvalidationClass, RenderPrimitive, SceneGraph, SceneNode, SceneNodeId};
use animation::LayerId;
use nexus_layout_types::{BoxShadow, FxPx, Rect, Rgba8};

/// Animation layer identities. These are the *public* ids the animation
/// driver springs target; they are decoupled from `SceneNodeId`s on purpose —
/// the historical `SceneNodeId::from(LayerId)` punning silently animated the
/// root/wallpaper nodes. `SystemUiShell::animation_target` owns the mapping.
pub(crate) const HOVER_LAYER_ID: LayerId = LayerId(1);
pub(crate) const CLICK_LAYER_ID: LayerId = LayerId(2);
pub(crate) const KEYBOARD_LAYER_ID: LayerId = LayerId(3);
pub(crate) const SIDEBAR_LAYER_ID: LayerId = LayerId(62);

// ---------------------------------------------------------------------------
// Device profile — manifest-backed, no hardcoded defaults
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct DeviceProfile {
    pub display_width: u32,
    pub display_height: u32,
    pub dpi: u32,
    pub shell_mode: ShellMode,
    pub orientation: Orientation,
    pub size_class: SizeClass,
    pub hw_cursor_supported: bool,
    pub min_ring_slots: u8,
    pub refresh_interval_ns: u64,
}

impl DeviceProfile {
    pub(crate) const fn qemu_default() -> Self {
        Self {
            display_width: 1280,
            display_height: 800,
            dpi: 96,
            shell_mode: ShellMode::Desktop,
            orientation: Orientation::Landscape,
            size_class: SizeClass::Expanded,
            hw_cursor_supported: true,
            min_ring_slots: 2,
            refresh_interval_ns: 8_333_333, // 120 Hz
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShellMode {
    Desktop,
    Tablet,
    Phone,
    Automotive,
    Tv,
    Headless,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum Orientation {
    Landscape,
    Portrait,
    LandscapeFlipped,
    PortraitFlipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum SizeClass {
    Compact,
    Medium,
    Expanded,
}

// ── Glass constants (shared between sidebar, proof panel, button) ─────
const SIDEBAR_WIDTH: u32 = 320;
const SIDEBAR_MARGIN_TOP: u32 = 18;
const SIDEBAR_MARGIN_BOTTOM: u32 = 18;
const SIDEBAR_RADIUS: u32 = 24;
const GLASS_BUTTON_W: u32 = 156;
const GLASS_BUTTON_H: u32 = 56;
const GLASS_BUTTON_TOP: u32 = 24;
const GLASS_BUTTON_RIGHT: u32 = 24;
const GLASS_BUTTON_RADIUS: u32 = 18;
const GLASS_BLUR_RADIUS: u32 = 20;
const GLASS_SATURATION: u32 = 180;
const GLASS_TINT_ALPHA: u8 = 178;
const GLASS_EDGE_ALPHA: u8 = 26;
const PROOF_PANEL_X: i32 = 56;
const PROOF_PANEL_Y: i32 = 440;
const PROOF_PANEL_W: u32 = 610;
const PROOF_PANEL_H: u32 = 260;
const PROOF_PANEL_RADIUS: u32 = 16;
const CARD_W: u32 = 126;
const CARD_H: u32 = 82;
const CARD_GAP: u32 = 16;
const CARD_RADIUS: u32 = 12;
const CARD_ICON_SIZE: u32 = 24;

// ── Card slot indices ────────────────────────────────────────────────
const CARD_HOVER: usize = 0;
const CARD_CLICK: usize = 1;
const CARD_KEYBOARD: usize = 2;
const CARD_SCROLL: usize = 3;

// ---------------------------------------------------------------------------
// SystemUI shell — canonical shell root
// ---------------------------------------------------------------------------

#[allow(dead_code)]
pub(crate) struct SystemUiShell {
    pub graph: SceneGraph,
    pub profile: DeviceProfile,
    // Top-level
    pub root_id: SceneNodeId,
    pub wallpaper_id: SceneNodeId,
    pub panel_container_id: SceneNodeId,
    pub cursor_id: SceneNodeId,
    // Proof panel (target test panel)
    pub proof_panel_id: SceneNodeId,
    pub proof_backdrop_id: SceneNodeId,
    pub proof_bg_id: SceneNodeId,
    // Cards inside proof panel: [hover, click, keyboard, scroll]
    pub card_group_ids: [SceneNodeId; 4],
    pub card_bg_ids: [SceneNodeId; 4],
    pub card_icon_ids: [SceneNodeId; 4],
    // Glass button (hamburger menu)
    pub glass_button_id: SceneNodeId,
    pub button_backdrop_id: SceneNodeId,
    pub button_bg_id: SceneNodeId,
    pub button_bar_ids: [SceneNodeId; 3],
    // Sidebar
    pub sidebar_id: SceneNodeId,
    pub sidebar_backdrop_id: SceneNodeId,
    pub sidebar_bg_id: SceneNodeId,
    pub sidebar_close_ids: [SceneNodeId; 2],
    // Chat panel (future virtual list mount point)
    pub chat_panel_id: SceneNodeId,
    pub chat_backdrop_id: SceneNodeId,
}

impl SystemUiShell {
    pub(crate) fn new(profile: DeviceProfile) -> Self {
        let mut graph = SceneGraph::new();
        let w = profile.display_width;
        let h = profile.display_height;

        // ── Root ──────────────────────────────────────────────────
        let root_id = insert_node(
            &mut graph,
            None,
            0,
            0,
            None,
            Some(Rect {
                x: FxPx::new(0),
                y: FxPx::new(0),
                width: FxPx::new(w as i32),
                height: FxPx::new(h as i32),
            }),
        );
        // ── Wallpaper ─────────────────────────────────────────────
        let wallpaper_id = insert_node(
            &mut graph,
            Some(root_id),
            0,
            0,
            Some(RenderPrimitive::Surface {
                surface_handle: 0,
                src_x: 0,
                src_y: 0,
                width: w,
                height: h,
            }),
            None,
        );
        // ── Panel container ───────────────────────────────────────
        let panel_container_id = insert_node(&mut graph, Some(root_id), 0, 0, None, None);

        // ── Proof panel (target test panel) ───────────────────────
        let proof_panel_id = insert_node(
            &mut graph,
            Some(panel_container_id),
            PROOF_PANEL_X,
            PROOF_PANEL_Y,
            Some(RenderPrimitive::Group {
                shadow: Some(BoxShadow {
                    offset_x: FxPx::new(0),
                    offset_y: FxPx::new(4),
                    blur_radius: FxPx::new(30),
                    spread: FxPx::ZERO,
                    color: Rgba8::new(0, 0, 0, 80),
                }),
            }),
            Some(Rect {
                x: FxPx::new(PROOF_PANEL_X),
                y: FxPx::new(PROOF_PANEL_Y),
                width: FxPx::new(PROOF_PANEL_W as i32),
                height: FxPx::new(PROOF_PANEL_H as i32),
            }),
        );
        // Glass backdrop filter on the proof panel
        let proof_backdrop_id = insert_node(
            &mut graph,
            Some(proof_panel_id),
            PROOF_PANEL_X,
            PROOF_PANEL_Y,
            Some(RenderPrimitive::BackdropFilter {
                blur_radius: GLASS_BLUR_RADIUS,
                saturation_percent: GLASS_SATURATION,
            }),
            Some(Rect {
                x: FxPx::new(PROOF_PANEL_X),
                y: FxPx::new(PROOF_PANEL_Y),
                width: FxPx::new(PROOF_PANEL_W as i32),
                height: FxPx::new(PROOF_PANEL_H as i32),
            }),
        );
        // Glass tint background
        let proof_bg_id = insert_node(
            &mut graph,
            Some(proof_panel_id),
            PROOF_PANEL_X,
            PROOF_PANEL_Y,
            Some(RenderPrimitive::Rect {
                width: PROOF_PANEL_W,
                height: PROOF_PANEL_H,
                radius: PROOF_PANEL_RADIUS,
                color: Rgba8::new(28, 28, 30, GLASS_TINT_ALPHA),
            }),
            None,
        );
        // Glass border
        let _proof_border_id = insert_node(
            &mut graph,
            Some(proof_panel_id),
            PROOF_PANEL_X,
            PROOF_PANEL_Y,
            Some(RenderPrimitive::StrokeRect {
                width: PROOF_PANEL_W,
                height: PROOF_PANEL_H,
                radius: PROOF_PANEL_RADIUS,
                stroke_width: 1,
                color: Rgba8::new(255, 255, 255, GLASS_EDGE_ALPHA),
            }),
            None,
        );

        // ── Cards inside proof panel ──────────────────────────────
        let card_colors: [[u8; 4]; 4] = [
            [94, 92, 230, 200], // hover — accent purple
            [52, 199, 89, 200], // click — success green
            [255, 149, 0, 200], // keyboard — focus orange
            [255, 204, 0, 200], // scroll — warning yellow
        ];
        let mut card_group_ids = [SceneNodeId(0); 4];
        let mut card_bg_ids = [SceneNodeId(0); 4];
        let mut card_icon_ids = [SceneNodeId(0); 4];

        for i in 0..4usize {
            let cx = PROOF_PANEL_X + 24 + (CARD_W + CARD_GAP) as i32 * i as i32;
            let cy = PROOF_PANEL_Y + 48;
            let c = card_colors[i];
            let card = insert_node(
                &mut graph,
                Some(proof_panel_id),
                cx,
                cy,
                Some(RenderPrimitive::Group { shadow: None }),
                Some(Rect {
                    x: FxPx::new(cx),
                    y: FxPx::new(cy),
                    width: FxPx::new(CARD_W as i32),
                    height: FxPx::new(CARD_H as i32),
                }),
            );
            let bg = insert_node(
                &mut graph,
                Some(card),
                cx,
                cy,
                Some(RenderPrimitive::Rect {
                    width: CARD_W,
                    height: CARD_H,
                    radius: CARD_RADIUS,
                    color: Rgba8::new(c[0], c[1], c[2], c[3]),
                }),
                None,
            );
            let icon = insert_node(
                &mut graph,
                Some(card),
                cx + (CARD_W - CARD_ICON_SIZE) as i32 / 2,
                cy + (CARD_H - CARD_ICON_SIZE) as i32 / 2,
                Some(RenderPrimitive::Rect {
                    width: CARD_ICON_SIZE,
                    height: CARD_ICON_SIZE,
                    radius: 4,
                    color: Rgba8::new(255, 255, 255, 220),
                }),
                None,
            );
            card_group_ids[i] = card;
            card_bg_ids[i] = bg;
            card_icon_ids[i] = icon;
        }

        // ── Glass button (hamburger menu, top-right) ──────────────
        let btn_x = (w - GLASS_BUTTON_W - GLASS_BUTTON_RIGHT) as i32;
        let btn_y = GLASS_BUTTON_TOP as i32;
        let glass_button_id = insert_node(
            &mut graph,
            Some(root_id),
            btn_x,
            btn_y,
            Some(RenderPrimitive::Group { shadow: None }),
            Some(Rect {
                x: FxPx::new(btn_x),
                y: FxPx::new(btn_y),
                width: FxPx::new(GLASS_BUTTON_W as i32),
                height: FxPx::new(GLASS_BUTTON_H as i32),
            }),
        );
        let button_backdrop_id = insert_node(
            &mut graph,
            Some(glass_button_id),
            btn_x,
            btn_y,
            Some(RenderPrimitive::BackdropFilter {
                blur_radius: GLASS_BLUR_RADIUS,
                saturation_percent: GLASS_SATURATION,
            }),
            Some(Rect {
                x: FxPx::new(btn_x),
                y: FxPx::new(btn_y),
                width: FxPx::new(GLASS_BUTTON_W as i32),
                height: FxPx::new(GLASS_BUTTON_H as i32),
            }),
        );
        let button_bg_id = insert_node(
            &mut graph,
            Some(glass_button_id),
            btn_x,
            btn_y,
            Some(RenderPrimitive::Rect {
                width: GLASS_BUTTON_W,
                height: GLASS_BUTTON_H,
                radius: GLASS_BUTTON_RADIUS,
                color: Rgba8::new(28, 28, 30, 176),
            }),
            None,
        );
        // Hamburger bars (3 horizontal bars)
        let bar_w = 18u32;
        let bar_h = 3u32;
        let bar_gap = 5u32;
        let bar_total = 3 * bar_h + 2 * bar_gap;
        let bar_base_x = btn_x + (GLASS_BUTTON_W - bar_w) as i32 / 2;
        let bar_base_y = btn_y + (GLASS_BUTTON_H - bar_total) as i32 / 2;
        let mut button_bar_ids = [SceneNodeId(0); 3];
        for b in 0..3 {
            let by = bar_base_y + (bar_h + bar_gap) as i32 * b as i32;
            button_bar_ids[b] = insert_node(
                &mut graph,
                Some(glass_button_id),
                bar_base_x,
                by,
                Some(RenderPrimitive::Rect {
                    width: bar_w,
                    height: bar_h,
                    radius: 1,
                    color: Rgba8::new(255, 255, 255, 220),
                }),
                None,
            );
        }

        // ── Sidebar (glass panel, right edge) ─────────────────────
        let sidebar_x = (w - SIDEBAR_WIDTH) as i32;
        let sidebar_y = SIDEBAR_MARGIN_TOP as i32;
        let sidebar_h = h - SIDEBAR_MARGIN_TOP - SIDEBAR_MARGIN_BOTTOM;
        let sidebar_id = insert_node(
            &mut graph,
            Some(root_id),
            sidebar_x,
            sidebar_y,
            Some(RenderPrimitive::Group {
                shadow: Some(BoxShadow {
                    offset_x: FxPx::new(-4),
                    offset_y: FxPx::new(0),
                    blur_radius: FxPx::new(30),
                    spread: FxPx::ZERO,
                    color: Rgba8::new(0, 0, 0, 100),
                }),
            }),
            Some(Rect {
                x: FxPx::new(sidebar_x),
                y: FxPx::new(sidebar_y),
                width: FxPx::new(SIDEBAR_WIDTH as i32),
                height: FxPx::new(sidebar_h as i32),
            }),
        );
        let sidebar_backdrop_id = insert_node(
            &mut graph,
            Some(sidebar_id),
            sidebar_x,
            sidebar_y,
            Some(RenderPrimitive::BackdropFilter {
                blur_radius: GLASS_BLUR_RADIUS,
                saturation_percent: GLASS_SATURATION,
            }),
            Some(Rect {
                x: FxPx::new(sidebar_x),
                y: FxPx::new(sidebar_y),
                width: FxPx::new(SIDEBAR_WIDTH as i32),
                height: FxPx::new(sidebar_h as i32),
            }),
        );
        let sidebar_bg_id = insert_node(
            &mut graph,
            Some(sidebar_id),
            sidebar_x,
            sidebar_y,
            Some(RenderPrimitive::Rect {
                width: SIDEBAR_WIDTH,
                height: sidebar_h,
                radius: SIDEBAR_RADIUS,
                color: Rgba8::new(28, 28, 30, 220),
            }),
            None,
        );
        // Close icon (X) at top-right of sidebar
        let close_size = 16u32;
        let close_inset = 16i32;
        let close_x = sidebar_x + SIDEBAR_WIDTH as i32 - close_size as i32 - close_inset;
        let close_y = sidebar_y + close_inset;
        let mut sidebar_close_ids = [SceneNodeId(0); 2];
        sidebar_close_ids[0] = insert_node(
            &mut graph,
            Some(sidebar_id),
            close_x,
            close_y + (close_size / 2 - 2) as i32,
            Some(RenderPrimitive::Rect {
                width: close_size,
                height: 3,
                radius: 1,
                color: Rgba8::new(255, 255, 255, 200),
            }),
            None,
        );
        sidebar_close_ids[1] = insert_node(
            &mut graph,
            Some(sidebar_id),
            close_x + (close_size / 2 - 2) as i32,
            close_y,
            Some(RenderPrimitive::Rect {
                width: 3,
                height: close_size,
                radius: 1,
                color: Rgba8::new(255, 255, 255, 200),
            }),
            None,
        );

        // ── Chat panel (dual-panel blur) ──────────────────────────
        let chat_x: i32 = 320;
        let chat_y: i32 = 80;
        let chat_w = 640u32;
        let chat_h = 640u32;
        let chat_panel_id = insert_node(
            &mut graph,
            Some(panel_container_id),
            chat_x,
            chat_y,
            Some(RenderPrimitive::Group {
                shadow: Some(BoxShadow {
                    offset_x: FxPx::new(0),
                    offset_y: FxPx::new(4),
                    blur_radius: FxPx::new(20),
                    spread: FxPx::ZERO,
                    color: Rgba8::new(0, 0, 0, 80),
                }),
            }),
            Some(Rect {
                x: FxPx::new(chat_x),
                y: FxPx::new(chat_y),
                width: FxPx::new(chat_w as i32),
                height: FxPx::new(chat_h as i32),
            }),
        );
        let chat_backdrop_id = insert_node(
            &mut graph,
            Some(chat_panel_id),
            chat_x,
            chat_y,
            Some(RenderPrimitive::BackdropFilter {
                blur_radius: GLASS_BLUR_RADIUS,
                saturation_percent: GLASS_SATURATION,
            }),
            Some(Rect {
                x: FxPx::new(chat_x),
                y: FxPx::new(chat_y),
                width: FxPx::new(chat_w as i32),
                height: FxPx::new(chat_h as i32),
            }),
        );

        // ── Cursor ────────────────────────────────────────────────
        let cursor_id = insert_node(
            &mut graph,
            Some(root_id),
            100,
            100,
            Some(RenderPrimitive::Cursor {
                hotspot_x: 0,
                hotspot_y: 0,
            }),
            None,
        );

        Self {
            graph,
            profile,
            root_id,
            wallpaper_id,
            panel_container_id,
            cursor_id,
            proof_panel_id,
            proof_backdrop_id,
            proof_bg_id,
            card_group_ids,
            card_bg_ids,
            card_icon_ids,
            glass_button_id,
            button_backdrop_id,
            button_bg_id,
            button_bar_ids,
            sidebar_id,
            sidebar_backdrop_id,
            sidebar_bg_id,
            sidebar_close_ids,
            chat_panel_id,
            chat_backdrop_id,
        }
    }

    // ── Card state helpers ────────────────────────────────────────────

    pub(crate) fn set_card_active(&mut self, slot: usize, active: bool) {
        if slot >= 4 {
            return;
        }
        let color = if active {
            Rgba8::new(255, 255, 255, 255)
        } else {
            Rgba8::new(255, 255, 255, 180)
        };
        self.graph.set_primitive(
            self.card_bg_ids[slot],
            RenderPrimitive::Rect {
                width: CARD_W,
                height: CARD_H,
                radius: CARD_RADIUS,
                color,
            },
        );
    }

    pub(crate) fn set_sidebar_visible(&mut self, visible: bool) {
        if let Some(node) = self.graph.find_mut(self.sidebar_id) {
            node.visible = visible;
        }
        if let Some(node) = self.graph.find_mut(self.sidebar_backdrop_id) {
            node.visible = visible;
        }
        if let Some(node) = self.graph.find_mut(self.sidebar_bg_id) {
            node.visible = visible;
        }
        for &id in &self.sidebar_close_ids {
            if let Some(node) = self.graph.find_mut(id) {
                node.visible = visible;
            }
        }
    }

    pub(crate) fn set_sidebar_slide(&mut self, translate_x: f32) {
        let base_x = (self.profile.display_width - SIDEBAR_WIDTH) as i32;
        let tx = (base_x as f32 + translate_x) as i32;
        self.graph
            .set_position(self.sidebar_id, tx, SIDEBAR_MARGIN_TOP as i32);
    }

    /// Resolve an animation layer id to the scene node it animates.
    ///
    /// This is the single owner of the layer→node mapping: hover/click/
    /// keyboard springs animate the corresponding card backgrounds, the
    /// sidebar spring animates the sidebar group. Unknown layers resolve to
    /// `None` (the update is dropped instead of hitting an unrelated node).
    pub(crate) fn animation_target(&self, layer: LayerId) -> Option<SceneNodeId> {
        if layer == HOVER_LAYER_ID {
            Some(self.card_bg_ids[0])
        } else if layer == CLICK_LAYER_ID {
            Some(self.card_bg_ids[1])
        } else if layer == KEYBOARD_LAYER_ID {
            Some(self.card_bg_ids[2])
        } else if layer == SIDEBAR_LAYER_ID {
            Some(self.sidebar_id)
        } else {
            None
        }
    }

    pub(crate) fn update_cursor(&mut self, x: i32, y: i32) {
        self.graph.set_position(self.cursor_id, x, y);
    }

    // ── Mount helpers ────────────────────────────────────────────────

    pub(crate) fn mount_proof_panel(&mut self) -> SceneNodeId {
        self.proof_panel_id
    }
}

// ── Helper: insert a node with common defaults ──────────────────────
fn insert_node(
    graph: &mut SceneGraph,
    parent: Option<SceneNodeId>,
    x: i32,
    y: i32,
    primitive: Option<RenderPrimitive>,
    clip: Option<Rect>,
) -> SceneNodeId {
    let id = graph.next_id();
    let mut node = SceneNode::new(id);
    node.parent = parent;
    node.x = x;
    node.y = y;
    node.clip = clip;
    node.primitive = primitive;
    graph.insert(node);
    id
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_creates_all_nodes() {
        let shell = SystemUiShell::new(DeviceProfile::qemu_default());
        let n = shell.graph.node_count();
        // root + wallpaper + panel_container + proof_panel + proof_backdrop +
        // proof_bg + proof_border + 12 card nodes + glass_button + button_backdrop +
        // button_bg + 3 bars + sidebar + sidebar_backdrop + sidebar_bg +
        // 2 close icons + chat_panel + chat_backdrop + cursor
        // = 1 + 1 + 1 + 1 + 1 + 1 + 1 + 12 + 1 + 1 + 1 + 3 + 1 + 1 + 1 + 2 + 1 + 1 + 1
        // = 31
        assert!(n >= 28, "shell should have at least 28 nodes, got {}", n);
    }

    #[test]
    fn device_profile_constants_match_display() {
        let p = DeviceProfile::qemu_default();
        assert_eq!(p.display_width, 1280);
        assert_eq!(p.display_height, 800);
    }

    #[test]
    fn cursor_update_changes_position() {
        let mut shell = SystemUiShell::new(DeviceProfile::qemu_default());
        shell.graph.mark_all_clean();
        shell.update_cursor(100, 200);
        let node = shell.graph.find(shell.cursor_id).unwrap();
        assert_eq!(node.x, 100);
        assert_eq!(node.y, 200);
    }

    #[test]
    fn sidebar_visibility_toggles() {
        let mut shell = SystemUiShell::new(DeviceProfile::qemu_default());
        shell.set_sidebar_visible(false);
        assert!(!shell.graph.find(shell.sidebar_id).unwrap().visible);
        shell.set_sidebar_visible(true);
        assert!(shell.graph.find(shell.sidebar_id).unwrap().visible);
    }

    #[test]
    fn sidebar_slide_updates_position() {
        let mut shell = SystemUiShell::new(DeviceProfile::qemu_default());
        shell.graph.mark_all_clean();
        shell.set_sidebar_slide(50.0);
        let node = shell.graph.find(shell.sidebar_id).unwrap();
        // base_x = 1280 - 320 = 960, +50 translate = 1010
        assert!(node.x > 960);
    }

    #[test]
    fn animation_targets_hit_the_intended_nodes() {
        let shell = SystemUiShell::new(DeviceProfile::qemu_default());
        // The explicit mapping — never the id-punned root/wallpaper nodes.
        assert_eq!(
            shell.animation_target(HOVER_LAYER_ID),
            Some(shell.card_bg_ids[0])
        );
        assert_eq!(
            shell.animation_target(CLICK_LAYER_ID),
            Some(shell.card_bg_ids[1])
        );
        assert_eq!(
            shell.animation_target(KEYBOARD_LAYER_ID),
            Some(shell.card_bg_ids[2])
        );
        assert_eq!(
            shell.animation_target(SIDEBAR_LAYER_ID),
            Some(shell.sidebar_id)
        );
        // Regression guard for the historical punning bug: the hover target
        // must not be the root node (LayerId(1) == SceneNodeId(1) == root).
        assert_ne!(shell.animation_target(HOVER_LAYER_ID), Some(shell.root_id));
        // Unknown layers are dropped, not mistargeted.
        assert_eq!(shell.animation_target(LayerId(999)), None);
    }

    #[test]
    fn card_active_changes_primitive() {
        let mut shell = SystemUiShell::new(DeviceProfile::qemu_default());
        shell.graph.mark_all_clean();
        shell.set_card_active(0, true);
        let node = shell.graph.find(shell.card_bg_ids[0]).unwrap();
        assert!(node.invalidation != InvalidationClass::Clean);
    }
}
