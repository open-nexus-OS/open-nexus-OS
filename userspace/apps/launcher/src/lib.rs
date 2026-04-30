// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Minimal launcher client proofs for first-frame present and TASK-0056 v2a click routing.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 2 unit tests
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

#![forbid(unsafe_code)]

use windowd::{
    CallerCtx, CommitSeq, FrameIndex, InputEventKind, Layer, PresentAck, Rect, SurfaceBuffer,
    SurfaceId, WindowServer, WindowdConfig, WindowdError, LAUNCHER_CLICK_OK_MARKER,
    LAUNCHER_MARKER,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClickDemoEvidence {
    pub surface: SurfaceId,
    pub highlighted: bool,
    pub present: windowd::ScheduledPresentAck,
}

pub fn draw_first_frame() -> Result<PresentAck, WindowdError> {
    let mut server = WindowServer::new(WindowdConfig::default())?;
    let launcher = CallerCtx::from_service_metadata(0x55);
    let buffer = SurfaceBuffer::solid(launcher, 100, 32, 24, [0x20, 0x80, 0xf0, 0xff])?;
    let surface = server.create_surface(launcher, buffer.clone())?;
    server.queue_buffer(launcher, surface, buffer, &[Rect::new(0, 0, 32, 24)])?;
    server.commit_scene(
        CallerCtx::system(),
        CommitSeq::new(1),
        &[Layer { surface, x: 0, y: 0, z: 0 }],
    )?;
    server.present_tick()?.ok_or(WindowdError::MarkerBeforePresentState)
}

pub fn first_frame_marker(ack: Option<PresentAck>) -> Result<&'static str, WindowdError> {
    match ack {
        Some(_) => Ok(LAUNCHER_MARKER),
        None => Err(WindowdError::MarkerBeforePresentState),
    }
}

pub fn click_demo() -> Result<ClickDemoEvidence, WindowdError> {
    let mut server = WindowServer::new(WindowdConfig::default())?;
    let launcher = CallerCtx::from_service_metadata(0x55);
    let initial = SurfaceBuffer::solid(launcher, 200, 32, 24, [0x20, 0x80, 0xf0, 0xff])?;
    let surface = server.create_surface(launcher, initial.clone())?;
    server.queue_buffer(launcher, surface, initial, &[Rect::new(0, 0, 32, 24)])?;
    server.commit_scene(
        CallerCtx::system(),
        CommitSeq::new(1),
        &[Layer { surface, x: 0, y: 0, z: 0 }],
    )?;
    let highlighted = SurfaceBuffer::solid(launcher, 201, 32, 24, [0x30, 0xe0, 0x60, 0xff])?;
    server.acquire_back_buffer(launcher, surface, FrameIndex::new(1), highlighted)?;
    server.present_frame(launcher, surface, FrameIndex::new(1), &[Rect::new(0, 0, 32, 24)])?;
    let present = server.present_scheduler_tick()?.ok_or(WindowdError::MarkerBeforePresentState)?;
    let click = server.route_pointer_down(4, 4)?;
    let delivered = server.take_input_events(launcher, surface)?;
    let highlighted = click.surface == surface
        && delivered.iter().any(|event| event.kind == InputEventKind::PointerDown);
    if !highlighted {
        return Err(WindowdError::MarkerBeforePresentState);
    }
    Ok(ClickDemoEvidence { surface, highlighted, present })
}

pub fn click_marker(evidence: Option<&ClickDemoEvidence>) -> Result<&'static str, WindowdError> {
    match evidence {
        Some(evidence) if evidence.highlighted => Ok(LAUNCHER_CLICK_OK_MARKER),
        _ => Err(WindowdError::MarkerBeforePresentState),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_marker_requires_present_ack() {
        assert_eq!(first_frame_marker(None), Err(WindowdError::MarkerBeforePresentState));
        let ack = draw_first_frame().expect("first frame ack");
        assert_eq!(ack.seq.raw(), 1);
        assert_eq!(first_frame_marker(Some(ack)), Ok(LAUNCHER_MARKER));
    }

    #[test]
    fn launcher_click_marker_requires_routed_click_state() {
        assert_eq!(click_marker(None), Err(WindowdError::MarkerBeforePresentState));
        let evidence = click_demo().expect("click demo");
        assert_eq!(evidence.surface.raw(), 1);
        assert_eq!(click_marker(Some(&evidence)), Ok(LAUNCHER_CLICK_OK_MARKER));
    }
}
