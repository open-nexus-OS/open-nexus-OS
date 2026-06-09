// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Canonical SystemUI shell root for the retained scene graph.
//! All UI frontends (bootstrap shell, launcher, QS, app windows, DSL pages)
//! mount into this single root. No duplicate shell hierarchies.
//! Locked by TASK-0073 through TASK-0120.
//!
//! OWNERS: @ui
//! STATUS: Phase 8 — contract definition
//! API_STABILITY: Locked for DSL/SystemUI migration

use crate::scene_graph::{InvalidationClass, RenderPrimitive, SceneGraph, SceneNode, SceneNodeId};
use nexus_layout_types::Rect;

// ---------------------------------------------------------------------------
// Device profile — manifest-backed, no hardcoded defaults
// ---------------------------------------------------------------------------

/// Device display configuration backed by a device manifest or policy recipe.
/// No desktop-only defaults — every field comes from the manifest.
#[derive(Debug, Clone)]
pub(crate) struct DeviceProfile {
    /// Display width in pixels.
    pub display_width: u32,
    /// Display height in pixels.
    pub display_height: u32,
    /// Physical DPI of the display.
    pub dpi: u32,
    /// Device form factor / shell mode.
    pub shell_mode: ShellMode,
    /// Primary orientation hint.
    pub orientation: Orientation,
    /// Size class (compact, medium, expanded).
    pub size_class: SizeClass,
    /// Whether hardware cursor is supported.
    pub hw_cursor_supported: bool,
    /// Minimum frame ring slots for triple buffering.
    pub min_ring_slots: u8,
    /// Target refresh interval in nanoseconds.
    pub refresh_interval_ns: u64,
}

impl DeviceProfile {
    /// Default profile for QEMU 1280×800 development.
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

/// Device form factor / shell mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShellMode {
    /// Desktop/laptop shell (window management, taskbar).
    Desktop,
    /// Tablet shell (full-screen, gesture navigation).
    Tablet,
    /// Phone shell (single-app, navigation bar).
    Phone,
    /// Automotive shell (dashboard, split-screen).
    Automotive,
    /// TV shell (10-foot UI, D-pad navigation).
    Tv,
    /// Headless / embedded (no display).
    Headless,
}

/// Primary display orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Orientation {
    Landscape,
    Portrait,
    LandscapeFlipped,
    PortraitFlipped,
}

/// Adaptive size class for responsive layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SizeClass {
    /// Compact: phone portrait, small windows.
    Compact,
    /// Medium: tablet portrait, split-screen.
    Medium,
    /// Expanded: desktop, tablet landscape.
    Expanded,
}

// ---------------------------------------------------------------------------
// SystemUI shell — canonical shell root
// ---------------------------------------------------------------------------

/// The canonical SystemUI shell root.
///
/// Owns one retained scene graph. All UI frontends mount into this graph.
/// The bootstrap shell, launcher, quick settings, app windows, and future
/// DSL pages all share this single root — no duplicate hierarchies.
pub(crate) struct SystemUiShell {
    /// The retained scene graph.
    pub graph: SceneGraph,
    /// Device profile backing this shell instance.
    pub profile: DeviceProfile,
    /// Scene node id of the shell root (top-level container).
    pub root_id: SceneNodeId,
    /// Scene node id of the wallpaper layer.
    pub wallpaper_id: SceneNodeId,
    /// Scene node id of the system UI panel container.
    pub panel_container_id: SceneNodeId,
    /// Scene node id of the cursor overlay.
    pub cursor_id: SceneNodeId,
}

impl SystemUiShell {
    /// Create a new SystemUI shell with the given device profile.
    pub(crate) fn new(profile: DeviceProfile) -> Self {
        let mut graph = SceneGraph::new();

        // Root: invisible container, anchors the entire scene.
        let root_id = graph.next_id();
        let root = SceneNode {
            id: root_id,
            parent: None,
            children: alloc::vec::Vec::new(),
            x: 0,
            y: 0,
            visible: true,
            opacity: 1.0,
            clip: Some(Rect {
                x: nexus_layout_types::FxPx::new(0),
                y: nexus_layout_types::FxPx::new(0),
                width: nexus_layout_types::FxPx::new(profile.display_width as i32),
                height: nexus_layout_types::FxPx::new(profile.display_height as i32),
            }),
            primitive: None,
            subtree_hash: 0,
            invalidation: InvalidationClass::MeasureAndPlace,
        };
        graph.insert(root);

        // Wallpaper: full-screen backdrop, rendered as a surface blit.
        let wallpaper_id = graph.next_id();
        let wallpaper = SceneNode {
            id: wallpaper_id,
            parent: Some(root_id),
            children: alloc::vec::Vec::new(),
            x: 0,
            y: 0,
            visible: true,
            opacity: 1.0,
            clip: None,
            primitive: Some(RenderPrimitive::Surface {
                surface_handle: 0, // wallpaper surface pool slot
                src_x: 0,
                src_y: 0,
                width: profile.display_width,
                height: profile.display_height,
            }),
            subtree_hash: 0,
            invalidation: InvalidationClass::MeasureAndPlace,
        };
        graph.insert(wallpaper);

        // Panel container: hosts system UI panels (proof panel, launcher, QS).
        let panel_container_id = graph.next_id();
        let panel_container = SceneNode {
            id: panel_container_id,
            parent: Some(root_id),
            children: alloc::vec::Vec::new(),
            x: 0,
            y: 0,
            visible: true,
            opacity: 1.0,
            clip: None,
            primitive: None,
            subtree_hash: 0,
            invalidation: InvalidationClass::MeasureAndPlace,
        };
        graph.insert(panel_container);

        // Cursor: hardware cursor overlay (position updated by inputd).
        let cursor_id = graph.next_id();
        let cursor = SceneNode {
            id: cursor_id,
            parent: Some(root_id),
            children: alloc::vec::Vec::new(),
            x: 0,
            y: 0,
            visible: true,
            opacity: 1.0,
            clip: None,
            primitive: Some(RenderPrimitive::Cursor {
                hotspot_x: 0,
                hotspot_y: 0,
            }),
            subtree_hash: 0,
            invalidation: InvalidationClass::MeasureAndPlace,
        };
        graph.insert(cursor);

        Self { graph, profile, root_id, wallpaper_id, panel_container_id, cursor_id }
    }

    /// Mount a proof panel into the panel container.
    /// Returns the scene node id of the mounted panel.
    pub(crate) fn mount_proof_panel(&mut self) -> SceneNodeId {
        let panel = self.graph.next_id();
        let node = SceneNode {
            id: panel,
            parent: Some(self.panel_container_id),
            children: alloc::vec::Vec::new(),
            x: 0,
            y: 0,
            visible: true,
            opacity: 1.0,
            clip: None,
            primitive: Some(RenderPrimitive::Group {
                shadow: None,
            }),
            subtree_hash: 0,
            invalidation: InvalidationClass::MeasureAndPlace,
        };
        self.graph.insert(node);
        panel
    }

    /// Update cursor position from inputd's VisibleState.
    pub(crate) fn update_cursor(&mut self, x: i32, y: i32) {
        self.graph.set_position(self.cursor_id, x, y);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_creates_four_default_nodes() {
        let shell = SystemUiShell::new(DeviceProfile::qemu_default());
        assert_eq!(shell.graph.node_count(), 4); // root, wallpaper, panels, cursor
    }

    #[test]
    fn device_profile_constants_match_display() {
        let p = DeviceProfile::qemu_default();
        assert_eq!(p.display_width, 1280);
        assert_eq!(p.display_height, 800);
        assert!(p.hw_cursor_supported);
        assert_eq!(p.min_ring_slots, 2);
    }

    #[test]
    fn mount_proof_panel_adds_node() {
        let mut shell = SystemUiShell::new(DeviceProfile::qemu_default());
        let before = shell.graph.node_count();
        let _panel_id = shell.mount_proof_panel();
        assert_eq!(shell.graph.node_count(), before + 1);
    }

    #[test]
    fn cursor_update_changes_position() {
        let mut shell = SystemUiShell::new(DeviceProfile::qemu_default());
        shell.graph.mark_all_clean();
        shell.update_cursor(100, 200);
        let node = shell.graph.find(shell.cursor_id).unwrap();
        assert_eq!(node.x, 100);
        assert_eq!(node.y, 200);
        assert_ne!(node.invalidation, InvalidationClass::Clean);
    }

    #[test]
    fn shell_mode_variants_are_distinct() {
        // Each shell mode must be a distinct value.
        let modes = [
            ShellMode::Desktop,
            ShellMode::Tablet,
            ShellMode::Phone,
            ShellMode::Automotive,
            ShellMode::Tv,
            ShellMode::Headless,
        ];
        for i in 0..modes.len() {
            for j in i + 1..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }
}
