// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Headless smoke scenario used to gate marker emission on real present state.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No direct tests
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

use crate::buffer::{PixelFormat, SurfaceBuffer};
use crate::error::{Result, WindowdError};
use crate::frame::Layer;
use crate::geometry::{checked_len, checked_stride, Rect};
use crate::ids::{CallerCtx, CommitSeq};
use crate::server::{
    PresentAck, UiProfile, WindowServer, WindowdConfig, VISIBLE_BOOTSTRAP_FORMAT,
    VISIBLE_BOOTSTRAP_HEIGHT, VISIBLE_BOOTSTRAP_WIDTH,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiSmokeEvidence {
    pub ready: bool,
    pub systemui_loaded: bool,
    pub launcher_first_frame: bool,
    pub resize_ok: bool,
    pub first_present: PresentAck,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleBootstrapMode {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: PixelFormat,
}

impl VisibleBootstrapMode {
    pub fn fixed() -> Result<Self> {
        let stride = checked_stride(VISIBLE_BOOTSTRAP_WIDTH)?;
        Ok(Self {
            width: VISIBLE_BOOTSTRAP_WIDTH,
            height: VISIBLE_BOOTSTRAP_HEIGHT,
            stride,
            format: VISIBLE_BOOTSTRAP_FORMAT,
        })
    }

    pub fn validate(self) -> Result<Self> {
        if self.width != VISIBLE_BOOTSTRAP_WIDTH || self.height != VISIBLE_BOOTSTRAP_HEIGHT {
            return Err(WindowdError::InvalidDimensions);
        }
        if self.format != PixelFormat::Bgra8888 {
            return Err(WindowdError::UnsupportedFormat);
        }
        if self.stride != checked_stride(self.width)? {
            return Err(WindowdError::InvalidStride);
        }
        let _ = checked_len(self.stride, self.height)?;
        Ok(self)
    }

    pub fn byte_len(self) -> Result<usize> {
        checked_len(self.stride, self.height)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleDisplayCapability {
    pub byte_len: usize,
    pub mapped: bool,
    pub writable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleBootstrapEvidence {
    pub ready: bool,
    pub mode: VisibleBootstrapMode,
    pub first_present: PresentAck,
    pub seed_surface: SurfaceBuffer,
}

pub fn validate_visible_bootstrap_capability(
    mode: VisibleBootstrapMode,
    cap: VisibleDisplayCapability,
) -> Result<()> {
    let mode = mode.validate()?;
    if !cap.mapped || !cap.writable || cap.byte_len < mode.byte_len()? {
        return Err(WindowdError::InvalidDisplayCapability);
    }
    Ok(())
}

pub fn visible_marker_postflight_ready(
    evidence: Option<VisibleBootstrapEvidence>,
) -> Result<VisibleBootstrapEvidence> {
    evidence.ok_or(WindowdError::MarkerBeforePresentState)
}

pub fn bootstrap_pixel_bgra(x: u32, y: u32) -> [u8; 4] {
    let tile = ((x / 80) + (y / 80)) & 1;
    let (r, g, b) = if tile == 0 { (0x20, 0x80, 0xf0) } else { (0x10, 0x20, 0x40) };
    if (560..720).contains(&x) && (320..480).contains(&y) {
        [0x30, 0xe0, 0x60, 0xff]
    } else {
        [b, g, r, 0xff]
    }
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

pub fn run_visible_bootstrap_smoke() -> Result<VisibleBootstrapEvidence> {
    let mode = VisibleBootstrapMode::fixed()?.validate()?;
    let mut server = WindowServer::new(WindowdConfig::visible_bootstrap())?;
    server.load_systemui(UiProfile::Desktop)?;
    let launcher = CallerCtx::from_service_metadata(0x55);
    let mut surface = SurfaceBuffer::solid(launcher, 20, 64, 48, bootstrap_pixel_bgra(0, 0))?;
    for y in 0..surface.height {
        for x in 0..surface.width {
            let idx = (y as usize * surface.stride as usize) + (x as usize * 4);
            surface.pixels[idx..idx + 4].copy_from_slice(&bootstrap_pixel_bgra(x, y));
        }
    }
    let surface_id = server.create_surface(launcher, surface.clone())?;
    server.queue_buffer(launcher, surface_id, surface.clone(), &[Rect::new(0, 0, 64, 48)])?;
    server.commit_scene(
        CallerCtx::system(),
        CommitSeq::new(1),
        &[Layer { surface: surface_id, x: 8, y: 8, z: 0 }],
    )?;
    let first_present = server.present_bootstrap_scanout_tick()?;
    Ok(VisibleBootstrapEvidence {
        ready: server.initialized(),
        mode,
        first_present,
        seed_surface: surface,
    })
}
