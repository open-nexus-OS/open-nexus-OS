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
    Ok(())
}
