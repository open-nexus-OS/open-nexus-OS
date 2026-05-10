// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0055/0055B/0055C host behavior proofs for `windowd`.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: `cargo test -p ui_windowd_host -- --nocapture`, `cargo test -p ui_windowd_host capnp -- --nocapture`
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

mod surface_capnp {
    include!(concat!(env!("OUT_DIR"), "/surface_capnp.rs"));
}

mod layer_capnp {
    include!(concat!(env!("OUT_DIR"), "/layer_capnp.rs"));
}

mod vsync_capnp {
    include!(concat!(env!("OUT_DIR"), "/vsync_capnp.rs"));
}

mod input_capnp {
    include!(concat!(env!("OUT_DIR"), "/input_capnp.rs"));
}

#[cfg(test)]
mod tests {
    use crate::{input_capnp, layer_capnp, surface_capnp, vsync_capnp};
    use windowd::{
        marker_postflight_ready, present_marker, run_visible_bootstrap_smoke,
        run_visible_systemui_smoke, validate_visible_bootstrap_capability,
        visible_marker_postflight_ready, visible_systemui_marker_postflight_ready, CallerCtx,
        CommitSeq, InputStubStatus, Layer, PixelFormat, PresentAck, PresentSeq, Rect,
        SurfaceBuffer, SurfaceId, VisibleBootstrapMode, VisibleDisplayCapability, VmoHandleId,
        VmoRights, WindowServer, WindowdConfig, WindowdError, PRESENT_VISIBLE_MARKER,
        SELFTEST_UI_VISIBLE_PRESENT_MARKER, SYSTEMUI_FIRST_FRAME_VISIBLE_MARKER,
        VISIBLE_BACKEND_MARKER,
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

    fn pixel(frame: &windowd::Frame, x: u32, y: u32) -> [u8; 4] {
        let idx = (y as usize * frame.stride as usize) + (x as usize * 4);
        [frame.pixels[idx], frame.pixels[idx + 1], frame.pixels[idx + 2], frame.pixels[idx + 3]]
    }

    #[test]
    fn two_surfaces_with_damage_composite_expected_pixels() {
        let mut server = server();
        let bottom = buffer(1, 4, 4, [1, 2, 3, 0xff]);
        let top = buffer(2, 2, 2, [9, 8, 7, 0xff]);
        let bottom_id = match server.create_surface(LAUNCHER, bottom.clone()) {
            Ok(id) => id,
            Err(err) => panic!("create bottom failed: {err:?}"),
        };
        let top_id = match server.create_surface(LAUNCHER, top.clone()) {
            Ok(id) => id,
            Err(err) => panic!("create top failed: {err:?}"),
        };
        assert_eq!(
            server.queue_buffer(LAUNCHER, bottom_id, bottom, &[Rect::new(0, 0, 4, 4)]),
            Ok(())
        );
        assert_eq!(server.queue_buffer(LAUNCHER, top_id, top, &[Rect::new(0, 0, 2, 2)]), Ok(()));
        assert_eq!(
            server.commit_scene(
                CallerCtx::system(),
                CommitSeq::new(1),
                &[
                    Layer { surface: top_id, x: 1, y: 1, z: 10 },
                    Layer { surface: bottom_id, x: 0, y: 0, z: 0 },
                ],
            ),
            Ok(())
        );
        let ack = match server.present_tick() {
            Ok(Some(ack)) => ack,
            other => panic!("expected present ack, got {other:?}"),
        };
        assert_eq!(ack.seq.raw(), 1);
        assert_eq!(ack.damage_rects, 2);
        let frame = match server.last_frame() {
            Some(frame) => frame,
            None => panic!("missing composed frame"),
        };
        assert_eq!(pixel(frame, 0, 0), [1, 2, 3, 0xff]);
        assert_eq!(pixel(frame, 1, 1), [9, 8, 7, 0xff]);
        assert_eq!(pixel(frame, 2, 2), [9, 8, 7, 0xff]);
        assert_eq!(pixel(frame, 3, 3), [1, 2, 3, 0xff]);
    }

    #[test]
    fn no_damage_means_no_present_sequence_advance() {
        let mut server = server();
        let initial = buffer(3, 2, 2, [3, 4, 5, 0xff]);
        let surface = match server.create_surface(LAUNCHER, initial.clone()) {
            Ok(id) => id,
            Err(err) => panic!("create failed: {err:?}"),
        };
        assert_eq!(
            server.queue_buffer(LAUNCHER, surface, initial, &[Rect::new(0, 0, 2, 2)]),
            Ok(())
        );
        assert_eq!(
            server.commit_scene(
                CallerCtx::system(),
                CommitSeq::new(1),
                &[Layer { surface, x: 0, y: 0, z: 0 }],
            ),
            Ok(())
        );
        assert!(matches!(server.present_tick(), Ok(Some(_))));
        assert_eq!(server.present_tick(), Ok(None));
    }

    #[test]
    fn bounded_layer_ordering_is_deterministic() {
        let mut a = server();
        let mut b = server();
        let red = buffer(4, 2, 2, [0, 0, 0xff, 0xff]);
        let blue = buffer(5, 2, 2, [0xff, 0, 0, 0xff]);
        let a_red = match a.create_surface(LAUNCHER, red.clone()) {
            Ok(id) => id,
            Err(err) => panic!("create a red failed: {err:?}"),
        };
        let a_blue = match a.create_surface(LAUNCHER, blue.clone()) {
            Ok(id) => id,
            Err(err) => panic!("create a blue failed: {err:?}"),
        };
        let b_red = match b.create_surface(LAUNCHER, red.clone()) {
            Ok(id) => id,
            Err(err) => panic!("create b red failed: {err:?}"),
        };
        let b_blue = match b.create_surface(LAUNCHER, blue.clone()) {
            Ok(id) => id,
            Err(err) => panic!("create b blue failed: {err:?}"),
        };
        for (srv, first, second) in [(&mut a, a_red, a_blue), (&mut b, b_red, b_blue)] {
            assert_eq!(
                srv.queue_buffer(LAUNCHER, first, red.clone(), &[Rect::new(0, 0, 2, 2)]),
                Ok(())
            );
            assert_eq!(
                srv.queue_buffer(LAUNCHER, second, blue.clone(), &[Rect::new(0, 0, 2, 2)]),
                Ok(())
            );
        }
        assert_eq!(
            a.commit_scene(
                CallerCtx::system(),
                CommitSeq::new(1),
                &[
                    Layer { surface: a_blue, x: 0, y: 0, z: 5 },
                    Layer { surface: a_red, x: 0, y: 0, z: 5 },
                ],
            ),
            Ok(())
        );
        assert_eq!(
            b.commit_scene(
                CallerCtx::system(),
                CommitSeq::new(1),
                &[
                    Layer { surface: b_red, x: 0, y: 0, z: 5 },
                    Layer { surface: b_blue, x: 0, y: 0, z: 5 },
                ],
            ),
            Ok(())
        );
        assert!(matches!(a.present_tick(), Ok(Some(_))));
        assert!(matches!(b.present_tick(), Ok(Some(_))));
        let a_pixels = match a.last_frame() {
            Some(frame) => &frame.pixels,
            None => panic!("missing a frame"),
        };
        let b_pixels = match b.last_frame() {
            Some(frame) => &frame.pixels,
            None => panic!("missing b frame"),
        };
        assert_eq!(a_pixels, b_pixels);
    }

    #[test]
    fn minimal_present_ack_after_compose() {
        let evidence = match windowd::run_headless_ui_smoke() {
            Ok(evidence) => evidence,
            Err(err) => panic!("smoke failed: {err:?}"),
        };
        assert_eq!(evidence.first_present.seq.raw(), 1);
        assert_eq!(evidence.first_present.damage_rects, 1);
        assert!(evidence.launcher_first_frame);
    }

    #[test]
    fn test_reject_invalid_dimensions_stride_and_format() {
        assert_eq!(
            SurfaceBuffer::solid(LAUNCHER, 6, 0, 2, [0, 0, 0, 0xff]).map(|_| ()),
            Err(WindowdError::InvalidDimensions)
        );
        let mut bad_stride = buffer(7, 2, 2, [0, 0, 0, 0xff]);
        bad_stride.stride = 4;
        assert_eq!(
            server().create_surface(LAUNCHER, bad_stride).map(|_| ()),
            Err(WindowdError::InvalidStride)
        );
        let mut bad_format = buffer(8, 2, 2, [0, 0, 0, 0xff]);
        bad_format.format = PixelFormat::Unsupported(9);
        assert_eq!(
            server().create_surface(LAUNCHER, bad_format).map(|_| ()),
            Err(WindowdError::UnsupportedFormat)
        );
    }

    #[test]
    fn test_reject_missing_forged_wrong_rights_vmo_handles() {
        let mut missing = buffer(9, 2, 2, [0, 0, 0, 0xff]);
        missing.handle.id = VmoHandleId::new(0);
        assert_eq!(
            server().create_surface(LAUNCHER, missing).map(|_| ()),
            Err(WindowdError::MissingVmoHandle)
        );
        let mut forged = buffer(10, 2, 2, [0, 0, 0, 0xff]);
        forged.handle.owner = OTHER.caller_id();
        assert_eq!(
            server().create_surface(LAUNCHER, forged).map(|_| ()),
            Err(WindowdError::ForgedVmoHandle)
        );
        let mut wrong_rights = buffer(11, 2, 2, [0, 0, 0, 0xff]);
        wrong_rights.handle.rights = VmoRights::read_only();
        assert_eq!(
            server().create_surface(LAUNCHER, wrong_rights).map(|_| ()),
            Err(WindowdError::WrongVmoRights)
        );
        let mut not_surface = buffer(12, 2, 2, [0, 0, 0, 0xff]);
        not_surface.handle.surface_buffer = false;
        assert_eq!(
            server().create_surface(LAUNCHER, not_surface).map(|_| ()),
            Err(WindowdError::NonSurfaceBuffer)
        );
    }

    #[test]
    fn test_reject_stale_surface_ids() {
        let mut srv = server();
        let stale = SurfaceId::new(999);
        assert_eq!(
            srv.queue_buffer(
                LAUNCHER,
                stale,
                buffer(13, 2, 2, [0, 0, 0, 0xff]),
                &[Rect::new(0, 0, 2, 2)]
            ),
            Err(WindowdError::StaleSurfaceId)
        );
    }

    #[test]
    fn test_reject_stale_commit_sequence_numbers() {
        let mut srv = server();
        let surface = match srv.create_surface(LAUNCHER, buffer(14, 2, 2, [0, 0, 0, 0xff])) {
            Ok(id) => id,
            Err(err) => panic!("create failed: {err:?}"),
        };
        assert_eq!(
            srv.commit_scene(
                CallerCtx::system(),
                CommitSeq::new(2),
                &[Layer { surface, x: 0, y: 0, z: 0 }],
            ),
            Err(WindowdError::StaleCommitSequence)
        );
    }

    #[test]
    fn test_reject_unauthorized_layer_mutation() {
        let mut srv = server();
        let surface = match srv.create_surface(LAUNCHER, buffer(15, 2, 2, [0, 0, 0, 0xff])) {
            Ok(id) => id,
            Err(err) => panic!("create failed: {err:?}"),
        };
        assert_eq!(
            srv.commit_scene(LAUNCHER, CommitSeq::new(1), &[Layer { surface, x: 0, y: 0, z: 0 }]),
            Err(WindowdError::Unauthorized)
        );
    }

    #[test]
    fn test_reject_marker_postflight_before_real_present_state() {
        assert_eq!(marker_postflight_ready(None), Err(WindowdError::MarkerBeforePresentState));
        let srv = server();
        assert_eq!(srv.marker_evidence(), Err(WindowdError::MarkerBeforePresentState));
    }

    #[test]
    fn visible_bootstrap_accepts_only_fixed_mode_after_present() {
        let evidence = match run_visible_bootstrap_smoke() {
            Ok(evidence) => evidence,
            Err(err) => panic!("visible bootstrap failed: {err:?}"),
        };
        let mode = evidence.mode;
        assert_eq!(mode.width, windowd::VISIBLE_BOOTSTRAP_WIDTH);
        assert_eq!(mode.height, windowd::VISIBLE_BOOTSTRAP_HEIGHT);
        assert_eq!(mode.stride, windowd::VISIBLE_BOOTSTRAP_WIDTH * 4);
        assert_eq!(mode.format, PixelFormat::Bgra8888);
        assert_eq!(evidence.first_present.seq.raw(), 1);
        assert_eq!(evidence.seed_surface.width, 64);
        assert_eq!(evidence.seed_surface.height, 48);
        assert_eq!(evidence.seed_surface.format, PixelFormat::Bgra8888);
        assert_eq!(visible_marker_postflight_ready(Some(evidence.clone())), Ok(evidence));
        assert_eq!(
            visible_marker_postflight_ready(None),
            Err(WindowdError::MarkerBeforePresentState)
        );
    }

    #[test]
    fn visible_systemui_present_uses_systemui_frame_source() {
        let evidence = match run_visible_systemui_smoke() {
            Ok(evidence) => evidence,
            Err(err) => panic!("visible systemui failed: {err:?}"),
        };
        assert!(evidence.ready);
        assert!(evidence.backend_visible);
        assert!(evidence.systemui_first_frame);
        assert_eq!(evidence.first_present.seq.raw(), 1);
        assert_eq!(evidence.frame_source.width, 160);
        assert_eq!(evidence.frame_source.height, 100);
        assert_eq!(evidence.frame_source.stride, 640);
        assert_eq!(evidence.frame_source.format, PixelFormat::Bgra8888);
        assert_eq!(evidence.frame_source.pixels[0..4], [0x80, 0x50, 0x20, 0xff]);
        let composed_frame = evidence.composed_frame.as_ref().expect("host composed frame");
        assert_eq!(composed_frame.width, windowd::VISIBLE_BOOTSTRAP_WIDTH);
        assert_eq!(composed_frame.height, windowd::VISIBLE_BOOTSTRAP_HEIGHT);
        assert_eq!(composed_frame.stride, windowd::VISIBLE_BOOTSTRAP_WIDTH * 4);
        assert_eq!(composed_frame.pixels[0..4], evidence.frame_source.pixels[0..4]);
        let inner_pixel = (20 * composed_frame.stride as usize) + (12 * 4);
        assert_eq!(composed_frame.pixels[inner_pixel..inner_pixel + 4], [0x24, 0x28, 0x34, 0xff]);
        let mut row = [0xff; windowd::VISIBLE_BOOTSTRAP_WIDTH as usize * 4];
        evidence.copy_composed_row(20, &mut row).expect("copy composed row");
        assert_eq!(row[12 * 4..(12 * 4) + 4], [0x24, 0x28, 0x34, 0xff]);
        assert_eq!(row[200 * 4..(200 * 4) + 4], [0, 0, 0, 0]);
        assert_eq!(visible_systemui_marker_postflight_ready(Some(evidence.clone())), Ok(evidence));
    }

    #[test]
    fn test_reject_visible_systemui_marker_before_present_state() {
        assert_eq!(
            visible_systemui_marker_postflight_ready(None),
            Err(WindowdError::MarkerBeforePresentState)
        );
        assert_eq!(VISIBLE_BACKEND_MARKER, "windowd: backend=visible");
        assert_eq!(PRESENT_VISIBLE_MARKER, "windowd: present visible ok");
        assert_eq!(SYSTEMUI_FIRST_FRAME_VISIBLE_MARKER, "systemui: first frame visible");
        assert_eq!(SELFTEST_UI_VISIBLE_PRESENT_MARKER, "SELFTEST: ui visible present ok");
    }

    #[test]
    fn test_reject_visible_bootstrap_mode_capability_and_prescanout_marker() {
        let mode = VisibleBootstrapMode::fixed().expect("fixed visible mode");
        assert_eq!(
            VisibleBootstrapMode { width: 64, ..mode }.validate(),
            Err(WindowdError::InvalidDimensions)
        );
        assert_eq!(
            VisibleBootstrapMode { stride: mode.stride - 4, ..mode }.validate(),
            Err(WindowdError::InvalidStride)
        );
        assert_eq!(
            VisibleBootstrapMode { format: PixelFormat::Unsupported(0x55), ..mode }.validate(),
            Err(WindowdError::UnsupportedFormat)
        );
        for cap in [
            VisibleDisplayCapability {
                byte_len: mode.byte_len().expect("mode byte len") - 1,
                mapped: true,
                writable: true,
            },
            VisibleDisplayCapability {
                byte_len: mode.byte_len().expect("mode byte len"),
                mapped: false,
                writable: true,
            },
            VisibleDisplayCapability {
                byte_len: mode.byte_len().expect("mode byte len"),
                mapped: true,
                writable: false,
            },
        ] {
            assert_eq!(
                validate_visible_bootstrap_capability(mode, cap),
                Err(WindowdError::InvalidDisplayCapability)
            );
        }
    }

    #[test]
    fn test_reject_buffer_length_mismatch() {
        let mut bad = buffer(16, 2, 2, [0, 0, 0, 0xff]);
        let _ = bad.pixels.pop();
        assert_eq!(
            server().create_surface(LAUNCHER, bad).map(|_| ()),
            Err(WindowdError::BufferLengthMismatch)
        );
    }

    #[test]
    fn test_reject_bounds_for_surface_layers_damage_and_total_bytes() {
        let mut srv = server();
        for handle in 100..132 {
            assert!(srv.create_surface(LAUNCHER, buffer(handle, 1, 1, [0, 0, 0, 0xff])).is_ok());
        }
        assert_eq!(
            srv.create_surface(LAUNCHER, buffer(132, 1, 1, [0, 0, 0, 0xff])).map(|_| ()),
            Err(WindowdError::TooManySurfaces)
        );

        let mut srv = server();
        let surface =
            srv.create_surface(LAUNCHER, buffer(200, 1, 1, [0, 0, 0, 0xff])).expect("surface");
        let too_many_layers = [Layer { surface, x: 0, y: 0, z: 0 }; 17];
        assert_eq!(
            srv.commit_scene(CallerCtx::system(), CommitSeq::new(1), &too_many_layers),
            Err(WindowdError::TooManyLayers)
        );

        let damage = [Rect::new(0, 0, 1, 1); 17];
        assert_eq!(
            srv.queue_buffer(LAUNCHER, surface, buffer(201, 1, 1, [0, 0, 0, 0xff]), &damage),
            Err(WindowdError::TooManyDamageRects)
        );

        let mut oversized = buffer(202, 2, 2, [0, 0, 0, 0xff]);
        oversized.stride = 64 * 1024 * 1024;
        assert_eq!(
            server().create_surface(LAUNCHER, oversized).map(|_| ()),
            Err(WindowdError::SurfaceTooLarge)
        );
    }

    #[test]
    fn test_reject_invalid_damage_and_no_committed_scene() {
        let mut srv = server();
        let surface =
            srv.create_surface(LAUNCHER, buffer(300, 2, 2, [0, 0, 0, 0xff])).expect("surface");
        assert_eq!(
            srv.queue_buffer(
                LAUNCHER,
                surface,
                buffer(301, 2, 2, [0, 0, 0, 0xff]),
                &[Rect::new(1, 1, 2, 2)]
            ),
            Err(WindowdError::InvalidDamage)
        );
        assert_eq!(srv.present_tick(), Err(WindowdError::NoCommittedScene));
    }

    #[test]
    fn rejected_scene_commit_preserves_previous_scene_and_sequence() {
        let mut srv = server();
        let red = buffer(400, 2, 2, [0, 0, 0xff, 0xff]);
        let surface = srv.create_surface(LAUNCHER, red.clone()).expect("surface");
        srv.queue_buffer(LAUNCHER, surface, red, &[Rect::new(0, 0, 2, 2)]).expect("queue");
        srv.commit_scene(
            CallerCtx::system(),
            CommitSeq::new(1),
            &[Layer { surface, x: 0, y: 0, z: 0 }],
        )
        .expect("commit");
        assert!(matches!(srv.present_tick(), Ok(Some(_))));

        let blue = buffer(401, 2, 2, [0xff, 0, 0, 0xff]);
        srv.queue_buffer(LAUNCHER, surface, blue, &[Rect::new(0, 0, 2, 2)]).expect("queue");
        assert_eq!(
            srv.commit_scene(
                CallerCtx::system(),
                CommitSeq::new(2),
                &[
                    Layer { surface, x: 0, y: 0, z: 0 },
                    Layer { surface: SurfaceId::new(999), x: 10, y: 10, z: 1 },
                ],
            ),
            Err(WindowdError::StaleSurfaceId)
        );
        srv.commit_scene(
            CallerCtx::system(),
            CommitSeq::new(2),
            &[Layer { surface, x: 0, y: 0, z: 0 }],
        )
        .expect("commit sequence preserved");
        assert_eq!(srv.present_tick().expect("present").expect("ack").seq.raw(), 2);
    }

    #[test]
    fn vsync_subscription_reports_only_new_present_ack() {
        let mut srv = server();
        assert_eq!(
            srv.subscribe_vsync(PresentSeq::new(0)),
            Err(WindowdError::MarkerBeforePresentState)
        );
        let surface =
            srv.create_surface(LAUNCHER, buffer(500, 2, 2, [0, 0, 0, 0xff])).expect("surface");
        srv.queue_buffer(
            LAUNCHER,
            surface,
            buffer(501, 2, 2, [0, 0, 0, 0xff]),
            &[Rect::new(0, 0, 2, 2)],
        )
        .expect("queue");
        srv.commit_scene(
            CallerCtx::system(),
            CommitSeq::new(1),
            &[Layer { surface, x: 0, y: 0, z: 0 }],
        )
        .expect("commit");
        let ack = srv.present_tick().expect("present").expect("ack");
        assert_eq!(srv.subscribe_vsync(PresentSeq::new(0)), Ok(Some(ack)));
        assert_eq!(srv.subscribe_vsync(ack.seq), Ok(None));
    }

    #[test]
    fn input_stub_is_explicitly_unsupported_and_authorized() {
        let mut srv = server();
        let surface =
            srv.create_surface(LAUNCHER, buffer(600, 2, 2, [0, 0, 0, 0xff])).expect("surface");
        assert_eq!(
            srv.subscribe_input_stub(LAUNCHER, surface),
            Ok(InputStubStatus::UnsupportedStub)
        );
        assert_eq!(srv.subscribe_input_stub(OTHER, surface), Err(WindowdError::Unauthorized));
    }

    #[test]
    fn present_marker_is_rendered_from_ack_evidence() {
        let ack = PresentAck { seq: PresentSeq::new(7), damage_rects: 3 };
        assert_eq!(present_marker(ack), "windowd: present ok (seq=7 dmg=3)");
    }

    #[test]
    fn test_reject_postflight_log_only_mode() {
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|path| path.parent())
            .expect("repo root");
        let output = std::process::Command::new(repo.join("tools/postflight-ui.sh"))
            .arg("--uart-log")
            .current_dir(repo)
            .output()
            .expect("postflight invocation");
        assert_eq!(output.status.code(), Some(2));
        let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
        assert!(stderr.contains("refuses log-grep-only closure"));
    }

    #[test]
    fn idl_contract_shape_names_required_reject_classes() {
        let surface = include_str!("../../../source/services/windowd/idl/surface.capnp");
        let layer = include_str!("../../../source/services/windowd/idl/layer.capnp");
        let vsync = include_str!("../../../source/services/windowd/idl/vsync.capnp");
        let input = include_str!("../../../source/services/windowd/idl/input.capnp");

        for required in [
            "SurfaceCreateRequest",
            "QueueBufferRequest",
            "forgedVmoHandle",
            "wrongVmoRights",
            "tooManyDamageRects",
        ] {
            assert!(surface.contains(required), "surface IDL missing {required}");
        }
        assert!(layer.contains("SceneCommitRequest"));
        assert!(layer.contains("unauthorized"));
        assert!(vsync.contains("PresentAck"));
        assert!(vsync.contains("SubscribeVsyncRequest"));
        assert!(input.contains("InputStub"));
        assert!(input.contains("unsupported"));
    }

    #[test]
    fn capnp_surface_create_roundtrip_uses_generated_schema() {
        let mut message = capnp::message::Builder::new_default();
        {
            let mut req = message.init_root::<surface_capnp::surface_create_request::Builder>();
            req.set_width(64);
            req.set_height(48);
            req.set_stride_bytes(256);
            req.set_format(surface_capnp::PixelFormat::Bgra8888);
            req.set_vmo_handle(0x55);
        }

        let mut encoded = Vec::new();
        capnp::serialize::write_message(&mut encoded, &message).expect("serialize surface");
        let reader = capnp::serialize::read_message(
            &mut std::io::Cursor::new(encoded),
            capnp::message::ReaderOptions::new(),
        )
        .expect("read surface");
        let req = reader
            .get_root::<surface_capnp::surface_create_request::Reader>()
            .expect("surface root");

        assert_eq!(req.get_width(), 64);
        assert_eq!(req.get_height(), 48);
        assert_eq!(req.get_stride_bytes(), 256);
        assert_eq!(req.get_format(), Ok(surface_capnp::PixelFormat::Bgra8888));
        assert_eq!(req.get_vmo_handle(), 0x55);
    }

    #[test]
    fn capnp_queue_buffer_damage_roundtrip_uses_generated_schema() {
        let mut message = capnp::message::Builder::new_default();
        {
            let mut req = message.init_root::<surface_capnp::queue_buffer_request::Builder>();
            req.set_surface_id(7);
            req.set_commit_seq(9);
            req.set_width(16);
            req.set_height(8);
            req.set_stride_bytes(64);
            req.set_format(surface_capnp::PixelFormat::Bgra8888);
            req.set_vmo_handle(0x77);
            let mut damage = req.reborrow().init_damage(1);
            let mut rect = damage.reborrow().get(0);
            rect.set_x(1);
            rect.set_y(2);
            rect.set_width(3);
            rect.set_height(4);
        }

        let mut encoded = Vec::new();
        capnp::serialize::write_message(&mut encoded, &message).expect("serialize queue");
        let reader = capnp::serialize::read_message(
            &mut std::io::Cursor::new(encoded),
            capnp::message::ReaderOptions::new(),
        )
        .expect("read queue");
        let req =
            reader.get_root::<surface_capnp::queue_buffer_request::Reader>().expect("queue root");
        let damage = req.get_damage().expect("damage");
        let rect = damage.get(0);

        assert_eq!(req.get_surface_id(), 7);
        assert_eq!(req.get_commit_seq(), 9);
        assert_eq!(rect.get_x(), 1);
        assert_eq!(rect.get_y(), 2);
        assert_eq!(rect.get_width(), 3);
        assert_eq!(rect.get_height(), 4);
    }

    #[test]
    fn capnp_layer_vsync_input_roundtrips_use_generated_schemas() {
        let mut layer_msg = capnp::message::Builder::new_default();
        {
            let mut req = layer_msg.init_root::<layer_capnp::scene_commit_request::Builder>();
            req.set_commit_seq(3);
            let mut layers = req.reborrow().init_layers(1);
            let mut entry = layers.reborrow().get(0);
            entry.set_surface_id(11);
            entry.set_x(-2);
            entry.set_y(5);
            entry.set_z(4);
        }
        let mut layer_bytes = Vec::new();
        capnp::serialize::write_message(&mut layer_bytes, &layer_msg).expect("serialize layer");
        let layer_reader = capnp::serialize::read_message(
            &mut std::io::Cursor::new(layer_bytes),
            capnp::message::ReaderOptions::new(),
        )
        .expect("read layer");
        let layer_req = layer_reader
            .get_root::<layer_capnp::scene_commit_request::Reader>()
            .expect("layer root");
        let layers = layer_req.get_layers().expect("layers");
        let layer = layers.get(0);
        assert_eq!(layer_req.get_commit_seq(), 3);
        assert_eq!(layer.get_surface_id(), 11);
        assert_eq!(layer.get_x(), -2);
        assert_eq!(layer.get_y(), 5);
        assert_eq!(layer.get_z(), 4);

        let mut vsync_msg = capnp::message::Builder::new_default();
        vsync_msg
            .init_root::<vsync_capnp::subscribe_vsync_request::Builder>()
            .set_last_seen_present_seq(12);
        let mut vsync_bytes = Vec::new();
        capnp::serialize::write_message(&mut vsync_bytes, &vsync_msg).expect("serialize vsync");
        let vsync_reader = capnp::serialize::read_message(
            &mut std::io::Cursor::new(vsync_bytes),
            capnp::message::ReaderOptions::new(),
        )
        .expect("read vsync");
        assert_eq!(
            vsync_reader
                .get_root::<vsync_capnp::subscribe_vsync_request::Reader>()
                .expect("vsync root")
                .get_last_seen_present_seq(),
            12
        );

        let mut input_msg = capnp::message::Builder::new_default();
        input_msg.init_root::<input_capnp::input_subscribe_request::Builder>().set_surface_id(22);
        let mut input_bytes = Vec::new();
        capnp::serialize::write_message(&mut input_bytes, &input_msg).expect("serialize input");
        let input_reader = capnp::serialize::read_message(
            &mut std::io::Cursor::new(input_bytes),
            capnp::message::ReaderOptions::new(),
        )
        .expect("read input");
        assert_eq!(
            input_reader
                .get_root::<input_capnp::input_subscribe_request::Reader>()
                .expect("input root")
                .get_surface_id(),
            22
        );
    }
}
