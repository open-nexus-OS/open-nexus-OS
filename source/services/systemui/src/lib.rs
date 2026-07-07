// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: SystemUI = the **declarative shell configuration resolver**. It parses
//! profile/shell/product TOML manifests from a registry, resolves a product into a
//! profile + shell + `DeviceEnvironment`, and flattens that to a `ShellConfig` the
//! compositor (windowd) consumes — "the shell is set in SystemUI, not hardcoded".
//! Also keeps the deterministic first-frame seed. Today windowd consumes this as a
//! LIBRARY (resolver in-process); booting SystemUI as a service is deferred (it
//! stalled the init handoff — see ADR-0035).
//! OWNERS: @ui
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p systemui -- --nocapture`
//!
//! PUBLIC API:
//!   - `resolve_default()` / `resolve_product(id)` → `ResolvedConfig`; `shell_config_default()` /
//!     `shell_config_next(current)` → `ShellConfig` (the compositor-facing config).
//!   - `available_shells()` / `switch_shell()` / `next_product_id()` — runtime shell switching.
//!   - `compose_first_frame()` — the deterministic SystemUI seed frame.
//!   - `service_boot()` (os-lite, dormant) — the would-be boot-service entry.
//!
//! ADR: docs/adr/0035-systemui-declarative-shell-configuration.md (this crate's architecture),
//!      docs/adr/0028 (windowd present / visible-bootstrap).
//! SPEC: docs/dev/ui/foundations/layout/profiles.md

#![cfg_attr(all(nexus_env = "os", target_os = "none"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

mod frame;
mod greeter;
mod ime_overlay;
mod product;
mod profile;
mod registry;
mod shell;

pub use frame::{
    compose_first_frame, frame_checksum, wallpaper_bgra, wallpaper_decoded_size,
    wallpaper_source_is_jpeg, FirstFrame,
};
pub use greeter::{greeter_config, parse_greeter_manifest, validate_greeter, GreeterConfig};
pub use ime_overlay::ImeOverlayState;
pub use product::{parse_product_manifest, validate_product, ProductManifest, SessionMode};
pub use profile::{
    desktop_profile, parse_profile_manifest, validate_profile, DeviceInput, DisplayDefaults,
    ProfileManifest, SystemUiError, KNOWN_DPI_CLASSES, KNOWN_ORIENTATIONS, KNOWN_SIZE_CLASSES,
};
pub use registry::{
    available_shells, next_product_id, product_by_id, profile_by_id, resolve_default,
    resolve_product, shell_by_id, shell_config_default, shell_config_next, summary, switch_shell,
    DeviceEnvironment, ResolvedConfig, ShellConfig, DEFAULT_PRODUCT_ID, PRODUCTS, PROFILES, SHELLS,
};
pub use shell::{
    desktop_shell, parse_shell_manifest, resolve_desktop_shell, validate_profile_shell,
    validate_shell, FirstFrameSpec, ResolvedShell, ShellFeatures, ShellManifest, KNOWN_SHELL_KINDS,
};

pub fn help() -> &'static str {
    "systemui draws system chrome. Usage: systemui [--help] [--first-frame]"
}

pub fn execute(args: &[&str]) -> String {
    if args.contains(&"--help") {
        return help().to_string();
    }
    // Resolve the active shell configuration from the manifest registry (the
    // declarative "which shell starts" — default product unless overridden).
    let cfg = registry::resolve_default();
    if args.contains(&"--first-frame") {
        return match (compose_first_frame(), &cfg) {
            (Ok(frame), Ok(c)) => {
                alloc::format!(
                    "systemui first-frame {} checksum={}",
                    registry::summary(c),
                    frame_checksum(&frame)
                )
            }
            _ => "systemui first-frame unavailable".to_string(),
        };
    }
    match &cfg {
        Ok(c) => alloc::format!("systemui ready {}", registry::summary(c)),
        Err(_) => "systemui config error".to_string(),
    }
}

pub fn compose_frame() -> Vec<u32> {
    match compose_first_frame() {
        Ok(frame) => frame
            .pixels
            .chunks_exact(4)
            .map(|pixel| u32::from_le_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]))
            .collect(),
        Err(_) => Vec::new(),
    }
}

pub fn checksum() -> u32 {
    match compose_first_frame() {
        Ok(frame) => frame_checksum(&frame),
        Err(_) => 0,
    }
}

/// OS service entry: resolve the boot **default product** from the manifest
/// registry and log the resolved shell configuration. One-shot for now (returns
/// `Ok` → `exit(0)`); SystemUI does not yet own a long-lived loop — the windowd
/// handoff + runtime shell switch are Phase D. Resolution is infallible (desktop
/// fallback), so booting SystemUI never faults the boot chain.
#[cfg(all(feature = "os-lite", nexus_env = "os"))]
pub fn service_boot() -> core::result::Result<(), SystemUiError> {
    let sc = registry::shell_config_default();
    let _ = nexus_abi::debug_println(&alloc::format!(
        "systemui: shell resolved product={} profile={} shell={} kind={} chrome={} locked={}",
        sc.product_id, sc.profile_id, sc.shell_id, sc.shell_kind, sc.desktop_chrome, sc.locked,
    ));
    Ok(())
}

#[cfg(not(all(nexus_env = "os", target_os = "none")))]
pub fn run() {
    let owned: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    println!("{}", execute(&refs));
}

#[cfg(test)]
mod tests {
    use super::{
        available_shells, checksum, compose_first_frame, desktop_profile, desktop_shell, execute,
        parse_profile_manifest, parse_shell_manifest, resolve_default, resolve_product, switch_shell,
        validate_profile_shell, wallpaper_decoded_size, wallpaper_source_is_jpeg, ImeOverlayState,
        SystemUiError,
    };

    fn pixel(frame: &super::FirstFrame, x: u32, y: u32) -> [u8; 4] {
        let idx = (y as usize * frame.stride as usize) + (x as usize * 4);
        [frame.pixels[idx], frame.pixels[idx + 1], frame.pixels[idx + 2], frame.pixels[idx + 3]]
    }

    #[test]
    fn help_flag() {
        assert!(execute(&["--help"]).contains("systemui"));
    }

    #[test]
    fn first_frame_is_deterministic_desktop_shell() {
        let frame = compose_first_frame().expect("first frame");
        assert_eq!(frame.width, 1280);
        assert_eq!(frame.height, 800);
        assert_eq!(frame.stride, 5120);
        assert!(wallpaper_source_is_jpeg());
        assert_eq!(wallpaper_decoded_size(), (1280, 800));
        assert_ne!(pixel(&frame, 12, 20), [0x24, 0x28, 0x34, 0xff]);
        assert_ne!(pixel(&frame, 4, 4), [0x80, 0x50, 0x20, 0xff]);
        assert_ne!(pixel(&frame, 4, 30), [0x40, 0x28, 0x18, 0xff]);
        assert_ne!(pixel(&frame, 20, 30), [0x48, 0x80, 0x38, 0xff]);
        assert_ne!(checksum(), 0);
    }

    #[test]
    fn profile_and_shell_manifests_are_compatible() {
        let profile = desktop_profile().expect("desktop profile");
        let shell = desktop_shell().expect("desktop shell");
        assert_eq!(profile.id, "desktop");
        assert_eq!(shell.id, "desktop");
        assert_eq!(validate_profile_shell(&profile, &shell), Ok(()));
    }

    #[test]
    fn desktop_profile_exposes_visible_qemu_input_capabilities() {
        let profile = desktop_profile().expect("desktop profile");

        assert!(
            profile.input.touch,
            "desktop visible profile keeps tablet/touch enabled for the mixed live lane"
        );
        assert!(profile.input.mouse, "desktop visible profile must keep mouse input on");
        assert!(profile.input.kbd, "desktop visible profile must keep keyboard input on");
    }

    #[test]
    fn generic_validation_accepts_well_formed_non_desktop_manifests() {
        // A phone profile is now structurally VALID (no hardcoded desktop-only
        // check); rejection of unknown ids happens at registry resolution instead.
        let phone_profile = r#"
id = "phone"
label = "Phone"
default_shell = "phone"
allowed_shells = ["phone"]

[input]
touch = true
mouse = false
kbd = false
remote = false
rotary = false

[display_defaults]
orientation = "portrait"
dpi_class = "high"
size_class = "compact"
"#;
        assert!(parse_profile_manifest(phone_profile).is_ok());
    }

    #[test]
    fn validation_rejects_bad_value_domains_and_pairings() {
        // Unknown display value → deterministic InvalidManifest.
        let bad_orientation = r#"
id = "tablet"
label = "Tablet"
default_shell = "tablet"
allowed_shells = ["tablet"]

[input]
touch = true
mouse = false
kbd = false
remote = false
rotary = false

[display_defaults]
orientation = "sideways"
dpi_class = "high"
size_class = "regular"
"#;
        assert_eq!(
            parse_profile_manifest(bad_orientation).map(|_| ()),
            Err(SystemUiError::InvalidManifest)
        );

        // default_shell not in allowed_shells → UnsupportedShell.
        let bad_default = r#"
id = "tablet"
label = "Tablet"
default_shell = "desktop"
allowed_shells = ["tablet"]

[input]
touch = true
mouse = false
kbd = false
remote = false
rotary = false

[display_defaults]
orientation = "portrait"
dpi_class = "high"
size_class = "regular"
"#;
        assert_eq!(
            parse_profile_manifest(bad_default).map(|_| ()),
            Err(SystemUiError::UnsupportedShell)
        );

        // Unknown shell kind → UnsupportedShell.
        let bad_kind = r#"
id = "weird"
kind = "hologram"
dsl_root = "ui/shells/weird"
supported_profiles = ["desktop"]

[first_frame]
width = 100
height = 100

[features]
launcher = false
multiwindow = false
quick_settings = false
settings_entry = false
"#;
        assert_eq!(
            parse_shell_manifest(bad_kind).map(|_| ()),
            Err(SystemUiError::UnsupportedShell)
        );

        // Incompatible profile↔shell pairing is caught by validate_profile_shell:
        // a desktop shell that supports only tablet, paired with the desktop profile.
        let tablet_only_desktop_shell = r#"
id = "desktop"
kind = "desktop"
dsl_root = "ui/shells/desktop"
supported_profiles = ["tablet"]

[first_frame]
width = 1280
height = 800

[features]
launcher = false
multiwindow = false
quick_settings = false
settings_entry = false
"#;
        let shell = parse_shell_manifest(tablet_only_desktop_shell).expect("structurally valid");
        let profile = desktop_profile().expect("desktop profile");
        assert_eq!(
            validate_profile_shell(&profile, &shell),
            Err(SystemUiError::IncompatibleShell)
        );
    }

    #[test]
    fn default_product_resolves_to_desktop() {
        let cfg = resolve_default().expect("default product resolves");
        assert_eq!(cfg.product.id, "default");
        assert_eq!(cfg.profile.id, "desktop");
        assert_eq!(cfg.shell.id, "desktop");
        assert_eq!(cfg.env.profile, "desktop");
        assert_eq!(cfg.env.shell_mode, "desktop");
        assert_eq!(cfg.env.shell_kind, "desktop");
        assert!(!cfg.env.is_kiosk());
    }

    #[test]
    fn tablet_and_kiosk_products_resolve() {
        let tablet = resolve_product("tablet").expect("tablet product");
        assert_eq!(tablet.profile.id, "tablet");
        assert_eq!(tablet.shell.id, "tablet");
        assert_eq!(tablet.env.orientation, "portrait");
        assert!(tablet.env.input.touch && !tablet.env.input.mouse);

        let kiosk = resolve_product("kiosk").expect("kiosk product");
        assert_eq!(kiosk.shell.id, "kiosk");
        assert_eq!(kiosk.shell.kind, "kiosk");
        assert!(kiosk.env.is_kiosk());
        assert_eq!(kiosk.product.policy_preset, "locked-down");
        assert_eq!(kiosk.product.deployment, "warehouse-floor");
    }

    #[test]
    fn unknown_product_is_deterministic_not_found() {
        assert_eq!(resolve_product("does-not-exist"), Err(SystemUiError::ManifestNotFound));
    }

    #[test]
    fn shell_config_default_is_desktop_chrome() {
        // The compositor-facing config for the boot default: desktop posture →
        // desktop chrome on, not locked, 1280x800.
        let sc = super::shell_config_default();
        assert_eq!(sc.shell_kind, "desktop");
        assert!(sc.desktop_chrome);
        assert!(!sc.locked);
        assert_eq!((sc.width, sc.height), (1280, 800));

        // A kiosk product flattens to a locked, non-desktop-chrome config.
        let kiosk = super::ShellConfig::from_resolved(&resolve_product("kiosk").unwrap());
        assert_eq!(kiosk.shell_kind, "kiosk");
        assert!(!kiosk.desktop_chrome);
        assert!(kiosk.locked);

        // The hardcoded fallback is always a valid desktop config.
        let fb = super::ShellConfig::desktop_fallback();
        assert!(fb.desktop_chrome && !fb.locked);
    }

    #[test]
    fn shell_switch_cycles_products_and_wraps() {
        // default → tablet → kiosk → default (the registered PRODUCTS order).
        assert_eq!(super::next_product_id("default"), "tablet");
        assert_eq!(super::next_product_id("tablet"), "kiosk");
        assert_eq!(super::next_product_id("kiosk"), "default");
        // Unknown current → first product.
        assert_eq!(super::next_product_id("bogus"), "default");

        // shell_config_next resolves the following product end-to-end.
        let after_default = super::shell_config_next("default");
        assert_eq!(after_default.product_id, "tablet");
        assert_eq!(after_default.shell_kind, "tablet");
        assert!(!after_default.desktop_chrome);
    }

    #[test]
    fn convertible_can_switch_shell_but_pure_desktop_cannot() {
        // Tablet/convertible: offers tablet + desktop + kiosk (all support tablet).
        let tablet = resolve_product("tablet").expect("tablet product");
        let shells = available_shells(&tablet);
        assert!(shells.contains(&"tablet".to_string()));
        assert!(shells.contains(&"desktop".to_string()));
        assert!(shells.contains(&"kiosk".to_string()));

        // Switch to the desktop posture (the convertible toggle).
        let switched = switch_shell(&tablet, "desktop").expect("switch to desktop");
        assert_eq!(switched.env.shell_mode, "desktop");
        assert_eq!(switched.env.shell_kind, "desktop");
        assert_eq!(switched.profile.id, "tablet"); // same device, different posture

        // A disallowed shell id is rejected deterministically.
        assert_eq!(switch_shell(&tablet, "nope"), Err(SystemUiError::UnsupportedShell));

        // Pure desktop profile allows only the desktop shell → cannot switch.
        let desktop = resolve_default().expect("default");
        assert_eq!(available_shells(&desktop), vec!["desktop".to_string()]);
        assert_eq!(switch_shell(&desktop, "tablet"), Err(SystemUiError::UnsupportedShell));
    }

    #[test]
    fn ime_overlay_tracks_show_hide_edges() {
        let mut overlay = ImeOverlayState::new();
        assert!(!overlay.visible());
        assert!(overlay.show());
        assert!(overlay.visible());
        assert!(!overlay.show());
        assert!(overlay.hide());
        assert!(!overlay.visible());
        assert_eq!(overlay.show_events(), 1);
        assert_eq!(overlay.hide_events(), 1);
    }
}
