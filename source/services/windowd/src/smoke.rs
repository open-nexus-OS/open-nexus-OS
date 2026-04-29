// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Headless smoke scenario used to gate marker emission on real present state.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No direct tests
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

use crate::buffer::SurfaceBuffer;
use crate::error::{Result, WindowdError};
use crate::frame::Layer;
use crate::geometry::Rect;
use crate::ids::{CallerCtx, CommitSeq};
use crate::server::{PresentAck, UiProfile, WindowServer, WindowdConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiSmokeEvidence {
    pub ready: bool,
    pub systemui_loaded: bool,
    pub launcher_first_frame: bool,
    pub resize_ok: bool,
    pub first_present: PresentAck,
}

pub fn run_headless_ui_smoke() -> Result<UiSmokeEvidence> {
    let mut server = WindowServer::new(WindowdConfig::default())?;
    server.load_systemui(UiProfile::Desktop)?;
    let launcher = CallerCtx::from_service_metadata(0x55);
    let initial = SurfaceBuffer::solid(launcher, 10, 64, 48, [0x20, 0x80, 0xf0, 0xff])?;
    let surface = server.create_surface(launcher, initial.clone())?;
    server.queue_buffer(launcher, surface, initial, &[Rect::new(0, 0, 64, 48)])?;
    server.commit_scene(
        CallerCtx::system(),
        CommitSeq::new(1),
        &[Layer { surface, x: 8, y: 8, z: 0 }],
    )?;
    let first_present = match server.present_tick()? {
        Some(ack) => ack,
        None => return Err(WindowdError::MarkerBeforePresentState),
    };
    let resized = SurfaceBuffer::solid(launcher, 11, 96, 64, [0x30, 0xa0, 0x40, 0xff])?;
    server.resize_surface(launcher, surface, resized, &[Rect::new(0, 0, 96, 64)])?;
    server.commit_scene(
        CallerCtx::system(),
        CommitSeq::new(2),
        &[Layer { surface, x: 8, y: 8, z: 0 }],
    )?;
    if server.present_tick()?.is_none() {
        return Err(WindowdError::MarkerBeforePresentState);
    }
    Ok(UiSmokeEvidence {
        ready: server.initialized(),
        systemui_loaded: server.systemui_loaded(),
        launcher_first_frame: first_present.seq.raw() == 1 && first_present.damage_rects == 1,
        resize_ok: server.marker_evidence()?.seq.raw() == 2,
        first_present,
    })
}
