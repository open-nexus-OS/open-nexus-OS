// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::error::{Result, WindowdError};
use crate::server::PresentAck;
use alloc::format;
use alloc::string::String;

pub const READY_MARKER: &str = "windowd: ready (w=64, h=48, hz=60)";
pub const SYSTEMUI_MARKER: &str = "windowd: systemui loaded (profile=desktop)";
pub const LAUNCHER_MARKER: &str = "launcher: first frame ok";
pub const SELFTEST_LAUNCHER_PRESENT_MARKER: &str = "SELFTEST: ui launcher present ok";
pub const SELFTEST_RESIZE_MARKER: &str = "SELFTEST: ui resize ok";

pub fn present_marker(ack: PresentAck) -> String {
    format!("windowd: present ok (seq={} dmg={})", ack.seq.raw(), ack.damage_rects)
}

pub fn marker_postflight_ready(evidence: Option<PresentAck>) -> Result<PresentAck> {
    evidence.ok_or(WindowdError::MarkerBeforePresentState)
}
