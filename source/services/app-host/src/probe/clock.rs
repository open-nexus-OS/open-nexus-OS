// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! app-host `DslApp` clock subsystem (RFC-0076): the HOST drives the minute
//! tick — it queries `timed`'s UTC walltime over the `svc.time` route,
//! converts via `tz-lite` using the windowd-pushed region data, and
//! dispatches a `ClockEvent::Tick(time, date)` into apps that declare it.
//! Apps without the event are untouched (no wakeups, no state).

use super::*;

/// English display names (v1 — localized names ride the locale packs,
/// RFC-0077 follow-up). ISO weekday order (0 = Monday).
const WEEKDAYS: [&str; 7] =
    ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

impl super::DslApp {
    /// Whether the mounted program declares the clock event.
    pub(super) fn clock_supported(&self) -> bool {
        self.view.runtime.event_case("ClockEvent", "Tick").is_some()
    }

    /// Region push from windowd (`OP_SURFACE_REGION`): update tz/hour-format
    /// and re-tick immediately; a locale change swaps the active pack catalog
    /// and re-emits the scene (RFC-0077); a keymap change re-emits the
    /// `device.keymap` axis + dispatches `KeymapEvent::Changed` (RFC-0075
    /// Phase 8b). Returns whether a re-render is needed.
    pub(super) fn apply_region(
        &mut self,
        hour_fmt: u8,
        locale: &str,
        tz: &str,
        keymap: &str,
    ) -> bool {
        self.clock_hour24 = hour_fmt == nexus_display_proto::surface_text::REGION_HOUR_24;
        if tz_lite::zone(tz).is_some() {
            self.clock_tz.clear();
            self.clock_tz.push_str(tz);
        }
        let locale_changed = self.apply_locale(locale);
        let keymap_changed = self.apply_keymap(keymap);
        let ticked = self.clock_tick();
        locale_changed || keymap_changed || ticked
    }

    /// Applies a pushed keymap tag: updates the `device.keymap` env axis
    /// (reemit re-selects `if device.keymap` arms — the size-class pattern)
    /// and dispatches `KeymapEvent::Changed(tag)` into programs that declare
    /// it (the ime-ui reloads its data-driven rows). Returns scene-changed.
    fn apply_keymap(&mut self, keymap: &str) -> bool {
        use nexus_dsl_runtime::Value;
        if keymap.is_empty() || keymap == self.keymap {
            return false;
        }
        self.keymap.clear();
        self.keymap.push_str(keymap);
        let tokens = tokens_for(self.theme_mode);
        let device = device_for(self.shell_profile, self.w, &self.locale_tag, &self.keymap);
        let mut changed = false;
        let reemit_ok = {
            let locale_src = super::app_locale!(self);
            self.view.reemit(tokens, &device, &locale_src).is_ok()
        };
        if reemit_ok {
            self.relayout_retained();
            self.hovered = None;
            self.anim_sync();
            changed = true;
        }
        if let Some((event, case)) = self.view.runtime.event_case("KeymapEvent", "Changed") {
            let damage = {
                let locale_src = super::app_locale!(self);
                self.view.dispatch(
                    tokens,
                    &device,
                    &locale_src,
                    &mut self.host,
                    event,
                    case,
                    alloc::vec![Value::Str(alloc::string::String::from(keymap))],
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

    /// Selects the pack catalog for a pushed locale tag — exact tag first,
    /// then primary language subtag (`de-DE` → `de`); no match = baked
    /// default. On an actual swap: `reemit` + relayout + bounded proof marker
    /// (tag only, never content). Returns whether the scene changed.
    fn apply_locale(&mut self, locale: &str) -> bool {
        if locale.is_empty() || locale == self.locale_tag {
            return false;
        }
        self.locale_tag.clear();
        self.locale_tag.push_str(locale);
        let lang = locale.split('-').next().unwrap_or(locale);
        let idx = self
            .catalogs
            .iter()
            .position(|(tag, _)| tag == locale)
            .or_else(|| self.catalogs.iter().position(|(tag, _)| tag == lang));
        if idx == self.active_catalog {
            return false;
        }
        self.active_catalog = idx;
        let tokens = tokens_for(self.theme_mode);
        let device = device_for(self.shell_profile, self.w, &self.locale_tag, &self.keymap);
        let locale_src = super::app_locale!(self);
        if self.view.reemit(tokens, &device, &locale_src).is_err() {
            raw_marker("apphost: FAIL locale reemit");
            return false;
        }
        self.relayout_retained();
        self.hovered = None;
        self.anim_sync();
        // Bounded end-to-end proof (RFC-0077): ≤8 per boot, tag only.
        static LOCALE_MARKS: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);
        let marks = LOCALE_MARKS.load(core::sync::atomic::Ordering::Relaxed);
        if marks < 8 {
            LOCALE_MARKS.store(marks + 1, core::sync::atomic::Ordering::Relaxed);
            raw_marker(&alloc::format!("apphost: locale {locale} applied"));
        }
        true
    }

    /// The event-loop wait: animation ticks (12ms) win; a declared clock
    /// event wakes at the next minute boundary; otherwise pure blocking.
    pub(super) fn event_wait(&self, animating: bool) -> nexus_ipc::Wait {
        if animating {
            nexus_ipc::Wait::Timeout(core::time::Duration::from_millis(12))
        } else if self.clock_supported() {
            nexus_ipc::Wait::Timeout(core::time::Duration::from_millis(
                self.clock_next_wait_ms.max(250),
            ))
        } else {
            nexus_ipc::Wait::Blocking
        }
    }

    /// Queries walltime, converts, dispatches `ClockEvent::Tick(time, date)`.
    /// Returns whether the app changed (re-render). Bounded: one IPC + const
    /// math per minute.
    pub(super) fn clock_tick(&mut self) -> bool {
        use nexus_dsl_runtime::Value;
        let Some((event, case)) = self.view.runtime.event_case("ClockEvent", "Tick") else {
            return false;
        };
        #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
        let wall = crate::time_client::walltime_now();
        #[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
        let wall: Option<u64> = None;
        let Some(epoch_ns) = wall else {
            self.clock_next_wait_ms = 60_000;
            return false; // placeholder stays — never fake time
        };
        let Some(zone) = tz_lite::zone(&self.clock_tz) else {
            self.clock_next_wait_ms = 60_000;
            return false;
        };
        let civil = tz_lite::to_civil(epoch_ns, zone);
        let mut hm = [0u8; 8];
        let n = tz_lite::format_hm(&civil, self.clock_hour24, &mut hm);
        let time = alloc::string::String::from(core::str::from_utf8(&hm[..n]).unwrap_or("--:--"));
        let date = alloc::format!(
            "{}, {} {}",
            WEEKDAYS[usize::from(civil.weekday.min(6))],
            MONTHS[usize::from(civil.month.saturating_sub(1).min(11))],
            civil.day
        );
        // Schedule the next wakeup just past the minute boundary.
        let sec_in_min = (epoch_ns / 1_000_000_000) % 60;
        self.clock_next_wait_ms = (60 - sec_in_min) * 1000 + 200;
        let tokens = tokens_for(self.theme_mode);
        let device = device_for(self.shell_profile, self.w, &self.locale_tag, &self.keymap);
        let locale = super::app_locale!(self);
        let damage = self.view.dispatch(
            tokens,
            &device,
            &locale,
            &mut self.host,
            event,
            case,
            alloc::vec![Value::Str(time), Value::Str(date)],
        );
        let changed = match damage {
            Ok(nexus_dsl_runtime::Damage::Layout) => {
                self.relayout_retained();
                true
            }
            Ok(nexus_dsl_runtime::Damage::Paint) => true,
            _ => false,
        };
        if changed {
            // One-shot end-to-end proof (RFC-0076): the first tick that
            // changed the app's clock state. Count-only.
            static TICK_MARKED: core::sync::atomic::AtomicBool =
                core::sync::atomic::AtomicBool::new(false);
            if !TICK_MARKED.swap(true, core::sync::atomic::Ordering::Relaxed) {
                raw_marker("apphost: clock tick applied");
            }
        }
        changed
    }
}
