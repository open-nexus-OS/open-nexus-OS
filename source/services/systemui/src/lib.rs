// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Modular SystemUI seed for profile-backed first-frame composition.
//! OWNERS: @ui
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p systemui -- --nocapture`
//!
//! PUBLIC API:
//!   - `compose_first_frame()`: builds the deterministic SystemUI seed frame.
//!   - `desktop_profile()` / `desktop_shell()`: parse the initial TOML manifests.
//!
//! DEPENDENCIES:
//!   - `alloc`: no_std-compatible strings and frame buffers.
//!
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

#![cfg_attr(all(nexus_env = "os", target_os = "none"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

mod frame;
mod profile;
mod shell;

pub use frame::{compose_first_frame, frame_checksum, FirstFrame};
pub use profile::{
    desktop_profile, parse_profile_manifest, validate_profile, DeviceInput, DisplayDefaults,
    ProfileManifest, SystemUiError,
};
pub use shell::{
    desktop_shell, parse_shell_manifest, resolve_desktop_shell, validate_profile_shell,
    validate_shell, FirstFrameSpec, ResolvedShell, ShellFeatures, ShellManifest,
};

pub fn help() -> &'static str {
    "systemui draws system chrome. Usage: systemui [--help] [--first-frame]"
}

pub fn execute(args: &[&str]) -> String {
    if args.contains(&"--help") {
        return help().to_string();
    }
    if args.contains(&"--first-frame") {
        return match compose_first_frame() {
            Ok(frame) => {
                let checksum = frame_checksum(&frame);
                alloc::format!(
                    "systemui first-frame profile=desktop shell=desktop {}x{} checksum={checksum}",
                    frame.width,
                    frame.height
                )
            }
            Err(_) => "systemui first-frame unavailable".to_string(),
        };
    }
    "systemui ready profile=desktop shell=desktop".to_string()
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

#[cfg(not(all(nexus_env = "os", target_os = "none")))]
pub fn run() {
    let owned: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    println!("{}", execute(&refs));
}

#[cfg(test)]
mod tests {
    use super::{
        checksum, compose_first_frame, desktop_profile, desktop_shell, execute,
        parse_profile_manifest, parse_shell_manifest, validate_profile_shell, SystemUiError,
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
        assert_eq!(frame.width, 160);
        assert_eq!(frame.height, 100);
        assert_eq!(frame.stride, 640);
        assert_eq!(pixel(&frame, 12, 20), [0x24, 0x28, 0x34, 0xff]);
        assert_eq!(pixel(&frame, 4, 4), [0x80, 0x50, 0x20, 0xff]);
        assert_eq!(pixel(&frame, 4, 30), [0x40, 0x28, 0x18, 0xff]);
        assert_eq!(pixel(&frame, 20, 30), [0x48, 0x80, 0x38, 0xff]);
        assert_eq!(checksum(), 1_999_217_024);
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
    fn test_reject_invalid_profile_and_shell_manifests() {
        let invalid_profile = r#"
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
        assert_eq!(
            parse_profile_manifest(invalid_profile).map(|_| ()),
            Err(SystemUiError::UnsupportedProfile)
        );

        let invalid_shell = r#"
id = "desktop"
kind = "desktop"
dsl_root = "ui/shells/desktop"
supported_profiles = ["tablet"]

[first_frame]
width = 160
height = 100

[features]
launcher = false
multiwindow = false
quick_settings = false
settings_entry = false
"#;
        assert_eq!(
            parse_shell_manifest(invalid_shell).map(|_| ()),
            Err(SystemUiError::IncompatibleShell)
        );
    }
}
