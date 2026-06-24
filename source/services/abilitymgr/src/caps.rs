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
];

/// `true` if `cap` is a recognized platform permission.
pub fn is_known_permission(cap: &str) -> bool {
    KNOWN_PERMISSIONS.contains(&cap)
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

    #[test]
    fn real_app_manifests_only_declare_known_permissions() {
        // The shipped bundles (generated from bundles/<app>/manifest.toml) must not
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
