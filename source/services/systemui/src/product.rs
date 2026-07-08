// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: SystemUI **product/deployment manifest** — the single declarative
//! point that chooses *which* profile + shell (+ theme/policy) the device boots
//! into. This is the "set the shell in SystemUI" config the rest of the runtime
//! resolves (see `docs/dev/ui/foundations/layout/profiles.md`): a default mode
//! that forks/products can override by shipping their own product manifest, with
//! a complete kiosk lockdown expressible purely declaratively.
//!
//! A product references a profile id and a shell id; the [`crate::registry`]
//! resolves those to manifests and validates the pairing, so an unknown id or an
//! incompatible profile/shell pair fails deterministically.
//!
//! OWNERS: @ui
//! STATUS: Experimental
//! API_STABILITY: Unstable

use alloc::string::String;

use crate::profile::{optional_string_field, parse_entries, string_field, Result, SystemUiError};

/// A product/deployment manifest: the chosen profile + shell plus optional
/// theme/policy/deployment metadata. The config point a fork edits to rebrand or
/// lock down a device without touching core SystemUI logic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductManifest {
    pub id: String,
    /// Referenced profile id (resolved via the registry).
    pub profile: String,
    /// Referenced shell id (resolved via the registry).
    pub shell: String,
    /// Optional theme id (empty when unset).
    pub theme: String,
    /// Optional policy preset id, e.g. `locked-down` for a kiosk (empty when unset).
    pub policy_preset: String,
    /// Optional deployment tag, e.g. `warehouse-floor` (empty when unset).
    pub deployment: String,
    /// Session mode (TASK-0081 boot TOML): how the device enters a session.
    /// `Greeter` shows the login greeter (default — the TASK-0065B contract);
    /// `Auto` logs the sole/default user in directly (kiosk/single-purpose).
    pub session: SessionMode,
    /// Greeter APP id (empty = the built-in greeter view). Any app
    /// implementing the greeter contract (`svc.session.*`) can be the
    /// greeter — the registry resolves it like the shell; AUTHORITY is
    /// unchanged (sessiond decides, this only picks the renderer).
    pub greeter: String,
}

/// How the device enters a session at boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionMode {
    /// Show the login greeter; sessiond gates the shell (default).
    Greeter,
    /// Auto-login the default user (kiosk/single-purpose devices).
    Auto,
}

pub fn parse_product_manifest(input: &str) -> Result<ProductManifest> {
    let entries = parse_entries(input)?;
    let manifest = ProductManifest {
        id: string_field(&entries, "", "id")?,
        profile: string_field(&entries, "", "profile")?,
        shell: string_field(&entries, "", "shell")?,
        theme: optional_string_field(&entries, "", "theme")?.unwrap_or_default(),
        policy_preset: optional_string_field(&entries, "", "policy_preset")?.unwrap_or_default(),
        deployment: optional_string_field(&entries, "", "deployment")?.unwrap_or_default(),
        session: match optional_string_field(&entries, "", "session")?.as_deref() {
            None | Some("greeter") => SessionMode::Greeter,
            Some("auto") => SessionMode::Auto,
            Some(_) => return Err(SystemUiError::InvalidManifest),
        },
        greeter: optional_string_field(&entries, "", "greeter")?.unwrap_or_default(),
    };
    validate_product(&manifest)?;
    Ok(manifest)
}

/// Structural validation only (the referenced ids exist + are compatible is the
/// registry's job, in [`crate::registry::resolve_product`]).
pub fn validate_product(manifest: &ProductManifest) -> Result<()> {
    if manifest.id.is_empty() || manifest.profile.is_empty() || manifest.shell.is_empty() {
        return Err(SystemUiError::InvalidManifest);
    }
    // Auto-login must not ALSO name a greeter app: the pair is contradictory
    // and a kiosk misconfiguration should fail at parse, not at boot.
    if manifest.session == SessionMode::Auto && !manifest.greeter.is_empty() {
        return Err(SystemUiError::InvalidManifest);
    }
    // ROLE-TYPE check (TASK-0080C, rides with the greeter-swap): the app named
    // by `greeter` must be a `bundle_type = greeter` bundle, and `shell` an
    // `app`/`shell` bundle. That cross-check needs the app's bundle_type from
    // bundlemgrd — done when systemui RESOLVES + launches the role app (the
    // greeter-swap). The security floor already holds regardless: the pack-time
    // privilege ceiling (nxb-pack) lets ONLY a greeter-type bundle hold
    // `SESSION`, so a mis-pointed `greeter =` can never actually drive login.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(extra: &str) -> String {
        format!("id = \"p\"\nprofile = \"desktop\"\nshell = \"desktop\"\n{extra}\n")
    }

    #[test]
    fn session_defaults_to_greeter() {
        let m = parse_product_manifest(&base("")).expect("parses");
        assert_eq!(m.session, SessionMode::Greeter);
        assert!(m.greeter.is_empty());
    }

    #[test]
    fn session_auto_and_greeter_app_parse() {
        let m = parse_product_manifest(&base("session = \"auto\"")).expect("parses");
        assert_eq!(m.session, SessionMode::Auto);
        let m = parse_product_manifest(&base("session = \"greeter\"\ngreeter = \"login-app\""))
            .expect("parses");
        assert_eq!(m.session, SessionMode::Greeter);
        assert_eq!(m.greeter, "login-app");
    }

    #[test]
    fn invalid_session_and_contradictory_auto_greeter_fail_closed() {
        assert!(parse_product_manifest(&base("session = \"maybe\"")).is_err());
        assert!(parse_product_manifest(&base("session = \"auto\"\ngreeter = \"x\"")).is_err());
    }
}
