// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0083 presentation-snapshot receive (`OP_SURFACE_SETTINGS`) —
//! ONE idempotent frame carries theme, accent, shell profile, hour format,
//! locale, timezone and keymap. Every change is a REEMIT, never a remount:
//! colors resolve at emit time from the Tokens trait, and `if device.*` arms
//! re-select page structure (the size-class precedent) — store state survives
//! by construction and `@effect on Load` does NOT re-run. Programs that load
//! profile-dependent data declare `ProfileEvent::Changed` (the
//! `KeymapEvent::Changed` precedent in `probe/clock.rs`).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: runtime-level reemit proofs in `tests/dsl_apps_conformance`
//! (`shell_presentation.rs`); wire reject matrix in `nexus-display-proto`.
//! RFC: docs/rfcs/RFC-0083-settings-distribution-v2-single-authority-versioned-snapshots.md

use nexus_display_proto::surface_settings::SurfaceSettings;

use super::{device_for, tokens_for};

impl super::DslApp {
    /// Applies a presentation snapshot field-by-field. Returns whether the
    /// scene changed (the caller repaints). Idempotent: a duplicate or
    /// re-ordered snapshot is free — every arm compares before acting, so
    /// the caller's `gen` check is a short-circuit, not a correctness need.
    pub(super) fn apply_settings(&mut self, snap: &SurfaceSettings<'_>) -> bool {
        // Region half first (locale / tz / hour / keymap): the existing
        // RFC-0076/0077 recipe — catalog swap + reemit on locale change,
        // KeymapEvent dispatch on keymap change, clock re-tick on tz/hour.
        let mut changed = self.apply_region(snap.hour_fmt, snap.locale, snap.tz, snap.keymap);
        changed |= self.apply_theme(snap.theme);
        changed |= self.apply_profile(snap.profile);
        changed
    }

    /// Theme/accent change = a REPAINT: swap the token set and re-emit.
    /// The packed byte (mode nibble + accent nibble) is the identity — any
    /// difference re-resolves every color through `tokens_for` at emit time.
    /// This replaces the legacy full remount (`mount_restoring` + store
    /// snapshot + re-run initial effects), which is what turned one accent
    /// tap into three app teardowns and an input-backpressure storm.
    fn apply_theme(&mut self, packed: u8) -> bool {
        if packed == self.theme_mode {
            return false;
        }
        self.theme_mode = packed;
        if !self.reemit_current("theme") {
            return false;
        }
        true
    }

    /// Shell-profile change = ALSO a reemit: `device.profile` is an env axis
    /// exactly like `device.sizeClass`, and `reemit_for_size_class` already
    /// proves structure re-selection without a remount. Window intent needs
    /// no re-announcement — it is a program constant (`root.get_window()`),
    /// not a profile-dependent value. Programs loading profile-dependent
    /// data declare `ProfileEvent::Changed(tag)`; `@effect on Load` does not
    /// re-run (RFC-0083 semantics change, documented in docs/dev/dsl).
    fn apply_profile(&mut self, profile: u8) -> bool {
        use nexus_dsl_runtime::Value;
        if profile == self.shell_profile {
            return false;
        }
        self.shell_profile = profile;
        let mut changed = self.reemit_current("profile");
        // Data-reload hook for programs that declare it.
        if let Some((event, case)) = self.view.runtime.event_case("ProfileEvent", "Changed") {
            let tokens = tokens_for(self.theme_mode);
            let device = device_for(
                self.shell_profile,
                self.w,
                &self.locale_tag,
                &self.keymap,
                self.theme_mode,
            );
            let tag = profile_tag(profile);
            let damage = {
                let locale_src = super::app_locale!(self);
                self.view.dispatch(
                    tokens,
                    &device,
                    &locale_src,
                    &mut self.host,
                    event,
                    case,
                    alloc::vec![Value::Str(alloc::string::String::from(tag))],
                )
            };
            match damage {
                Ok(nexus_dsl_runtime::Damage::Layout) => {
                    self.relayout_retained();
                    changed = true;
                }
                Ok(nexus_dsl_runtime::Damage::Paint) => changed = true,
                _ => {}
            }
        }
        changed
    }

    /// One reemit under the CURRENT env (tokens + device + locale) with the
    /// standard follow-ups: relayout the retained boxes, drop the stale hover
    /// anchor, reconcile animations. The shared tail of every settings arm.
    fn reemit_current(&mut self, what: &str) -> bool {
        let tokens = tokens_for(self.theme_mode);
        let device =
            device_for(self.shell_profile, self.w, &self.locale_tag, &self.keymap, self.theme_mode);
        let ok = {
            let locale_src = super::app_locale!(self);
            self.view.reemit(tokens, &device, &locale_src).is_ok()
        };
        if !ok {
            // A failed reemit must never be silent: the store may have state
            // the old scene no longer matches. Bounded by settings-change rate.
            super::raw_marker(&alloc::format!("apphost: FAIL {what} reemit"));
            return false;
        }
        self.relayout_retained();
        self.hovered = None;
        self.anim_sync();
        true
    }
}

/// Wire profile byte → the `device.profile` tag apps see (`ProfileEvent`
/// payload). Mirrors `probe/env.rs::device_for`'s vocabulary.
fn profile_tag(profile: u8) -> &'static str {
    use nexus_display_proto::client_surface as wire;
    match profile {
        wire::PROFILE_DESKTOP => "desktop",
        wire::PROFILE_PHONE => "phone",
        wire::PROFILE_TV => "tv",
        _ => "tablet",
    }
}

/// The main-loop arm body: gen short-circuit, apply, and the bookkeeping
/// that keeps the remount re-apply + legacy-arm state coherent while the
/// migration window still remounts on OP 16/17. Returns whether to repaint.
#[allow(clippy::too_many_arguments)] // reason: main-loop locals, one call site
pub(super) fn absorb_snapshot(
    app: Option<&mut super::DslApp>,
    snap: &SurfaceSettings<'_>,
    last_gen: &mut Option<u32>,
    theme_mode: &mut u8,
    shell_profile: &mut u8,
    last_region: &mut Option<super::boot::RegionPush>,
) -> bool {
    if *last_gen == Some(snap.gen) {
        return false;
    }
    *last_gen = Some(snap.gen);
    let dirty = app.is_some_and(|d| d.apply_settings(snap));
    *theme_mode = snap.theme;
    *shell_profile = snap.profile;
    *last_region = Some(super::boot::RegionPush {
        hour_fmt: snap.hour_fmt,
        locale: alloc::string::String::from(snap.locale),
        tz: alloc::string::String::from(snap.tz),
        keymap: alloc::string::String::from(snap.keymap),
    });
    static GEN_MARKS: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);
    let marks = GEN_MARKS.load(core::sync::atomic::Ordering::Relaxed);
    if marks < 8 {
        GEN_MARKS.store(marks + 1, core::sync::atomic::Ordering::Relaxed);
        super::raw_marker(&alloc::format!("apphost: presentation gen={} applied", snap.gen));
    }
    dirty
}
