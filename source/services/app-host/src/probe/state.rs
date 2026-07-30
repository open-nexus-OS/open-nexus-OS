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
    pub(super) texts:
        alloc::vec::Vec<(usize, alloc::string::String, nexus_text_baked::FontSize, [u8; 4])>,
    /// The service seam: `svc.*` effects (tap handlers AND the root
    /// initial-load effects) call through this over the provisioned slots.
    pub(super) host: effect_host::AppEffectHost,
    /// Base (page background) alpha: OPAQUE for a desktop/fullscreen
    /// surface (it IS the base layer — the shell/greeter owns every
    /// pixel; a translucent base let the wallpaper — or its solid-blue
    /// fallback — bleed through), frosted-translucent for floating
    /// windows (the glass material over the blurred backdrop).
    pub(super) base_alpha: u8,
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
    /// The `node_id` of the interactive box under the pointer (windowd
    /// MOVE events → `hover()`), washed at PAINT time. Presentation-only
    /// state: hover never re-runs layout (pretext), only a repaint.
    pub(super) hovered: Option<usize>,
    /// Whether the pointer sits over an editable (Change-bound) field —
    /// the windowd cursor-hint latch (I-beam), sent only on change.
    pub(super) hover_text: bool,
    /// Reused render row buffer (width×4). The bump allocator NEVER
    /// frees: a per-render `vec!` leaked ~5KB per hover repaint until the
    /// heap page-faulted (the "nothing clickable after mousing around"
    /// crash). One allocation at mount, resized on WM resize.
    pub(super) row_scratch: alloc::vec::Vec<u8>,
    /// Scroll offsets of the page's `.scroll(...)` viewport (paint-time
    /// state like `hovered`: scrolling NEVER re-runs layout and NEVER
    /// allocates — the retained boxes are repainted shifted).
    pub(super) scroll_x: i32,
    pub(super) scroll_y: i32,
    /// The vertical scroll PHYSICS (SSOT `animation::ScrollMomentum`):
    /// wheel notches extend a target the position eases toward; the loop
    /// ticks it while `is_animating` — apple-smooth, never a hard jump.
    pub(super) momentum: animation::ScrollMomentum,
    /// Last physics tick (ns) for dt integration.
    pub(super) momentum_last_ns: u64,
    /// The PAGER physics (`.scroll(paged)` viewports — launcher pager):
    /// the same `ScrollMomentum`, driving `scroll_x` toward whole-page
    /// multiples of the viewport width (glide-eased snap). A page has ONE
    /// scroll region, so this and `momentum` are never active together.
    pub(super) pager: animation::ScrollMomentum,
    /// Wheel lock: notches before this instant are swallowed, so one flick
    /// turns ONE page instead of rushing through several (spec: 360 ms).
    pub(super) pager_lock_until_ns: u64,
    /// The DSL animation subsystem (`.animate`/`.transition`/`.effect`):
    /// the `AnimationDriver` physics + per-node paint transforms, ticked on
    /// the compositor frame pulse. Host owns the clock (the DSL stays pure);
    /// see `probe/anim.rs`.
    pub(super) anim: anim::AnimState,
    /// RFC-0077 locale packs from the payload container: (tag, catalog).
    pub(super) catalogs: alloc::vec::Vec<(alloc::string::String, nexus_dsl_runtime::Catalog)>,
    /// Index into `catalogs` of the ACTIVE locale (None = baked default).
    pub(super) active_catalog: Option<usize>,
    /// The last region-pushed locale tag (e.g. `de-DE`).
    pub(super) locale_tag: alloc::string::String,
    /// The active keymap layout tag (`us`/`de`/… — `device.keymap`).
    pub(super) keymap: alloc::string::String,
    /// Clock state (RFC-0076, `probe/clock.rs`): tz, format, next wait.
    pub(super) clock_tz: alloc::string::String,
    pub(super) clock_hour24: bool,
    pub(super) clock_next_wait_ms: u64,
    /// EndReached latch: fired once per approach to the content end;
    /// re-armed whenever layout re-runs (content grew/shrank).
    pub(super) end_fired: bool,
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
    /// WebRender compositor-scroll: this surface renders a TALL packed band
    /// (fixed header + fixed footer + the whole resident scroll content) ONCE
    /// and windowd/gpud shift the source row per scroll. When set, wheel is
    /// owned by the compositor: the app never self-scrolls/re-renders on a
    /// notch — it only mirrors the pushed `INPUT_KIND_SCROLL_POS` and
    /// re-renders the band on a content change (LoadMore). `false` = the
    /// legacy paint-time-`dy` scroll (unchanged).
    /// Damage span of the LAST structural tap (`merge_tap_damage`): `Some` =
    /// the diffed row window (may be empty — pixels identical), `None` =
    /// full repaint. Set by `tap()`, consumed once by the event loop.
    pub(super) tap_render_span: Option<(i32, i32)>,
    /// Whether the last structural tap may have changed the GLASS-REGION
    /// set (an overlay opened/closed): a span present must still re-declare
    /// layers then — `submit_layers` used to run on full presents only.
    pub(super) layers_dirty: bool,
    pub(super) banded: bool,
    /// The band geometry (`header_h, footer_h, content_h`) the CURRENT
    /// windowd surface was created with — the re-negotiation detector
    /// compares `band_geometry()` after every model change (a structural
    /// view swap moves the packed-band slices; mount-fixed slices painted
    /// chat's composer over the bubbles).
    pub(super) last_band: Option<(u32, u32, u32)>,
    /// The tall VMO/band height (rows) allocated at create — render_band
    /// clamps to it so a LoadMore that grows content never overflows the VMO
    /// (the compositor band is the same fixed size; `tail(…)` keeps it finite).
    pub(super) alloc_band_h: u32,
    /// Reused pick buffer for render_band's unclipped (fixed header/footer)
    /// region — recycled like `vis_pick`, never allocated per render.
    pub(super) band_pick: alloc::vec::Vec<u32>,
}
