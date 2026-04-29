// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: CLI adapter for deterministic `windowd` help/execute/run paths.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No direct tests
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::markers::{
    present_marker, LAUNCHER_MARKER, READY_MARKER, SELFTEST_LAUNCHER_PRESENT_MARKER,
    SELFTEST_RESIZE_MARKER, SYSTEMUI_MARKER,
};
use crate::run_headless_ui_smoke;

pub fn help() -> &'static str {
    "windowd headless compositor. Usage: windowd [--help]"
}

pub fn execute(args: &[&str]) -> Vec<String> {
    if args.contains(&"--help") {
        vec![String::from(help())]
    } else {
        match run_headless_ui_smoke() {
            Ok(evidence)
                if evidence.ready
                    && evidence.systemui_loaded
                    && evidence.launcher_first_frame
                    && evidence.resize_ok =>
            {
                vec![
                    String::from(READY_MARKER),
                    String::from(SYSTEMUI_MARKER),
                    present_marker(evidence.first_present),
                    String::from(LAUNCHER_MARKER),
                    String::from(SELFTEST_LAUNCHER_PRESENT_MARKER),
                    String::from(SELFTEST_RESIZE_MARKER),
                ]
            }
            _ => vec![String::from("windowd: headless present failed")],
        }
    }
}

#[cfg(not(all(nexus_env = "os", target_os = "none")))]
pub fn run() {
    let owned: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    for line in execute(&refs) {
        println!("{line}");
    }
}
