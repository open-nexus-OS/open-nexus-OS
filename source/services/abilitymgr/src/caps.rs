// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: App capability (permission) resolution + validation — abilitymgr's
//! launch authority for manifest-declared `caps` (RFC-0065).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 4 tests
//!
//! App capability (permission) resolution + validation (RFC-0065).
//!
//! Each app bundle manifest declares the permissions it needs
//! (`caps = ["nexus.permission.WINDOW", …]`). `abilitymgr` is the launch
//! authority: before an ability runs, it resolves the app's declared caps from
//! the manifest and validates them against the known permission set, so a bundle
//! cannot launch while requesting a permission the system does not recognize
//! (a typo, or a permission removed/renamed). This is the precondition for the
//! per-app capability grant at spawn (the granted set == the validated manifest
//! set).
//!
//! The app → caps table is GENERATED at build time from `bundles/<app>/
//! manifest.toml` (see `build.rs`) — the same manifest files `bundlemgrd` derives
//! its registry from — so there is no hand-maintained duplicate of the manifest.

// Generated: `APP_MANIFEST_CAPS: &[(&str, &[&str])]` from the real bundle manifests.
include!(concat!(env!("OUT_DIR"), "/app_manifest_caps.rs"));

/// The permissions the platform recognizes. A manifest cap outside this set is
/// rejected at launch (fail-closed). Grows as real permissions are introduced;
/// the namespace mirrors the manifest (`nexus.permission.*`).
pub const KNOWN_PERMISSIONS: &[&str] = &[
    // Bind a window surface via windowd (every UI app needs this).
    "nexus.permission.WINDOW",
    // Post notifications via notifd (RFC-0065 notifications).
    "nexus.permission.NOTIFY",
    // Persist/read app state via statefsd.
    "nexus.permission.STATE",
    // Execute QuerySpec v1 queries via queryd (docs/dev/dsl/db-queries.md).
    "nexus.permission.QUERY",
    // System-role permissions (TASK-0080C). The privilege ceiling (nxb-pack)
    // only lets `greeter`/`shell` bundles ship these, so a plain app can never
    // hold them; here they are just recognized as valid platform permissions.
    // Drive the session/login authority (sessiond) — greeter only.
    "nexus.permission.SESSION",
    // Launch apps via abilitymgr — shell only.
    "nexus.permission.LAUNCH",
    // Enumerate the installed-app registry via bundlemgrd — shell only.
    "nexus.permission.ENUMERATE",
    // Read/write SYSTEM settings via settingsd — settings-type only. (Distinct
    // from STATE, which is an app's OWN state via statefsd.)
    "nexus.permission.SETTINGS",
];

/// `true` if `cap` is a recognized platform permission — OR an app-owned
/// permission (`app.<bundle>.<CAP>`, manifest v2.2 exports) that some
/// installed app actually EXPORTS. Fail-closed: an `app.*` cap nobody
/// exports is unknown, exactly like a typoed platform permission.
pub fn is_known_permission(cap: &str) -> bool {
    KNOWN_PERMISSIONS.contains(&cap) || is_exported_permission(cap)
}

/// `true` if some installed app exports `cap` under its own namespace
/// (build-time `APP_EXPORTS` table from `userspace/apps/*/manifest.toml`).
pub fn is_exported_permission(cap: &str) -> bool {
    APP_EXPORTS
        .iter()
        .any(|(_, entries)| entries.iter().any(|(_, permission)| *permission == cap))
}

/// Finds the app exporting `ability` → `(exporter, ability, permission)`.
/// The resolve primitive of the mediation core (`crate::mediation`).
pub fn find_export(ability: &str) -> Option<(&'static str, &'static str, &'static str)> {
    APP_EXPORTS.iter().find_map(|(app, entries)| {
        entries
            .iter()
            .find(|(a, _)| *a == ability)
            .map(|(a, p)| (*app, *a, *p))
    })
}

/// The exports of one app: `(ability, app-owned permission)` pairs — the
/// resolve source for the mediated-then-direct app-to-app channel
/// (TASK-0081 decision C2; mediation itself rides with the broker).
pub fn exports_of(app_id: &str) -> &'static [(&'static str, &'static str)] {
    APP_EXPORTS
        .iter()
        .find(|(app, _)| *app == app_id)
        .map(|(_, entries)| *entries)
        .unwrap_or(&[])
}

/// The capabilities an app's manifest declares, or `&[]` if the app has no
/// manifest entry (an app with no declared permissions).
pub fn required_caps(app_id: &str) -> &'static [&'static str] {
    APP_MANIFEST_CAPS
        .iter()
        .find(|(id, _)| *id == app_id)
        .map(|(_, caps)| *caps)
        .unwrap_or(&[])
}

/// Validates a set of declared caps against the known permission set. Returns the
/// first unrecognized permission (borrowed from the input) on failure — the
/// caller emits it so a bad manifest is diagnosable by name.
pub fn first_unknown<'a>(caps: &'a [&'a str]) -> Option<&'a str> {
    caps.iter().copied().find(|c| !is_known_permission(c))
}

/// `Ok` iff every declared cap is a known permission.
pub fn validate<'a>(caps: &'a [&'a str]) -> Result<(), &'a str> {
    match first_unknown(caps) {
        Some(bad) => Err(bad),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_permissions_validate() {
        assert!(validate(&["nexus.permission.WINDOW"]).is_ok());
        assert!(validate(&["nexus.permission.WINDOW", "nexus.permission.NOTIFY"]).is_ok());
        assert!(validate(&[]).is_ok(), "no declared caps is valid");
    }

    #[test]
    fn unknown_permission_is_rejected_by_name() {
        let caps = ["nexus.permission.WINDOW", "nexus.permission.BOGUS"];
        assert_eq!(validate(&caps), Err("nexus.permission.BOGUS"));
        assert_eq!(first_unknown(&caps), Some("nexus.permission.BOGUS"));
    }

    /// Guard: every routable permission in the SDK routing SSOT must be a
    /// permission this authority actually recognizes — otherwise the launch
    /// provisioner would try to grant a route for a cap `validate` rejects.
    #[test]
    fn every_sdk_route_permission_is_known() {
        for r in nexus_sdk_routes::SERVICE_ROUTES {
            assert!(
                is_known_permission(r.permission),
                "SDK route `svc.{}` needs permission `{}` which is not in KNOWN_PERMISSIONS",
                r.svc,
                r.permission
            );
        }
    }

    #[test]
    fn real_app_manifests_only_declare_known_permissions() {
        // The shipped bundles (generated from userspace/apps/<app>/manifest.toml) must not
        // drift to an unrecognized permission — caught here at `cargo test`.
        for (app, caps) in APP_MANIFEST_CAPS {
            assert!(
                validate(caps).is_ok(),
                "app `{app}` declares an unknown permission: {:?}",
                first_unknown(caps)
            );
        }
    }

    #[test]
    fn required_caps_resolves_from_manifest() {
        // chat + search both declare WINDOW in their manifests.
        assert!(required_caps("chat").contains(&"nexus.permission.WINDOW"));
        assert!(required_caps("search").contains(&"nexus.permission.WINDOW"));
        // An app with no manifest entry has no required caps.
        assert_eq!(required_caps("definitely-not-installed"), &[] as &[&str]);
    }
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
        // …and a CONSUMER may declare them in caps (validate() accepts).
        assert!(is_known_permission("app.chat.SEND"));
        assert!(validate(&["nexus.permission.WINDOW", "app.chat.RECEIVE"]).is_ok());
    }

    #[test]
    fn unexported_app_permissions_stay_unknown_fail_closed() {
        assert!(!is_known_permission("app.chat.DELETE"), "nobody exports it");
        assert!(!is_known_permission("app.ghost.SEND"), "no such app");
        assert!(validate(&["app.ghost.SEND"]).is_err());
        assert!(exports_of("counter").is_empty());
    }
}
