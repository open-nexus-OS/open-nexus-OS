// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Minimal TASK-0055 launcher client contract.
//! OWNERS: @ui
//! STATUS: Done
//! TEST_COVERAGE: `cargo test -p launcher -- --nocapture`

#![forbid(unsafe_code)]

use windowd::{
    CallerCtx, CommitSeq, Layer, PresentAck, Rect, SurfaceBuffer, WindowServer, WindowdConfig,
    WindowdError, LAUNCHER_MARKER,
};

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
}
