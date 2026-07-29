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
    /// surface size. `None` on any failure (fail-closed; caller shows
    /// the probe fill).
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
        let device = device_for(shell_profile, w, "", "");
        let mut view = {
            let locale = IdentityLocale { symbols: &symbols, keys: &keys };
            View::mount(nxir, tokens, &device, &locale).ok()?
        };
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
            w,
            h,
            theme_mode,
            shell_profile,
            hovered: None,
            hover_text: false,
            row_scratch: alloc::vec![0u8; w as usize * 4],
            scroll_x: 0,
            scroll_y: 0,
            momentum: animation::ScrollMomentum::new(animation::ScrollConfig::default()),
            momentum_last_ns: 0,
            anim: anim::AnimState::new(),
            catalogs,
            active_catalog: None,
            locale_tag: alloc::string::String::new(),
            keymap: alloc::string::String::new(),
            clock_tz: alloc::string::String::from("Europe/Berlin"),
            clock_hour24: true,
            clock_next_wait_ms: 1_000,
            end_fired: false,
            vis_pick: alloc::vec::Vec::new(),
            vis_anim: alloc::vec::Vec::new(),
            vis_text: alloc::vec::Vec::new(),
            banded: false,
            last_band: None,
            alloc_band_h: 0,
            band_pick: alloc::vec::Vec::new(),
        };
        // Seed the animation state from the mounted scene: resting
        // transforms for value-tracked nodes, enter transitions for
        // `.transition` nodes (the first present's frame pulse plays them).
        app.anim_sync();
        Some(app)
    }
}
