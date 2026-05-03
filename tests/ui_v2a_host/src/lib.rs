// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0056/TASK-0056B host behavior proofs for v2a present scheduling and visible input.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Stable for TASK-0056/TASK-0056B proof floor
//! TEST_COVERAGE: 19 host tests including 12 reject-filtered tests
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

mod surface_capnp {
    include!(concat!(env!("OUT_DIR"), "/surface_capnp.rs"));
}

mod vsync_capnp {
    include!(concat!(env!("OUT_DIR"), "/vsync_capnp.rs"));
}

mod input_capnp {
    include!(concat!(env!("OUT_DIR"), "/input_capnp.rs"));
}

#[cfg(test)]
mod tests {
    use crate::{input_capnp, surface_capnp, vsync_capnp};
    use windowd::{
        focus_marker, v2a_marker_postflight_ready, CallerCtx, CommitSeq, FrameIndex,
        InputEventKind, Layer, Rect, SurfaceBuffer, SurfaceId, WindowServer, WindowdConfig,
        WindowdError, CURSOR_MOVE_VISIBLE_MARKER, FOCUS_VISIBLE_MARKER, HOVER_VISIBLE_MARKER,
        INPUT_ON_MARKER, INPUT_VISIBLE_ON_MARKER, LAUNCHER_CLICK_OK_MARKER,
        LAUNCHER_CLICK_VISIBLE_OK_MARKER, PRESENT_SCHEDULER_ON_MARKER,
        SELFTEST_UI_V2_INPUT_OK_MARKER, SELFTEST_UI_V2_PRESENT_OK_MARKER,
        SELFTEST_UI_VISIBLE_INPUT_OK_MARKER, VISIBLE_CURSOR_BGRA, VISIBLE_FOCUS_BGRA,
        VISIBLE_HOVER_BGRA, VISIBLE_INPUT_CLICK_BGRA,
    };

    const LAUNCHER: CallerCtx = CallerCtx::from_service_metadata(0x55);
    const OTHER: CallerCtx = CallerCtx::from_service_metadata(0x66);

    fn server() -> WindowServer {
        match WindowServer::new(WindowdConfig::default()) {
            Ok(server) => server,
            Err(err) => panic!("server init failed: {err:?}"),
        }
    }

    fn buffer(handle: u64, width: u32, height: u32, color: [u8; 4]) -> SurfaceBuffer {
        match SurfaceBuffer::solid(LAUNCHER, handle, width, height, color) {
            Ok(buffer) => buffer,
            Err(err) => panic!("buffer build failed: {err:?}"),
        }
    }

    fn other_buffer(handle: u64, width: u32, height: u32, color: [u8; 4]) -> SurfaceBuffer {
        match SurfaceBuffer::solid(OTHER, handle, width, height, color) {
            Ok(buffer) => buffer,
            Err(err) => panic!("other buffer build failed: {err:?}"),
        }
    }

    fn pixel(frame: &windowd::Frame, x: u32, y: u32) -> [u8; 4] {
        let idx = (y as usize * frame.stride as usize) + (x as usize * 4);
        [frame.pixels[idx], frame.pixels[idx + 1], frame.pixels[idx + 2], frame.pixels[idx + 3]]
    }

    fn surface_with_scene(srv: &mut WindowServer, width: u32, height: u32) -> SurfaceId {
        let initial = buffer(1, width, height, [0x10, 0x20, 0x30, 0xff]);
        let surface = match srv.create_surface(LAUNCHER, initial.clone()) {
            Ok(id) => id,
            Err(err) => panic!("surface create failed: {err:?}"),
        };
        assert_eq!(
            srv.commit_scene(
                CallerCtx::system(),
                CommitSeq::new(1),
                &[Layer { surface, x: 0, y: 0, z: 0 }],
            ),
            Ok(())
        );
        surface
    }

    #[test]
    fn rapid_submits_coalesce_and_signal_fences_after_tick() {
        let mut srv = server();
        let surface = surface_with_scene(&mut srv, 4, 4);
        let first = buffer(10, 4, 4, [0, 0, 0xff, 0xff]);
        let latest = buffer(11, 4, 4, [0xff, 0, 0, 0xff]);

        assert_eq!(
            srv.acquire_back_buffer(LAUNCHER, surface, FrameIndex::new(1), first)
                .map(|lease| { (lease.surface.raw(), lease.frame_index.raw()) }),
            Ok((surface.raw(), 1))
        );
        let first_ack = match srv.present_frame(
            LAUNCHER,
            surface,
            FrameIndex::new(1),
            &[Rect::new(0, 0, 4, 4)],
        ) {
            Ok(ack) => ack,
            Err(err) => panic!("first present failed: {err:?}"),
        };
        assert_eq!(
            srv.present_fence_status(first_ack.fence_id).map(|status| status.signaled),
            Ok(false)
        );
        assert_eq!(
            srv.acquire_back_buffer(LAUNCHER, surface, FrameIndex::new(2), latest)
                .map(|lease| { (lease.surface.raw(), lease.frame_index.raw()) }),
            Ok((surface.raw(), 2))
        );
        let latest_ack = match srv.present_frame(
            LAUNCHER,
            surface,
            FrameIndex::new(2),
            &[Rect::new(1, 1, 2, 2)],
        ) {
            Ok(ack) => ack,
            Err(err) => panic!("latest present failed: {err:?}"),
        };

        let scheduled = match srv.present_scheduler_tick() {
            Ok(Some(ack)) => ack,
            other => panic!("expected scheduled present, got {other:?}"),
        };
        assert_eq!(scheduled.seq.raw(), 1);
        assert_eq!(scheduled.damage_rects, 2);
        assert_eq!(scheduled.frames_coalesced, 1);
        assert_eq!(scheduled.fences_signaled, 2);
        assert_eq!(pixel(srv.last_frame().expect("frame"), 0, 0), [0xff, 0, 0, 0xff]);

        let first_status = srv.present_fence_status(first_ack.fence_id).expect("first fence");
        let latest_status = srv.present_fence_status(latest_ack.fence_id).expect("latest fence");
        assert!(first_status.signaled);
        assert!(first_status.coalesced);
        assert_eq!(first_status.present_seq.map(|seq| seq.raw()), Some(1));
        assert!(latest_status.signaled);
        assert!(!latest_status.coalesced);
        assert_eq!(latest_status.present_seq.map(|seq| seq.raw()), Some(1));
    }

    #[test]
    fn no_damage_no_present_and_marker_rejects_before_evidence() {
        let mut srv = server();
        let _surface = surface_with_scene(&mut srv, 4, 4);
        assert_eq!(srv.present_scheduler_tick(), Ok(None));
        assert_eq!(srv.last_scheduled_present(), None);
        assert_eq!(v2a_marker_postflight_ready(None), Err(WindowdError::MarkerBeforePresentState));
    }

    #[test]
    fn overlapping_surfaces_route_pointer_focus_and_keyboard_to_topmost() {
        let mut srv = server();
        let bottom_buffer = buffer(20, 8, 8, [0, 0, 0xff, 0xff]);
        let top_buffer = other_buffer(21, 4, 4, [0xff, 0, 0, 0xff]);
        let bottom = srv.create_surface(LAUNCHER, bottom_buffer).expect("bottom");
        let top = srv.create_surface(OTHER, top_buffer).expect("top");
        assert_eq!(
            srv.commit_scene(
                CallerCtx::system(),
                CommitSeq::new(1),
                &[
                    Layer { surface: bottom, x: 0, y: 0, z: 0 },
                    Layer { surface: top, x: 2, y: 2, z: 10 },
                ],
            ),
            Ok(())
        );

        let pointer = srv.route_pointer_down(3, 3).expect("pointer");
        assert_eq!(pointer.surface, top);
        assert_eq!(srv.focused_surface(), Some(top));
        let keyboard = srv.route_keyboard(0x41).expect("keyboard");
        assert_eq!(keyboard.surface, top);
        let top_events = srv.take_input_events(OTHER, top).expect("top events");
        assert_eq!(top_events.len(), 2);
        assert!(top_events.iter().any(|event| event.kind == InputEventKind::PointerDown));
        assert!(top_events
            .iter()
            .any(|event| matches!(event.kind, InputEventKind::Keyboard { key_code: 0x41 })));
        assert_eq!(srv.take_input_events(LAUNCHER, bottom), Ok(Vec::new()));
    }

    #[test]
    fn launcher_click_demo_marker_requires_real_routed_click_state() {
        assert_eq!(launcher::click_marker(None), Err(WindowdError::MarkerBeforePresentState));
        let evidence = launcher::click_demo().expect("click demo");
        assert!(evidence.highlighted);
        assert_eq!(launcher::click_marker(Some(&evidence)), Ok(LAUNCHER_CLICK_OK_MARKER));
    }

    #[test]
    fn v2a_smoke_evidence_gates_marker_literals() {
        let evidence = windowd::run_ui_v2a_smoke().expect("v2a smoke");
        assert!(evidence.present_scheduler_on);
        assert!(evidence.input_on);
        assert!(evidence.launcher_click_ok);
        assert_eq!(focus_marker(evidence.focused_surface), "windowd: focus -> 1");
        assert_eq!(PRESENT_SCHEDULER_ON_MARKER, "windowd: present scheduler on");
        assert_eq!(INPUT_ON_MARKER, "windowd: input on");
        assert_eq!(SELFTEST_UI_V2_PRESENT_OK_MARKER, "SELFTEST: ui v2 present ok");
        assert_eq!(SELFTEST_UI_V2_INPUT_OK_MARKER, "SELFTEST: ui v2 input ok");
        assert_eq!(v2a_marker_postflight_ready(Some(evidence.clone())), Ok(evidence));
    }

    #[test]
    fn visible_input_smoke_couples_cursor_focus_and_click_to_frame_pixels() {
        let evidence = windowd::run_visible_input_smoke().expect("visible input smoke");
        assert!(evidence.input_visible_on);
        assert!(evidence.cursor_move_visible);
        assert!(evidence.hover_visible);
        assert!(evidence.focus_visible);
        assert!(evidence.launcher_click_visible);
        assert_eq!(evidence.focused_surface.raw(), 1);
        assert_eq!(evidence.cursor_start_position.x, 12);
        assert_eq!(evidence.cursor_start_position.y, 12);
        assert_eq!(evidence.cursor_position.x, 36);
        assert_eq!(evidence.cursor_position.y, 28);

        let cursor_frame = evidence.cursor_frame.as_ref().expect("cursor frame");
        assert_eq!(pixel(cursor_frame, 12, 12), VISIBLE_CURSOR_BGRA);
        let hover_frame = evidence.hover_frame.as_ref().expect("hover frame");
        assert_eq!(pixel(hover_frame, 8, 8), VISIBLE_HOVER_BGRA);
        assert_eq!(pixel(hover_frame, 36, 28), VISIBLE_CURSOR_BGRA);
        let frame = evidence.visible_frame.as_ref().expect("visible frame");
        assert_eq!(pixel(frame, 36, 28), VISIBLE_CURSOR_BGRA);
        assert_eq!(pixel(frame, 8, 8), VISIBLE_FOCUS_BGRA);
        assert_eq!(pixel(frame, 24, 18), VISIBLE_INPUT_CLICK_BGRA);
        assert_eq!(
            windowd::visible_input_marker_postflight_ready(Some(evidence.clone())),
            Ok(evidence)
        );
        assert_eq!(INPUT_VISIBLE_ON_MARKER, "windowd: input visible on");
        assert_eq!(CURSOR_MOVE_VISIBLE_MARKER, "windowd: cursor move visible");
        assert_eq!(HOVER_VISIBLE_MARKER, "windowd: hover visible");
        assert_eq!(FOCUS_VISIBLE_MARKER, "windowd: focus visible");
        assert_eq!(LAUNCHER_CLICK_VISIBLE_OK_MARKER, "launcher: click visible ok");
        assert_eq!(SELFTEST_UI_VISIBLE_INPUT_OK_MARKER, "SELFTEST: ui visible input ok");
    }

    #[test]
    fn launcher_visible_click_marker_requires_windowd_visible_evidence() {
        assert_eq!(
            launcher::visible_click_marker(None),
            Err(WindowdError::MarkerBeforePresentState)
        );
        let mut evidence = launcher::visible_click_demo().expect("visible click demo");
        assert_eq!(
            launcher::visible_click_marker(Some(&evidence)),
            Ok(LAUNCHER_CLICK_VISIBLE_OK_MARKER)
        );
        evidence.clicked_visible = false;
        assert_eq!(
            launcher::visible_click_marker(Some(&evidence)),
            Err(WindowdError::MarkerBeforePresentState)
        );
    }

    #[test]
    fn test_reject_stale_unauthorized_and_invalid_present_inputs() {
        let mut srv = server();
        let surface = surface_with_scene(&mut srv, 4, 4);
        assert_eq!(
            srv.acquire_back_buffer(
                LAUNCHER,
                SurfaceId::new(999),
                FrameIndex::new(1),
                buffer(30, 4, 4, [0, 0, 0, 0xff]),
            )
            .map(|_| ()),
            Err(WindowdError::StaleSurfaceId)
        );
        assert_eq!(
            srv.acquire_back_buffer(
                OTHER,
                surface,
                FrameIndex::new(1),
                other_buffer(31, 4, 4, [0, 0, 0, 0xff]),
            )
            .map(|_| ()),
            Err(WindowdError::Unauthorized)
        );
        assert_eq!(
            srv.acquire_back_buffer(
                LAUNCHER,
                surface,
                FrameIndex::new(0),
                buffer(32, 4, 4, [0, 0, 0, 0xff]),
            )
            .map(|_| ()),
            Err(WindowdError::InvalidFrameIndex)
        );
        assert_eq!(
            srv.present_frame(LAUNCHER, surface, FrameIndex::new(77), &[Rect::new(0, 0, 1, 1)])
                .map(|_| ()),
            Err(WindowdError::InvalidFrameIndex)
        );
        assert_eq!(
            srv.take_input_events(OTHER, surface).map(|_| ()),
            Err(WindowdError::Unauthorized)
        );
    }

    #[test]
    fn test_reject_scheduler_queue_and_coalesced_damage_caps() {
        let mut srv = server();
        let surface = surface_with_scene(&mut srv, 16, 16);
        assert!(srv
            .acquire_back_buffer(
                LAUNCHER,
                surface,
                FrameIndex::new(1),
                buffer(40, 16, 16, [0, 0, 0, 0xff]),
            )
            .is_ok());
        assert!(srv
            .acquire_back_buffer(
                LAUNCHER,
                surface,
                FrameIndex::new(2),
                buffer(41, 16, 16, [1, 0, 0, 0xff]),
            )
            .is_ok());
        assert_eq!(
            srv.acquire_back_buffer(
                LAUNCHER,
                surface,
                FrameIndex::new(3),
                buffer(42, 16, 16, [2, 0, 0, 0xff]),
            )
            .map(|_| ()),
            Err(WindowdError::SchedulerQueueFull)
        );
        let damage = [
            Rect::new(0, 0, 1, 1),
            Rect::new(1, 0, 1, 1),
            Rect::new(2, 0, 1, 1),
            Rect::new(3, 0, 1, 1),
            Rect::new(4, 0, 1, 1),
            Rect::new(5, 0, 1, 1),
            Rect::new(6, 0, 1, 1),
            Rect::new(7, 0, 1, 1),
            Rect::new(8, 0, 1, 1),
            Rect::new(9, 0, 1, 1),
            Rect::new(10, 0, 1, 1),
            Rect::new(11, 0, 1, 1),
            Rect::new(12, 0, 1, 1),
            Rect::new(13, 0, 1, 1),
            Rect::new(14, 0, 1, 1),
            Rect::new(15, 0, 1, 1),
        ];
        assert!(srv.present_frame(LAUNCHER, surface, FrameIndex::new(1), &damage).is_ok());
        assert_eq!(
            srv.present_frame(LAUNCHER, surface, FrameIndex::new(2), &[Rect::new(0, 1, 1, 1)])
                .map(|_| ()),
            Err(WindowdError::TooManyDamageRects)
        );
    }

    #[test]
    fn test_reject_input_event_queue_and_keyboard_without_focus() {
        let mut srv = server();
        let surface = surface_with_scene(&mut srv, 4, 4);
        assert_eq!(srv.route_keyboard(0x41), Err(WindowdError::NoFocusedSurface));
        for _ in 0..32 {
            assert!(srv.route_pointer_down(1, 1).is_ok());
        }
        assert_eq!(srv.route_pointer_down(1, 1), Err(WindowdError::InputEventQueueFull));
        assert_eq!(
            srv.take_input_events(LAUNCHER, SurfaceId::new(999)),
            Err(WindowdError::StaleSurfaceId)
        );
        assert_eq!(srv.focused_surface(), Some(surface));
    }

    #[test]
    fn test_reject_visible_input_marker_before_routed_state() {
        assert_eq!(
            windowd::visible_input_marker_postflight_ready(None),
            Err(WindowdError::MarkerBeforePresentState)
        );
    }

    #[test]
    fn test_reject_cursor_move_outside_visible_bounds() {
        let mut srv = server();
        let _surface = surface_with_scene(&mut srv, 4, 4);
        assert_eq!(srv.route_pointer_move(-1, 1), Err(WindowdError::InvalidPointerPosition));
        assert_eq!(srv.route_pointer_move(64, 1), Err(WindowdError::InvalidPointerPosition));
        assert_eq!(srv.pointer_position(), None);
    }

    #[test]
    fn test_reject_focus_visible_without_committed_hit_surface() {
        let mut srv = server();
        assert_eq!(srv.route_pointer_down(4, 4), Err(WindowdError::StaleSurfaceId));
        assert_eq!(srv.render_visible_input_frame(), Err(WindowdError::NoCommittedScene));
    }

    #[test]
    fn test_reject_stale_surface_visible_input_delivery() {
        let mut srv = server();
        let _surface = surface_with_scene(&mut srv, 4, 4);
        assert_eq!(
            srv.take_input_events(LAUNCHER, SurfaceId::new(999)),
            Err(WindowdError::StaleSurfaceId)
        );
    }

    #[test]
    fn test_reject_unauthorized_visible_input_delivery() {
        let mut srv = server();
        let surface = surface_with_scene(&mut srv, 4, 4);
        assert_eq!(srv.take_input_events(OTHER, surface), Err(WindowdError::Unauthorized));
    }

    #[test]
    fn test_reject_visible_input_queue_bounds() {
        let mut srv = server();
        let _surface = surface_with_scene(&mut srv, 4, 4);
        for _ in 0..32 {
            let delivery = srv.route_pointer_move(1, 1).expect("pointer move");
            assert_eq!(delivery.surface.raw(), 1);
        }
        assert_eq!(srv.route_pointer_move(1, 1), Err(WindowdError::InputEventQueueFull));
    }

    #[test]
    fn test_reject_click_visible_marker_without_visible_state_change() {
        let mut evidence = launcher::visible_click_demo().expect("visible click demo");
        evidence.input.launcher_click_visible = false;
        assert_eq!(
            launcher::visible_click_marker(Some(&evidence)),
            Err(WindowdError::MarkerBeforePresentState)
        );
    }

    #[test]
    fn test_reject_v2a_postflight_log_only_mode() {
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|path| path.parent())
            .expect("repo root");
        let output = std::process::Command::new(repo.join("tools/postflight-ui-v2a.sh"))
            .arg("--uart-log")
            .current_dir(repo)
            .output()
            .expect("postflight invocation");
        assert_eq!(output.status.code(), Some(2));
        let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
        assert!(stderr.contains("refuses log-grep-only closure"));
    }

    #[test]
    fn capnp_v2a_contract_roundtrips_use_generated_schema() {
        let surface_id = 9;
        let mut acquire_msg = capnp::message::Builder::new_default();
        {
            let mut req =
                acquire_msg.init_root::<surface_capnp::acquire_back_buffer_request::Builder>();
            req.set_surface_id(surface_id);
            req.set_frame_index(3);
            req.set_width(64);
            req.set_height(48);
            req.set_stride_bytes(256);
            req.set_format(surface_capnp::PixelFormat::Bgra8888);
            req.set_vmo_handle(0x77);
        }
        let mut acquire_bytes = Vec::new();
        capnp::serialize::write_message(&mut acquire_bytes, &acquire_msg).expect("serialize");
        let acquire_reader = capnp::serialize::read_message(
            &mut std::io::Cursor::new(acquire_bytes),
            capnp::message::ReaderOptions::new(),
        )
        .expect("read acquire");
        let acquire = acquire_reader
            .get_root::<surface_capnp::acquire_back_buffer_request::Reader>()
            .expect("acquire root");
        assert_eq!(acquire.get_surface_id(), surface_id);
        assert_eq!(acquire.get_frame_index(), 3);

        let mut vsync_msg = capnp::message::Builder::new_default();
        {
            let mut ack = vsync_msg.init_root::<vsync_capnp::scheduled_present_ack::Builder>();
            ack.set_present_seq(5);
            ack.set_damage_rect_count(2);
            ack.set_frames_coalesced(1);
            ack.set_fences_signaled(2);
            ack.set_latency_ms(16);
        }
        let mut vsync_bytes = Vec::new();
        capnp::serialize::write_message(&mut vsync_bytes, &vsync_msg).expect("serialize vsync");
        let vsync_reader = capnp::serialize::read_message(
            &mut std::io::Cursor::new(vsync_bytes),
            capnp::message::ReaderOptions::new(),
        )
        .expect("read vsync");
        let scheduled = vsync_reader
            .get_root::<vsync_capnp::scheduled_present_ack::Reader>()
            .expect("scheduled root");
        assert_eq!(scheduled.get_frames_coalesced(), 1);
        assert_eq!(scheduled.get_fences_signaled(), 2);

        let mut input_msg = capnp::message::Builder::new_default();
        {
            let mut delivery = input_msg.init_root::<input_capnp::input_delivery::Builder>();
            delivery.set_input_seq(7);
            delivery.set_surface_id(surface_id);
            delivery.set_kind(input_capnp::InputDeliveryKind::PointerDown);
        }
        let mut input_bytes = Vec::new();
        capnp::serialize::write_message(&mut input_bytes, &input_msg).expect("serialize input");
        let input_reader = capnp::serialize::read_message(
            &mut std::io::Cursor::new(input_bytes),
            capnp::message::ReaderOptions::new(),
        )
        .expect("read input");
        let delivery =
            input_reader.get_root::<input_capnp::input_delivery::Reader>().expect("delivery root");
        assert_eq!(delivery.get_surface_id(), surface_id);
        assert_eq!(delivery.get_kind(), Ok(input_capnp::InputDeliveryKind::PointerDown));
    }
}
