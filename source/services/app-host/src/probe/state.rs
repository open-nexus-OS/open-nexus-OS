// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the mounted-app STATE — `DslApp`, the owned aggregate the event
//! loop re-layouts/re-renders after taps. Split out of `main.rs` (TASK-0307
//! structure ratchet): this file holds the data contract; behavior lives in
//! the sibling probe modules (`boot` mount/remount, `interaction`, `clock`,
//! `presentation`, `scroll`, `paint`, `anim`).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: exercised by every probe-module path + the QEMU ladder.

use super::anim;
use crate::effect_host;

/// The mounted DSL app: interpreter view + current layout + text runs.
/// Owned state so the event loop can re-layout/re-render after taps.
pub(super) struct DslApp {
    pub(super) view: nexus_dsl_runtime::View<'static>,
    pub(super) symbols: alloc::vec::Vec<alloc::string::String>,
    pub(super) keys: alloc::vec::Vec<u32>,
    pub(super) layout: nexus_layout::LayoutResult,
    /// The pre-order text runs the painter indexes by. The REQUESTED weight
    /// rides along because `FontSize` cannot give it back — a Light request at
    /// 36px already resolved to the SemiBold rung, so re-resolving a
    /// `.textFit` size from the FontSize alone would silently change weight.
    pub(super) texts: alloc::vec::Vec<crate::layout_diff::TextRun>,
    /// The service seam: `svc.*` effects (tap handlers AND the root
    /// initial-load effects) call through this over the provisioned slots.
    pub(super) host: effect_host::AppEffectHost,
    /// Base (page background) alpha: OPAQUE for a desktop/fullscreen
    /// surface (it IS the base layer — the shell/greeter owns every
    /// pixel; a translucent base let the wallpaper — or its solid-blue
    /// fallback — bleed through), frosted-translucent for floating
    /// windows (the glass material over the blurred backdrop).
    pub(super) base_alpha: u8,
    /// The surface is the desktop base (`level=desktop`): its regions are the shell's glass
    /// (dock, bar, cards on the wallpaper). A window's regions follow the materials rules
    /// (TASK-0337): one glass, the pane opaque, the rest tint.
    pub(super) desktop: bool,
    /// Surface dimensions (the WM-composed content rect, or the probe
    /// default). Layout width + render bounds derive from these — a
    /// full-screen shell lays out at the display size, a windowed app at its
    /// own size.
    pub(super) w: u32,
    pub(super) h: u32,
    /// Active theme mode (`THEME_*`, pushed by windowd). Selects the token
    /// set for every render so the app matches the compositor.
    pub(super) theme_mode: u8,
    /// Active shell profile (`PROFILE_*`, pushed by windowd). The device
    /// env every mount/interaction passes to the runtime.
    pub(super) shell_profile: u8,
    /// The `node_id` of the interactive box under the pointer (windowd MOVE events →
    /// `hover()`): its wash is a layer the compositor draws (RFC-0109) — a hover re-runs no
    /// layout and paints nothing.
    pub(super) hovered: Option<usize>,
    /// The pointer's last position over the surface (MOVE, DRAG, TAP): where a wheel notch
    /// goes, and what a scrolled layer hovers under a still pointer.
    pub(super) pointer: Option<(i32, i32)>,
    /// Whether the pointer sits over an editable (Change-bound) field —
    /// the windowd cursor-hint latch (I-beam), sent only on change.
    pub(super) hover_text: bool,
    /// Reused render row buffer (width×4). The bump allocator NEVER
    /// frees: a per-render `vec!` leaked ~5KB per hover repaint until the
    /// heap page-faulted (the "nothing clickable after mousing around"
    /// crash). One allocation at mount, resized on WM resize.
    pub(super) row_scratch: alloc::vec::Vec<u8>,
    /// The page's scroll layers (RFC-0109, `probe/scroll.rs`): every top-level `.scroll(...)`
    /// container, its offset, its physics and its band.
    pub(super) scroll_layers:
        [Option<super::scroll::ScrollLayer>; super::scroll::MAX_SCROLL_LAYERS],
    /// The top-level scroll containers past the bound: no layer — their content paints in the
    /// base at identity, clipped to the viewport (where the hit-test finds it too).
    pub(super) flat_views: alloc::vec::Vec<nexus_scene_raster::ScrollView>,
    /// Reused per pass: the views the base paints scroll content through (`probe/paint.rs`).
    pub(super) view_scratch: alloc::vec::Vec<nexus_scene_raster::ScrollView>,
    /// The surface's rows (base + store) — its VMO and the atlas region windowd keeps.
    pub(super) store_rows: u32,
    /// The store's used width the surface was created with (RFC-0109: windowd's store region).
    pub(super) store_w: u16,
    /// windowd placed no store at the last create (`NO_STORE`): every scroll layer is flat.
    pub(super) store_refused: bool,
    /// The wash patch's place in the store (`probe/band.rs`).
    pub(super) patch: (u16, u16),
    /// The wash's opacity — the theme's hover colour's alpha (the patch is opaque).
    pub(super) wash_opacity: u8,
    /// The structure changed since the last `OP_SURFACE_TREE` (regions, layers, bands).
    pub(super) tree_dirty: bool,
    /// The structure last sent — what windowd checks a motion against (`probe/band.rs`).
    pub(super) sent_tree: nexus_display_proto::surface_layers::SurfaceTree,
    /// The motion changed since the last `OP_SURFACE_MOTION` (an offset, the wash).
    pub(super) motion_dirty: bool,
    /// The layer set's geometry changed: the store is planned again (and the surface re-created
    /// when it no longer fits its rows).
    pub(super) store_replan: bool,
    /// What a paint pass's rows read beside the picked boxes (`probe/paint.rs`).
    pub(super) pass_ctx: super::paint::PassCtx,
    /// The DSL animation subsystem (`.animate`/`.transition`/`.effect`):
    /// the `AnimationDriver` physics + per-node paint transforms, ticked on
    /// the compositor frame pulse. Host owns the clock (the DSL stays pure);
    /// see `probe/anim.rs`.
    pub(super) anim: anim::AnimState,
    /// RFC-0077 locale packs from the payload container: (tag, catalog).
    pub(super) catalogs: alloc::vec::Vec<(alloc::string::String, nexus_dsl_runtime::Catalog)>,
    /// Index into `catalogs` of the ACTIVE locale (None = baked default).
    pub(super) active_catalog: Option<usize>,
    /// One-shot guard for the text-run dump (see `paint::dump_text_runs_once`).
    pub(super) text_dump_pending: bool,
    /// The last region-pushed locale tag (e.g. `de-DE`).
    pub(super) locale_tag: alloc::string::String,
    /// The active keymap layout tag (`us`/`de`/… — `device.keymap`).
    pub(super) keymap: alloc::string::String,
    /// Clock state (RFC-0076, `probe/clock.rs`): tz, format, next wait.
    pub(super) clock_tz: alloc::string::String,
    pub(super) clock_hour24: bool,
    /// Absolute monotonic deadline of the next clock tick (0 = tick as soon as possible).
    pub(super) clock_deadline_ns: u64,
    /// The modal depth last mirrored to windowd (`CONTROL_WIN_MODAL` on the 0↔n edge) and
    /// named in the `apphost: modal open` marker (TASK-0074, `probe/overlay.rs`).
    pub(super) modal_depth_sent: usize,
    /// The topmost modal last synced, by identity (node path, store instance): a modal that
    /// closed and another that opened between two paints keep the depth — the identity tells.
    pub(super) modal_top_sent: Option<(alloc::vec::Vec<u32>, u64)>,
    /// The transient overlay whose `.dismissAfter` is armed: (its node path, the absolute
    /// monotonic deadline). Re-armed only when a DIFFERENT transient layer appears.
    pub(super) transient_armed: Option<(alloc::vec::Vec<u32>, u64)>,
    /// Reused per-picked-box animation index (parallel to `vis_pick`;
    /// -1 = none) — resolved ONCE per repaint so the painter never scans
    /// the anims slice per box per row (the hover slowdown).
    pub(super) vis_anim: alloc::vec::Vec<i16>,
    /// Reused visibility index (box indices intersecting the repaint
    /// span) — per-row paint cost follows what is ON SCREEN, not the
    /// page's total box count (the 1000-message transcript contract).
    pub(super) vis_pick: alloc::vec::Vec<u32>,
    /// Reused (box index, texts index) pairs for the span's text runs.
    pub(super) vis_text: alloc::vec::Vec<(u32, u32)>,
    /// Damage span of the LAST structural tap (`merge_tap_damage`): `Some` =
    /// the diffed row window (may be empty — pixels identical), `None` =
    /// full repaint. Set by `tap()`, consumed once by the event loop.
    pub(super) tap_render_span: Option<(i32, i32)>,
    /// The keyboard overlay (`level: overlay`): a tap here IS a keystroke, so it
    /// is never traced — no per-tap line carries what was typed (privacy rule).
    pub(super) taps_are_keystrokes: bool,
    /// A traced tap repainted; its present owes `apphost: tap settled` (the live
    /// injector's anchor that the tap's frame is on screen).
    pub(super) tap_present_owed: bool,
    /// The press a drag may grow from (RFC-0095, `probe/drag.rs`): the press point, and the
    /// box that took `DragStart` once the pointer moved past the slop (`None` until then).
    pub(super) drag: Option<super::drag::Drag>,
}
