// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: SystemUI **manifest registry + resolver** — the catalog of known
//! profile/shell/product manifests and the logic that turns a product id into a
//! fully resolved, validated shell configuration plus a stable device
//! environment. This is the runtime realisation of "the shell is set in
//! SystemUI": pick a product (the default, or a fork/kiosk override) and the
//! registry resolves profile + shell, validates the pairing, and derives the
//! `device.*` environment the DSL/windowd shell renders against. Runtime shell
//! switching (e.g. convertible desktop↔tablet) re-resolves through the same path.
//!
//! Forks add a profile/shell/product by adding a manifest + one registry entry —
//! NOT by patching core enums (see `docs/dev/ui/foundations/layout/profiles.md`).
//! Unknown ids and incompatible pairings fail deterministically.
//!
//! OWNERS: @ui
//! STATUS: Experimental
//! API_STABILITY: Unstable

use alloc::string::String;
use alloc::vec::Vec;

use crate::product::{parse_product_manifest, ProductManifest};
use crate::profile::{
    parse_profile_manifest, DeviceInput, ProfileManifest, Result, SystemUiError,
};
use crate::shell::{parse_shell_manifest, validate_profile_shell, ShellManifest};

/// One registered manifest: a stable id paired with its (compile-time embedded)
/// TOML source. Forks append entries; the resolver never hardcodes ids.
pub struct ManifestEntry {
    pub id: &'static str,
    pub toml: &'static str,
}

/// The default product booted when none is otherwise selected.
pub const DEFAULT_PRODUCT_ID: &str = "default";

/// Registered **profile** manifests (device-class axis).
pub const PROFILES: &[ManifestEntry] = &[
    ManifestEntry { id: "desktop", toml: include_str!("../manifests/profiles/desktop/profile.toml") },
    ManifestEntry { id: "tablet", toml: include_str!("../manifests/profiles/tablet/profile.toml") },
];

/// Registered **shell** manifests (shell posture).
pub const SHELLS: &[ManifestEntry] = &[
    ManifestEntry { id: "desktop", toml: include_str!("../manifests/shells/desktop/shell.toml") },
    ManifestEntry { id: "tablet", toml: include_str!("../manifests/shells/tablet/shell.toml") },
    ManifestEntry { id: "kiosk", toml: include_str!("../manifests/shells/kiosk/shell.toml") },
];

/// Registered **product** manifests (deployment config = profile + shell + theme).
pub const PRODUCTS: &[ManifestEntry] = &[
    ManifestEntry { id: "default", toml: include_str!("../manifests/products/default/product.toml") },
    ManifestEntry { id: "tablet", toml: include_str!("../manifests/products/tablet/product.toml") },
    ManifestEntry { id: "kiosk", toml: include_str!("../manifests/products/kiosk/product.toml") },
];

fn lookup<'a>(catalog: &'a [ManifestEntry], id: &str) -> Result<&'a str> {
    catalog
        .iter()
        .find(|e| e.id == id)
        .map(|e| e.toml)
        .ok_or(SystemUiError::ManifestNotFound)
}

/// Parse a registered profile by id (deterministic `ManifestNotFound` for unknown).
pub fn profile_by_id(id: &str) -> Result<ProfileManifest> {
    parse_profile_manifest(lookup(PROFILES, id)?)
}

/// Parse a registered shell by id.
pub fn shell_by_id(id: &str) -> Result<ShellManifest> {
    parse_shell_manifest(lookup(SHELLS, id)?)
}

/// Parse a registered product by id.
pub fn product_by_id(id: &str) -> Result<ProductManifest> {
    parse_product_manifest(lookup(PRODUCTS, id)?)
}

/// The stable device environment the shell renders against — the deterministic
/// `device.*` surface from the profiles doc, derived from the resolved
/// profile + shell rather than baked into shell code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceEnvironment {
    /// `device.profile` — the resolved profile id.
    pub profile: String,
    /// `device.shellMode` — the active shell id/posture (changes on a switch).
    pub shell_mode: String,
    /// `device.shellKind` — the active shell's kind (desktop/tablet/kiosk/…).
    pub shell_kind: String,
    pub orientation: String,
    pub size_class: String,
    pub dpi_class: String,
    pub input: DeviceInput,
}

/// A fully resolved, validated shell configuration: the product that selected it,
/// the profile + shell manifests, and the derived device environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedConfig {
    pub product: ProductManifest,
    pub profile: ProfileManifest,
    pub shell: ShellManifest,
    pub env: DeviceEnvironment,
}

fn environment_for(profile: &ProfileManifest, shell: &ShellManifest) -> DeviceEnvironment {
    DeviceEnvironment {
        profile: profile.id.clone(),
        shell_mode: shell.id.clone(),
        shell_kind: shell.kind.clone(),
        orientation: profile.display_defaults.orientation.clone(),
        size_class: profile.display_defaults.size_class.clone(),
        dpi_class: profile.display_defaults.dpi_class.clone(),
        input: profile.input.clone(),
    }
}

/// Resolve a product id → profile + shell + validated pairing + device
/// environment. The single entry point the boot path calls ("which shell starts").
pub fn resolve_product(product_id: &str) -> Result<ResolvedConfig> {
    let product = product_by_id(product_id)?;
    let profile = profile_by_id(&product.profile)?;
    let shell = shell_by_id(&product.shell)?;
    validate_profile_shell(&profile, &shell)?;
    let env = environment_for(&profile, &shell);
    Ok(ResolvedConfig { product, profile, shell, env })
}

/// Resolve the default product (the boot default mode).
pub fn resolve_default() -> Result<ResolvedConfig> {
    resolve_product(DEFAULT_PRODUCT_ID)
}

/// The shell ids the current profile can switch between: its `allowed_shells`
/// that are registered AND declare support for the profile. This is what a shell
/// switcher (convertible toggle, settings) offers.
pub fn available_shells(cfg: &ResolvedConfig) -> Vec<String> {
    cfg.profile
        .allowed_shells
        .iter()
        .filter(|sid| {
            shell_by_id(sid)
                .map(|s| s.supported_profiles.iter().any(|p| *p == cfg.profile.id))
                .unwrap_or(false)
        })
        .cloned()
        .collect()
}

/// Switch the active shell within the same profile (e.g. desktop↔tablet on a
/// convertible). Keeps the product + profile, re-resolves the shell + environment.
/// Rejects a shell the profile does not allow / that does not support the profile,
/// or an unknown id — deterministically, leaving the caller's config untouched.
pub fn switch_shell(cfg: &ResolvedConfig, new_shell: &str) -> Result<ResolvedConfig> {
    if !cfg.profile.allowed_shells.iter().any(|s| s == new_shell) {
        return Err(SystemUiError::UnsupportedShell);
    }
    let shell = shell_by_id(new_shell)?;
    validate_profile_shell(&cfg.profile, &shell)?;
    let env = environment_for(&cfg.profile, &shell);
    Ok(ResolvedConfig {
        product: cfg.product.clone(),
        profile: cfg.profile.clone(),
        shell,
        env,
    })
}

/// A short, stable summary line for boot logs / markers.
pub fn summary(cfg: &ResolvedConfig) -> String {
    alloc::format!(
        "product={} profile={} shell={} kind={} {}x{}",
        cfg.product.id,
        cfg.env.profile,
        cfg.env.shell_mode,
        cfg.env.shell_kind,
        cfg.shell.first_frame.width,
        cfg.shell.first_frame.height,
    )
}

impl DeviceEnvironment {
    /// `true` when the active shell is a locked kiosk posture (no launcher/switch
    /// affordances should be offered).
    pub fn is_kiosk(&self) -> bool {
        self.shell_kind == "kiosk"
    }
}

/// The compact, infallible shell configuration the **compositor** (windowd)
/// consumes — the resolved shell's identity, posture-derived chrome, feature
/// flags and first-frame size flattened to plain values. windowd, which already
/// depends on this crate, gets the boot default via [`shell_config_default`] (no
/// IPC needed for the default mode); a later runtime switch hands a new one over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellConfig {
    pub product_id: String,
    pub profile_id: String,
    pub shell_id: String,
    pub shell_kind: String,
    /// Desktop-posture chrome (glass topbar + side panel). True for the `desktop`
    /// shell kind; tablet/kiosk get their own chrome when those renderers land.
    pub desktop_chrome: bool,
    pub launcher: bool,
    pub multiwindow: bool,
    pub quick_settings: bool,
    pub settings_entry: bool,
    /// Whether switch/launcher affordances must be suppressed (locked kiosk).
    pub locked: bool,
    pub width: u32,
    pub height: u32,
}

impl ShellConfig {
    /// Flatten a fully [`ResolvedConfig`] into the compositor-facing config.
    pub fn from_resolved(cfg: &ResolvedConfig) -> Self {
        Self {
            product_id: cfg.product.id.clone(),
            profile_id: cfg.profile.id.clone(),
            shell_id: cfg.shell.id.clone(),
            shell_kind: cfg.shell.kind.clone(),
            desktop_chrome: cfg.shell.kind == "desktop",
            launcher: cfg.shell.features.launcher,
            multiwindow: cfg.shell.features.multiwindow,
            quick_settings: cfg.shell.features.quick_settings,
            settings_entry: cfg.shell.features.settings_entry,
            locked: cfg.env.is_kiosk() || cfg.product.policy_preset == "locked-down",
            width: cfg.shell.first_frame.width,
            height: cfg.shell.first_frame.height,
        }
    }

    /// The hardcoded last-resort desktop config, used only if even the default
    /// product manifest fails to resolve — the compositor must always boot.
    pub fn desktop_fallback() -> Self {
        Self {
            product_id: String::from("default"),
            profile_id: String::from("desktop"),
            shell_id: String::from("desktop"),
            shell_kind: String::from("desktop"),
            desktop_chrome: true,
            launcher: false,
            multiwindow: false,
            quick_settings: false,
            settings_entry: false,
            locked: false,
            width: 1280,
            height: 800,
        }
    }
}

/// Resolve the boot default product into the compositor-facing [`ShellConfig`].
/// Infallible: falls back to [`ShellConfig::desktop_fallback`] if resolution fails
/// so windowd always has a valid shell to render.
pub fn shell_config_default() -> ShellConfig {
    match resolve_default() {
        Ok(cfg) => ShellConfig::from_resolved(&cfg),
        Err(_) => ShellConfig::desktop_fallback(),
    }
}

/// The next registered product id after `current`, wrapping around the [`PRODUCTS`]
/// catalog (e.g. default → tablet → kiosk → default). The basis of a shell
/// switcher / convertible toggle: resolve the returned id to apply the new shell.
/// Falls back to [`DEFAULT_PRODUCT_ID`] if `current` is unknown.
pub fn next_product_id(current: &str) -> &'static str {
    let n = PRODUCTS.len();
    if n == 0 {
        return DEFAULT_PRODUCT_ID;
    }
    match PRODUCTS.iter().position(|e| e.id == current) {
        Some(i) => PRODUCTS[(i + 1) % n].id,
        None => PRODUCTS[0].id,
    }
}

/// Resolve the product that follows `current` straight to a [`ShellConfig`]
/// (infallible — desktop fallback). The one call a switch trigger makes.
pub fn shell_config_next(current: &str) -> ShellConfig {
    match resolve_product(next_product_id(current)) {
        Ok(cfg) => ShellConfig::from_resolved(&cfg),
        Err(_) => ShellConfig::desktop_fallback(),
    }
}
