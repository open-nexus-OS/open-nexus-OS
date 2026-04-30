// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Headless, visible, and v2a smoke scenarios used to gate markers on real present/input state.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by `windowd`, `ui_windowd_host`, and `ui_v2a_host` tests plus visible-bootstrap QEMU proof
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

use crate::buffer::{PixelFormat, SurfaceBuffer};
use crate::error::{Result, WindowdError};
use crate::frame::{Frame, Layer};
use crate::geometry::{checked_len, checked_stride, Rect};
use crate::ids::{CallerCtx, CommitSeq, FrameIndex, SurfaceId};
use crate::server::{
    InputEventKind, PresentAck, PresentFenceStatus, ScheduledPresentAck, UiProfile, WindowServer,
    WindowdConfig, VISIBLE_BOOTSTRAP_FORMAT, VISIBLE_BOOTSTRAP_HEIGHT, VISIBLE_BOOTSTRAP_WIDTH,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleSystemUiEvidence {
    pub ready: bool,
    pub backend_visible: bool,
    pub systemui_first_frame: bool,
    pub mode: VisibleBootstrapMode,
    pub first_present: PresentAck,
    pub frame_source: SurfaceBuffer,
    pub composed_frame: Option<Frame>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiV2aEvidence {
    pub present_scheduler_on: bool,
    pub input_on: bool,
    pub focused_surface: SurfaceId,
    pub launcher_click_ok: bool,
    pub scheduled_present: ScheduledPresentAck,
    pub latest_fence: PresentFenceStatus,
}

impl VisibleSystemUiEvidence {
    pub fn copy_composed_row(&self, y: u32, row: &mut [u8]) -> Result<()> {
        let row_len = self.mode.stride as usize;
        if row.len() < row_len {
            return Err(WindowdError::BufferLengthMismatch);
        }
        row[..row_len].fill(0);
        if let Some(frame) = &self.composed_frame {
            if frame.width != self.mode.width
                || frame.height != self.mode.height
                || frame.stride != self.mode.stride
            {
                return Err(WindowdError::InvalidDimensions);
            }
            let src = y as usize * frame.stride as usize;
            row[..row_len].copy_from_slice(
                frame.pixels.get(src..src + row_len).ok_or(WindowdError::BufferLengthMismatch)?,
            );
            return Ok(());
        }
        if y < self.frame_source.height {
            let source_row_len = self.frame_source.width as usize * 4;
            let src = y as usize * self.frame_source.stride as usize;
            row[..source_row_len].copy_from_slice(
                self.frame_source
                    .pixels
                    .get(src..src + source_row_len)
                    .ok_or(WindowdError::BufferLengthMismatch)?,
            );
        }
        Ok(())
    }
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

pub fn visible_systemui_marker_postflight_ready(
    evidence: Option<VisibleSystemUiEvidence>,
) -> Result<VisibleSystemUiEvidence> {
    evidence.ok_or(WindowdError::MarkerBeforePresentState)
}

pub fn v2a_marker_postflight_ready(evidence: Option<UiV2aEvidence>) -> Result<UiV2aEvidence> {
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

pub fn run_visible_systemui_smoke() -> Result<VisibleSystemUiEvidence> {
    let mode = VisibleBootstrapMode::fixed()?.validate()?;
    let mut server = WindowServer::new(WindowdConfig::visible_bootstrap())?;
    server.load_systemui(UiProfile::Desktop)?;
    let systemui = CallerCtx::from_service_metadata(0x57);
    let first_frame =
        systemui::compose_first_frame().map_err(|_| WindowdError::InvalidDimensions)?;
    let surface = SurfaceBuffer::from_bgra_pixels(
        systemui,
        30,
        first_frame.width,
        first_frame.height,
        first_frame.pixels,
    )?;
    let surface_id = server.create_surface(systemui, surface.clone())?;
    server.queue_buffer(
        systemui,
        surface_id,
        surface.clone(),
        &[Rect::new(0, 0, surface.width, surface.height)],
    )?;
    server.commit_scene(
        CallerCtx::system(),
        CommitSeq::new(1),
        &[Layer { surface: surface_id, x: 0, y: 0, z: 0 }],
    )?;
    let first_present = server.present_bootstrap_scanout_tick()?;
    let composed_frame = server.last_frame().cloned();
    #[cfg(not(all(nexus_env = "os", target_os = "none")))]
    let systemui_first_frame = composed_frame
        .as_ref()
        .map(|frame| frame.pixels.get(0..4) == surface.pixels.get(0..4))
        .unwrap_or(false);
    #[cfg(all(nexus_env = "os", target_os = "none"))]
    let systemui_first_frame =
        server.systemui_loaded() && first_present.seq.raw() == 1 && first_present.damage_rects > 0;
    Ok(VisibleSystemUiEvidence {
        ready: server.initialized(),
        backend_visible: mode.width == VISIBLE_BOOTSTRAP_WIDTH
            && mode.height == VISIBLE_BOOTSTRAP_HEIGHT,
        systemui_first_frame: server.systemui_loaded() && systemui_first_frame,
        mode,
        first_present,
        frame_source: surface,
        composed_frame,
    })
}

pub fn run_ui_v2a_smoke() -> Result<UiV2aEvidence> {
    let mut server = WindowServer::new(WindowdConfig::default())?;
    server.load_systemui(UiProfile::Desktop)?;
    let launcher = CallerCtx::from_service_metadata(0x55);
    let background = CallerCtx::from_service_metadata(0x57);
    let launcher_initial = SurfaceBuffer::solid(launcher, 40, 8, 8, [0x20, 0x80, 0xf0, 0xff])?;
    let background_initial =
        SurfaceBuffer::solid(background, 41, 16, 16, [0x10, 0x20, 0x40, 0xff])?;
    let launcher_surface = server.create_surface(launcher, launcher_initial.clone())?;
    let background_surface = server.create_surface(background, background_initial.clone())?;
    server.queue_buffer(launcher, launcher_surface, launcher_initial, &[Rect::new(0, 0, 8, 8)])?;
    server.queue_buffer(
        background,
        background_surface,
        background_initial,
        &[Rect::new(0, 0, 16, 16)],
    )?;
    server.commit_scene(
        CallerCtx::system(),
        CommitSeq::new(1),
        &[
            Layer { surface: background_surface, x: 0, y: 0, z: 0 },
            Layer { surface: launcher_surface, x: 2, y: 2, z: 10 },
        ],
    )?;

    let frame_one = SurfaceBuffer::solid(launcher, 42, 8, 8, [0x30, 0x90, 0xf0, 0xff])?;
    let frame_two = SurfaceBuffer::solid(launcher, 43, 8, 8, [0x60, 0xb0, 0x20, 0xff])?;
    server.acquire_back_buffer(launcher, launcher_surface, FrameIndex::new(1), frame_one)?;
    let first_fence = server.present_frame(
        launcher,
        launcher_surface,
        FrameIndex::new(1),
        &[Rect::new(0, 0, 8, 8)],
    )?;
    server.acquire_back_buffer(launcher, launcher_surface, FrameIndex::new(2), frame_two)?;
    let latest_fence_ack = server.present_frame(
        launcher,
        launcher_surface,
        FrameIndex::new(2),
        &[Rect::new(2, 2, 4, 4)],
    )?;
    let scheduled_present =
        server.present_scheduler_tick()?.ok_or(WindowdError::MarkerBeforePresentState)?;
    let coalesced_status = server.present_fence_status(first_fence.fence_id)?;
    let latest_fence = server.present_fence_status(latest_fence_ack.fence_id)?;
    if !coalesced_status.signaled || !coalesced_status.coalesced || !latest_fence.signaled {
        return Err(WindowdError::FenceNotReady);
    }

    let pointer = server.route_pointer_down(3, 3)?;
    let keyboard = server.route_keyboard(0x20)?;
    let delivered = server.take_input_events(launcher, launcher_surface)?;
    let launcher_click_ok = pointer.surface == launcher_surface
        && keyboard.surface == launcher_surface
        && delivered.iter().any(|event| event.kind == InputEventKind::PointerDown)
        && delivered.iter().any(|event| matches!(event.kind, InputEventKind::Keyboard { .. }));
    let focused_surface = server.focused_surface().ok_or(WindowdError::NoFocusedSurface)?;
    Ok(UiV2aEvidence {
        present_scheduler_on: server.scheduler_enabled()
            && scheduled_present.frames_coalesced == 1
            && scheduled_present.fences_signaled == 2,
        input_on: server.input_enabled(),
        focused_surface,
        launcher_click_ok,
        scheduled_present,
        latest_fence,
    })
}
