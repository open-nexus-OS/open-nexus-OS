// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The app export table of the mediation core (manifest v2.2, TASK-0081): which app
//! exports which ability under which app-owned permission — GENERATED at build time from
//! `userspace/apps/<app>/manifest.toml` (`build.rs`), with the ui-program and pre-session role
//! lists the OS spawn path reads. The permissions an app HOLDS are not here: since RFC-0111 §6
//! (ADR-0084) the one permission authority is policyd, published by bundlemgrd from the
//! verified volume, and this service asks it (`wire::GrantSource`).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 2 tests (the chat reference exports; unexported abilities stay unknown)

// Generated: `APP_UI_PROGRAMS`, `APP_PRE_SESSION`, `APP_SINGLETON`, `APP_EXPORTS` from the real
// bundle manifests.
include!(concat!(env!("OUT_DIR"), "/app_manifest_caps.rs"));

/// Whether `app_id` is a one-instance role (bundle_type shell / greeter / ime, RFC-0112).
pub fn is_singleton(app_id: &str) -> bool {
    APP_SINGLETON.contains(&app_id)
}

/// Finds the app exporting `ability` → `(exporter, ability, permission)`.
/// The resolve primitive of the mediation core (`crate::mediation`).
pub fn find_export(ability: &str) -> Option<(&'static str, &'static str, &'static str)> {
    APP_EXPORTS.iter().find_map(|(app, entries)| {
        entries.iter().find(|(a, _)| *a == ability).map(|(a, p)| (*app, *a, *p))
    })
}

/// The exports of one app: `(ability, app-owned permission)` pairs — the
/// resolve source for the mediated-then-direct app-to-app channel
/// (TASK-0081 decision C2; mediation itself rides with the broker).
pub fn exports_of(app_id: &str) -> &'static [(&'static str, &'static str)] {
    APP_EXPORTS.iter().find(|(app, _)| *app == app_id).map(|(_, entries)| *entries).unwrap_or(&[])
}

#[cfg(test)]
mod export_tests {
    use super::*;

    #[test]
    fn chat_reference_exports_are_known_consumer_permissions() {
        // The manifest-driven table carries the chat exports…
        let exports = exports_of("chat");
        assert!(
            exports.contains(&("chat.Send", "app.chat.SEND")),
            "chat.Send export missing: {exports:?}"
        );
        assert!(exports.contains(&("chat.Receive", "app.chat.RECEIVE")));
        // …and the mediation resolves them for a consumer the authority granted the permission.
        assert_eq!(
            find_export("chat.Send").map(|(app, _, p)| (app, p)),
            Some(("chat", "app.chat.SEND"))
        );
    }

    #[test]
    fn unexported_abilities_stay_unknown_fail_closed() {
        assert!(find_export("chat.Delete").is_none(), "nobody exports it");
        assert!(find_export("ghost.Send").is_none(), "no such app");
        assert!(exports_of("counter").is_empty());
    }
}
