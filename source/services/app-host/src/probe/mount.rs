// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! `DslApp` construction: validate the program bytes, mount the `View`, lay it
//! out at surface size, and (on a re-mount) restore the store snapshot.
//!
//! Split out of `probe/mod.rs` (structure-gate) — it is the app's BIRTH, not
//! its event loop, and `mod.rs` is the loop.

use super::*;

impl DslApp {
    /// Validates + mounts the program bytes and lays them out at
    /// surface size. `None` on any failure (fail-closed; the caller
    /// aborts without creating a window).
    pub(super) fn mount(
        nxir: &'static [u8],
        w: u32,
        h: u32,
        theme_mode: u8,
        shell_profile: u8,
        base_alpha: u8,
    ) -> Option<Self> {
        Self::mount_restoring(nxir, w, h, theme_mode, shell_profile, base_alpha, None)
    }

    /// [`Self::mount`] + an optional store snapshot from the PREVIOUS
    /// mount of the same program. A live re-theme is a drop-first remount
    /// — without this, every store reset to its defaults (open Control
    /// Center snapped shut ~1.5s after a moon/sun tap, sliders jumped
    /// back). Restored BEFORE the initial effects so service-loaded data
    /// still refreshes over the stale copy.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn mount_restoring(
        nxir: &'static [u8],
        w: u32,
        h: u32,
        theme_mode: u8,
        shell_profile: u8,
        base_alpha: u8,
        store_snapshot: Option<&[alloc::vec::Vec<(u32, nexus_dsl_runtime::Value)>]>,
    ) -> Option<Self> {
        use nexus_dsl_runtime::{IdentityLocale, View};

        let (nxir, catalogs) = locale::split_payload(nxir);
        let runtime = nexus_dsl_runtime::Runtime::mount(nxir).ok()?;
        let symbols = runtime.symbols().to_vec();
        emit_mounted_hash_marker(nxir);
        emit_window_intent_marker(nxir);
        let keys = locale::i18n_key_table(nxir);
        let tokens = tokens_for(theme_mode);
        // The pushed shell profile IS the device env: `device.profile`
        // selects the platform override arms (tablet base / desktop).
        let device = device_for(shell_profile, w, "", "", theme_mode);
        let mut view = {
            let locale = IdentityLocale { symbols: &symbols, keys: &keys };
            View::mount(nxir, tokens, &device, &locale).ok()?
        };
        // ADR-0065 / TASK-0077C P2b: route EMISSION into its own generation
        // arena. Set once, here — it never varies per call, so it is a field
        // and not an argument on six entry points. `View::emit` holds it only
        // while the scene is built; the live-instance sweep and the focus
        // record run after it, on the ordinary heap.
        #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
        view.set_frame_scope(&EMIT_ARENA);
        if let Some(snapshot) = store_snapshot {
            view.runtime.store_restore(snapshot);
        }
        // Declarative initial load (principles.md §5): an `@effect` event
        // dispatched by NOTHING is a ROOT — it runs once at mount. Fire the
        // roots BEFORE the first layout so the frame reflects service-loaded
        // data (e.g. the shell's `bundlemgr.enumerate` app grid). No
        // lifecycle hook; the runtime derives roots from the dataflow.
        let mut host = crate::effect_host::AppEffectHost::new(&symbols);
        {
            let locale = IdentityLocale { symbols: &symbols, keys: &keys };
            match view.run_initial_effects(tokens, &device, &locale, &mut host) {
                Ok(_) => raw_marker("APPHOST: initial effects ran"),
                Err(_) => raw_marker("apphost: FAIL initial effects"),
            }
        }
        let engine = nexus_layout::LayoutEngine::new();
        let layout = engine
            .layout_with_viewport(
                view.scene(),
                nexus_layout_types::FxPx::new(w as i32),
                // Bounded viewport: the surface height — Spacer/flex_grow
                // children distribute it, so DSL centering works (an
                // unbounded root hugged everything to the top-left).
                Some(nexus_layout_types::FxPx::new(h as i32)),
                &nexus_text_baked::measure_text::BakedTextMeasure,
            )
            .ok()?;
        let mut texts = alloc::vec::Vec::new();
        collect_texts(view.scene(), &mut 0, &mut texts);
        let mut app = Self {
            view,
            symbols,
            keys,
            layout,
            texts,
            host,
            base_alpha,
            // The desktop base paints at base alpha 0 (the wallpaper shows): its regions are
            // the shell's glass.
            desktop: base_alpha == 0,
            w,
            h,
            theme_mode,
            shell_profile,
            hovered: None,
            pointer: None,
            hover_text: false,
            row_scratch: alloc::vec![0u8; w as usize * 4],
            scroll_layers: [None; super::scroll::MAX_SCROLL_LAYERS],
            flat_views: alloc::vec::Vec::new(),
            view_scratch: alloc::vec::Vec::new(),
            store_rows: h,
            store_w: 0,
            store_refused: false,
            patch: (0, h as u16),
            wash_opacity: 0,
            tree_dirty: true,
            sent_tree: nexus_display_proto::surface_layers::SurfaceTree::EMPTY,
            motion_dirty: true,
            store_replan: true,
            pass_ctx: super::paint::PassCtx::EMPTY,
            anim: anim::AnimState::new(),
            catalogs,
            active_catalog: None,
            text_dump_pending: true,
            locale_tag: alloc::string::String::new(),
            keymap: alloc::string::String::new(),
            clock_tz: alloc::string::String::from("Europe/Berlin"),
            clock_hour24: true,
            clock_deadline_ns: 0,
            modal_depth_sent: 0,
            modal_top_sent: None,
            transient_armed: None,
            drag: None,
            vis_pick: alloc::vec::Vec::new(),
            vis_anim: alloc::vec::Vec::new(),
            vis_text: alloc::vec::Vec::new(),
            tap_render_span: None,
            input_rx_ns: None,
            hop_input: nexus_ux_budgets::Hop::new(),
            taps_are_keystrokes: false,
            tap_present_owed: false,
        };
        // Seed the animation state from the mounted scene: resting
        // transforms for value-tracked nodes, enter transitions for
        // `.transition` nodes (the first present's frame pulse plays them).
        app.anim_sync();
        // The page's scroll layers (RFC-0109): the store is planned from them before the surface
        // exists.
        app.sync_layers();
        Some(app)
    }
}

/// The emit-phase arena scope handed to the DSL runtime (ADR-0065).
///
/// A zero-sized singleton: exactly one app-host view tree per process, exactly
/// one emit region. The runtime only needs something that lives long enough.
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
struct EmitArena;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
static EMIT_ARENA: EmitArena = EmitArena;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
impl nexus_dsl_runtime::FrameScope for EmitArena {
    fn enter(&self) -> nexus_dsl_runtime::FrameScopeGuard {
        use nexus_service_entry::os::{
            abandon_frame_generation, close_frame_generation, frame_generation, Region,
        };
        // The allocator's own guard would end the generation on drop; the runtime
        // owns the lifetime instead, so leak that guard and end it by the
        // runtime's word: kept when the frame completed, abandoned when emission
        // failed — the scene on screen is never the next frame's to reset
        // (TASK-0344).
        core::mem::forget(frame_generation(Region::Emit));
        nexus_dsl_runtime::FrameScopeGuard::new(|completed| {
            if completed {
                close_frame_generation(Region::Emit);
            } else {
                abandon_frame_generation(Region::Emit);
            }
        })
    }
}
