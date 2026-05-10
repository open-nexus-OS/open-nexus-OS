// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Bounded IME visibility hook seam and host CLI shim for TASK-0253.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 2 unit tests.
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

#![cfg_attr(all(nexus_env = "os", target_os = "none"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::string::{String, ToString};
#[cfg(not(all(nexus_env = "os", target_os = "none")))]
use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImeVisibility {
    Hidden,
    Visible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImeService {
    visibility: ImeVisibility,
}

impl ImeService {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            visibility: ImeVisibility::Hidden,
        }
    }

    pub fn show(&mut self) -> bool {
        if self.visibility == ImeVisibility::Visible {
            return false;
        }
        self.visibility = ImeVisibility::Visible;
        true
    }

    pub fn hide(&mut self) -> bool {
        if self.visibility == ImeVisibility::Hidden {
            return false;
        }
        self.visibility = ImeVisibility::Hidden;
        true
    }

    #[must_use]
    pub const fn visibility(self) -> ImeVisibility {
        self.visibility
    }

    #[must_use]
    pub const fn visible(self) -> bool {
        matches!(self.visibility, ImeVisibility::Visible)
    }
}

impl Default for ImeService {
    fn default() -> Self {
        Self::new()
    }
}

pub fn help() -> &'static str {
    "ime provides input methods. Usage: ime [--help] [--show|--hide] [text]"
}

pub fn transform(input: &str) -> String {
    input.to_uppercase()
}

pub fn execute(args: &[&str]) -> String {
    if args.contains(&"--help") {
        return help().to_string();
    }
    let mut service = ImeService::new();
    if args.contains(&"--show") {
        service.show();
        return "ime: visible".to_string();
    }
    if args.contains(&"--hide") {
        service.hide();
        return "ime: hidden".to_string();
    }
    if let Some(text) = args.first() {
        return transform(text);
    }
    "ime awaiting text".to_string()
}

#[cfg(not(all(nexus_env = "os", target_os = "none")))]
pub fn run() {
    let owned: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    println!("{}", execute(&refs));
}

#[cfg(test)]
mod tests {
    use super::{execute, transform, ImeService, ImeVisibility};

    #[test]
    fn uppercase_conversion() {
        assert_eq!(transform("abc"), "ABC");
        assert_eq!(execute(&["abc"]), "ABC");
    }

    #[test]
    fn show_hide_hooks_are_idempotent() {
        let mut service = ImeService::new();
        assert_eq!(service.visibility(), ImeVisibility::Hidden);
        assert!(service.show());
        assert!(service.visible());
        assert!(!service.show());
        assert!(service.hide());
        assert_eq!(service.visibility(), ImeVisibility::Hidden);
        assert!(!service.hide());
    }
}
