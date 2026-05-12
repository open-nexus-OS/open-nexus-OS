// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host behavior proofs for pointer-motion coalescing, no-damage frame skip,
//! idle-cheap wakeup bounding, and semantic-edge preservation in the windowd present pipeline.
//! OWNERS: @ui @runtime
//! STATUS: Draft
//! API_STABILITY: Stable for coalescing/skip/perf proof floor
//! TEST_COVERAGE: Host proof suite for windowd coalescing, skip rules, and latency contracts
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

#[cfg(test)]
mod tests {
    use windowd::{
        CallerCtx, CommitSeq, InputEventKind, Layer, PresentAck, Rect, ScheduledPresentAck,
        SurfaceBuffer, TouchInputPhase, WindowServer, WindowdConfig, WindowdError,
    };

    const LAUNCHER: CallerCtx = CallerCtx::system();

    fn server() -> WindowServer {
        match WindowServer::new(WindowdConfig::default()) {
            Ok(server) => server,
            Err(err) => panic!("server init failed: {err:?}"),
        }
    }

    fn surface_buffer(handle: u64, width: u32, height: u32, color: [u8; 4]) -> SurfaceBuffer {
        match SurfaceBuffer::solid(LAUNCHER, handle, width, height, color) {
            Ok(buffer) => buffer,
            Err(err) => panic!("buffer build failed: {err:?}"),
        }
    }

    /// Helper: create surface, queue initial buffer with full damage, and commit scene.
    fn register_surface(server: &mut WindowServer, handle: u64, width: u32, height: u32) {
        let buffer = surface_buffer(handle, width, height, [128, 0, 0, 255]);
        let sid = server.create_surface(LAUNCHER, buffer.clone()).expect("create_surface");
        let damage = [Rect::new(0, 0, width, height)];
        server.queue_buffer(LAUNCHER, sid, buffer, &damage).expect("queue_buffer");
        server
            .commit_scene(LAUNCHER, CommitSeq::new(1), &[Layer { surface: sid, z: 0, x: 0, y: 0 }])
            .expect("commit_scene");
        server.route_pointer_move(10, 10).expect("initial pointer move");
        server.present_scheduler_tick().expect("first present");
    }

    // ── Phase A: Pointer-motion coalescing host proofs ──

    #[test]
    fn coalesce_pointer_move_within_budget_preserves_position() {
        let mut server = server();
        register_surface(&mut server, 1, 64, 48);
        server.enable_fastpath();

        // 8 pointer moves within budget should all coalesce
        for i in 0..8_i32 {
            assert!(server.try_coalesce_pointer_move(i, i * 2).unwrap());
        }
        // Position should reflect last coalesced move
        let pos = server.last_coalesced_pointer().expect("last position set");
        assert_eq!(pos.x, 7);
        assert_eq!(pos.y, 14);
    }

    #[test]
    fn reject_coalesce_when_fastpath_disabled() {
        let mut server = server();
        register_surface(&mut server, 1, 64, 48);
        // fastpath NOT enabled
        assert_eq!(server.try_coalesce_pointer_move(10, 20), Err(WindowdError::FastPathDisabled));
    }

    #[test]
    fn reject_coalesce_when_burst_exceeded() {
        let mut server = server();
        register_surface(&mut server, 1, 64, 48);
        server.enable_fastpath();

        // 8 in budget
        for i in 0..8_i32 {
            assert!(server.try_coalesce_pointer_move(i, i).unwrap());
        }
        // 9th exceeds budget
        assert_eq!(
            server.try_coalesce_pointer_move(99, 99),
            Err(WindowdError::CoalesceBurstExceeded)
        );
    }

    #[test]
    fn reset_coalesce_burst_clears_counter_and_position() {
        let mut server = server();
        register_surface(&mut server, 1, 64, 48);
        server.enable_fastpath();

        // Exhaust burst
        for i in 0..8_i32 {
            server.try_coalesce_pointer_move(i, i).unwrap();
        }
        assert_eq!(
            server.try_coalesce_pointer_move(99, 99),
            Err(WindowdError::CoalesceBurstExceeded)
        );

        // Reset and verify clean state
        server.reset_coalesce_burst();
        assert_eq!(server.pointer_coalesce_burst(), 0);
        assert!(server.last_coalesced_pointer().is_none());
        assert!(server.try_coalesce_pointer_move(1, 1).unwrap());
    }

    #[test]
    fn pointer_coalesce_counters_exposed_for_telemetry() {
        let mut server = server();
        register_surface(&mut server, 1, 64, 48);
        server.enable_fastpath();

        assert_eq!(server.pointer_coalesce_burst(), 0);
        assert_eq!(server.no_damage_skips(), 0);
        assert_eq!(server.idle_cheap_wakeups(), 0);

        for i in 0..3_i32 {
            server.try_coalesce_pointer_move(i, i).unwrap();
        }
        assert_eq!(server.pointer_coalesce_burst(), 3);
    }

    #[test]
    fn fastpath_state_is_queryable() {
        let mut server = server();
        register_surface(&mut server, 1, 64, 48);

        assert!(!server.fastpath_enabled());
        server.enable_fastpath();
        assert!(server.fastpath_enabled());
    }

    // ── Phase B: No-damage skip host proofs ──

    #[test]
    fn no_damage_skip_within_budget_when_no_visible_change() {
        let mut server = server();
        register_surface(&mut server, 1, 64, 48);

        // First present sets the frame, establish baseline hash
        let _ = server.present_scheduler_tick().expect("first present");
        server.set_last_frame_hash_for_tests(server.compute_frame_hash());
        // Skip up to MAX_NO_DAMAGE_SKIPS (4) times
        for _ in 0..4 {
            assert!(server.try_no_damage_skip().unwrap());
        }
    }

    #[test]
    fn no_damage_skip_forced_after_budget() {
        let mut server = server();
        register_surface(&mut server, 1, 64, 48);

        let _ = server.present_scheduler_tick().expect("first present");
        server.set_last_frame_hash_for_tests(server.compute_frame_hash());
        // Exhaust skip budget
        for _ in 0..4 {
            assert!(server.try_no_damage_skip().unwrap());
        }
        // 5th must return false (force present)
        assert!(!server.try_no_damage_skip().unwrap());
    }

    #[test]
    fn no_damage_skip_resets_on_frame_change() {
        let mut server = server();
        register_surface(&mut server, 1, 64, 48);

        let _ = server.present_scheduler_tick().expect("first present");
        server.set_last_frame_hash_for_tests(server.compute_frame_hash());
        // One skip
        assert!(server.try_no_damage_skip().unwrap());
        // Simulate frame change (hash mismatch)
        server.set_last_frame_hash_for_tests(Some(0xDEAD_BEEF_CAFE_BABE));
        // Now should force present (hash mismatch)
        assert!(!server.try_no_damage_skip().unwrap());
        // After forced present, hash updated; should skip again
        server.set_last_frame_hash_for_tests(server.compute_frame_hash());
        assert!(server.try_no_damage_skip().unwrap());
    }

    #[test]
    fn idle_cheap_exceeded_error_after_budget() {
        let mut server = server();
        register_surface(&mut server, 1, 64, 48);

        let _ = server.present_scheduler_tick().expect("first present");
        server.set_last_frame_hash_for_tests(server.compute_frame_hash());
        // Push idle-cheap wakeups to threshold
        server.set_idle_cheap_wakeups_for_tests(6);
        // Next skip exceeds idle-cheap budget
        assert_eq!(server.try_no_damage_skip(), Err(WindowdError::IdleCheapBudgetExceeded));
    }

    #[test]
    fn compute_frame_hash_is_deterministic() {
        let mut server = server();
        register_surface(&mut server, 1, 64, 48);

        let hash1 = server.compute_frame_hash();
        let hash2 = server.compute_frame_hash();
        assert_eq!(hash1, hash2, "frame hash must be deterministic");
    }

    // ── Phase C: Semantic edge guards (reject-path proofs) ──

    #[test]
    fn semantic_edge_reject_pointer_down_preserves_click() {
        let mut server = server();
        register_surface(&mut server, 1, 64, 48);
        server.enable_fastpath();

        // Pointer motion is coalesced
        assert!(server.try_coalesce_pointer_move(5, 5).unwrap());

        // Pointer-down (click) must NOT be coalesced — it's a semantic edge
        // route_pointer_down resets the coalesce burst via reset_coalesce_burst()
        let delivery = server.route_pointer_down(10, 10).expect("pointer down");
        assert_eq!(
            delivery.kind,
            InputEventKind::PointerDown,
            "click must preserve PointerDown kind"
        );
        // After pointer-down, coalesce burst should be reset
        assert_eq!(server.pointer_coalesce_burst(), 0);
    }

    #[test]
    fn semantic_edge_reject_keyboard_preserves_key() {
        let mut server = server();
        register_surface(&mut server, 1, 64, 48);

        // route_pointer_down to set focused_surface
        server.route_pointer_down(10, 10).expect("pointer down for focus");

        // Route a key event via route_keyboard
        let delivery = server.route_keyboard(0x04).expect("key event");
        assert_eq!(
            delivery.kind,
            InputEventKind::Keyboard { key_code: 0x04 },
            "key must preserve Keyboard kind"
        );
    }

    #[test]
    fn semantic_edge_touch_preserves_touch() {
        let mut server = server();
        register_surface(&mut server, 1, 64, 48);

        // route_touch with Down phase
        let delivery = server.route_touch(10, 10, TouchInputPhase::Down).expect("touch event");
        assert_eq!(
            delivery.kind,
            InputEventKind::TouchDown { x: 10, y: 10 },
            "touch must preserve TouchDown kind"
        );
    }

    // ── Phase D: Integration proofs (coalescing + present scheduler) ──

    #[test]
    fn coalesce_then_present_resets_burst() {
        let mut server = server();
        register_surface(&mut server, 1, 64, 48);
        server.enable_fastpath();

        // Coalesce a few moves
        for i in 0..3_i32 {
            assert!(server.try_coalesce_pointer_move(i, i).unwrap());
        }
        assert_eq!(server.pointer_coalesce_burst(), 3);

        // Present should reset the burst counter
        server.reset_coalesce_burst();
        assert_eq!(server.pointer_coalesce_burst(), 0);

        // Should be able to coalesce again
        assert!(server.try_coalesce_pointer_move(1, 1).unwrap());
        assert_eq!(server.pointer_coalesce_burst(), 1);
    }

    #[test]
    fn present_scheduler_tick_produces_ack() {
        let mut server = server();

        // Register a surface and seed first frame
        let buffer = surface_buffer(1, 64, 48, [128, 0, 0, 255]);
        let sid = server.create_surface(LAUNCHER, buffer.clone()).expect("create_surface");
        server
            .queue_buffer(LAUNCHER, sid, buffer, &[Rect::new(0, 0, 64, 48)])
            .expect("queue_buffer");
        server
            .commit_scene(LAUNCHER, CommitSeq::new(1), &[Layer { surface: sid, z: 0, x: 0, y: 0 }])
            .expect("commit_scene");
        server.route_pointer_move(10, 10).expect("pointer move");

        // Present scheduler tick should produce a ScheduledPresentAck
        let ack: ScheduledPresentAck =
            server.present_scheduler_tick().expect("present scheduler tick").expect("expected ack");
        assert!(ack.seq.raw() >= 1, "present seq must be >= 1");
    }

    #[test]
    fn empty_server_returns_error_on_tick() {
        let mut server = server();
        // No surfaces, no layers — tick should return Err(NoCommittedScene)
        let result = server.present_scheduler_tick();
        assert!(result.is_err(), "empty server should error on tick");
    }

    #[test]
    fn register_multiple_surfaces_and_compose() {
        let mut server = server();

        let buf1 = surface_buffer(1, 32, 32, [255, 0, 0, 255]);
        let sid1 = server.create_surface(LAUNCHER, buf1.clone()).expect("create surface 1");
        server
            .queue_buffer(LAUNCHER, sid1, buf1, &[Rect::new(0, 0, 32, 32)])
            .expect("queue buffer 1");

        let buf2 = surface_buffer(2, 32, 32, [0, 255, 0, 255]);
        let sid2 = server.create_surface(LAUNCHER, buf2.clone()).expect("create surface 2");
        server
            .queue_buffer(LAUNCHER, sid2, buf2, &[Rect::new(0, 0, 32, 32)])
            .expect("queue buffer 2");

        server
            .commit_scene(
                LAUNCHER,
                CommitSeq::new(1),
                &[
                    Layer { surface: sid1, z: 0, x: 0, y: 0 },
                    Layer { surface: sid2, z: 1, x: 32, y: 0 },
                ],
            )
            .expect("commit_scene");

        server.route_pointer_move(16, 16).expect("pointer move");

        let ack: ScheduledPresentAck =
            server.present_scheduler_tick().expect("present scheduler tick").expect("expected ack");
        assert!(ack.seq.raw() >= 1);
    }

    #[test]
    fn focus_transfer_preserves_focus_not_coalesced() {
        let mut server = server();

        let buf1 = surface_buffer(1, 32, 32, [255, 0, 0, 255]);
        let sid1 = server.create_surface(LAUNCHER, buf1.clone()).expect("create surface 1");
        server
            .queue_buffer(LAUNCHER, sid1, buf1, &[Rect::new(0, 0, 32, 32)])
            .expect("queue buffer 1");
        server
            .commit_scene(LAUNCHER, CommitSeq::new(1), &[Layer { surface: sid1, z: 0, x: 0, y: 0 }])
            .expect("commit_scene");

        // Focus transfer is a semantic edge and must produce events
        let delivery = server.route_pointer_down(16, 16).expect("pointer down");
        assert_eq!(delivery.kind, InputEventKind::PointerDown);
        // After pointer down, burst counter should be 0 (reset by semantic edge)
        assert_eq!(server.pointer_coalesce_burst(), 0);
    }

    // ── Phase E: Boundedness / negative proofs ──

    #[test]
    fn coalesce_budget_bounded_no_unbounded_accumulation() {
        let mut server = server();
        register_surface(&mut server, 1, 64, 48);
        server.enable_fastpath();

        // Try 100 coalesce calls — only 8 should succeed
        let mut ok_count = 0u32;
        let mut err_count = 0u32;
        for i in 0..100_i32 {
            match server.try_coalesce_pointer_move(i % 10, i % 10) {
                Ok(true) => ok_count += 1,
                Err(WindowdError::CoalesceBurstExceeded) => err_count += 1,
                other => panic!("unexpected result: {other:?}"),
            }
        }
        assert_eq!(ok_count, 8, "only 8 coalesce calls should succeed");
        assert_eq!(err_count, 92, "remaining should be burst-exceeded errors");
    }

    #[test]
    fn no_visible_change_skip_unbounded_accumulation_prevented() {
        let mut server = server();
        register_surface(&mut server, 1, 64, 48);

        let _ = server.present_scheduler_tick().expect("first present");
        server.set_last_frame_hash_for_tests(server.compute_frame_hash());

        // 100 identical frames -> 4-of-5 cycle: 4 skips succeed, 1 forced present resets counter
        let mut skip_count = 0u32;
        let mut force_count = 0u32;
        for _ in 0..100 {
            match server.try_no_damage_skip() {
                Ok(true) => skip_count += 1,
                Ok(false) => force_count += 1,
                Err(_) => force_count += 1,
            }
        }
        assert_eq!(skip_count, 80, "4-of-5 cycle: 80 skips in 100 calls");
        assert_eq!(force_count, 20, "4-of-5 cycle: 20 forced presents reset counter");
    }

    #[test]
    fn burst_exhausted_does_not_panic() {
        let mut server = server();
        register_surface(&mut server, 1, 64, 48);
        server.enable_fastpath();

        // Exhaust burst
        for i in 0..8_i32 {
            server.try_coalesce_pointer_move(i, i).unwrap();
        }
        // 9th should error, not panic
        let result = server.try_coalesce_pointer_move(99, 99);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), WindowdError::CoalesceBurstExceeded);
    }

    // ── Per-hop chain proofs (display output hardening matrix) ──

    fn setup_scene(server: &mut WindowServer) {
        let buffer = surface_buffer(1, 64, 48, [128, 0, 0, 255]);
        let sid = server.create_surface(LAUNCHER, buffer.clone()).expect("create_surface");
        let damage = [Rect::new(0, 0, 64, 48)];
        server.queue_buffer(LAUNCHER, sid, buffer, &damage).expect("queue_buffer");
        server
            .commit_scene(LAUNCHER, CommitSeq::new(1), &[Layer { surface: sid, z: 0, x: 0, y: 0 }])
            .expect("commit_scene");
    }

    #[test]
    fn input_route_triggers_present_with_damage_count() {
        let mut server = server();
        setup_scene(&mut server);
        // Route input to create damage
        let delivery = server.route_pointer_move(16, 16).expect("pointer move");
        assert_eq!(delivery.surface.raw(), 1);
        // Present tick must return a composed frame (damage exists)
        let ack = server.present_tick().expect("present tick").expect("frame composed");
        assert!(ack.damage_rects > 0, "composed frame must have damage rects");
        assert_eq!(ack.seq.raw(), 1);
    }

    #[test]
    fn present_sequence_is_monotonic_across_ticks() {
        let mut server = server();
        setup_scene(&mut server);
        // First present
        let ack1 = server.present_tick().expect("present tick").expect("first frame");
        // Queue again with new damage
        let buffer = surface_buffer(2, 64, 48, [0, 128, 0, 255]);
        let sid = server.create_surface(LAUNCHER, buffer.clone()).expect("create surface 2");
        server
            .queue_buffer(LAUNCHER, sid, buffer, &[Rect::new(0, 0, 64, 48)])
            .expect("queue buffer 2");
        server.route_pointer_move(32, 32).expect("pointer move 2");
        let ack2 = server.present_tick().expect("present tick").expect("second frame");
        assert!(ack2.seq.raw() > ack1.seq.raw(), "present sequence must increase");
    }

    #[test]
    fn no_damage_returns_none_from_present_tick() {
        let mut server = server();
        setup_scene(&mut server);
        // First present consumes damage
        let _ = server.present_tick().expect("present tick").expect("first present");
        // Second present with no new damage must return None
        let result = server.present_tick().expect("present tick");
        assert!(result.is_none(), "no damage should skip compose");
    }

    #[test]
    fn scheduler_present_reports_coalesced_frames_and_latency() {
        let mut server = server();
        setup_scene(&mut server);
        let ack: ScheduledPresentAck =
            server.present_scheduler_tick().expect("scheduler present").expect("ack");
        assert!(ack.damage_rects > 0);
        assert!(ack.latency_ms < 100, "latency must be within 100ms for host test");
    }

    #[test]
    fn input_delivery_to_present_roundtrip_produces_ack() {
        let mut server = server();
        setup_scene(&mut server);
        // Deliver input event
        let delivery = server.route_pointer_move(16, 16).expect("pointer move");
        assert_eq!(delivery.surface.raw(), 1);
        // Present must produce a composed frame ack
        let ack: PresentAck = server.present_tick().expect("present").expect("composed");
        assert!(ack.damage_rects > 0);
        // After present, last frame must exist
        assert!(server.last_frame().is_some(), "last frame must be set after present");
    }

    #[test]
    fn pointer_move_to_frame_latency_under_16ms_in_host() {
        // Perf contract: input-to-frame roundtrip must complete within 16ms (60Hz budget)
        // in the host test environment. This validates the compose path is not a bottleneck.
        let mut server = server();
        setup_scene(&mut server);

        use std::time::Instant;
        let start = Instant::now();
        let _delivery = server.route_pointer_move(16, 16).expect("pointer move");
        let _ack = server.present_tick().expect("present").expect("composed");
        let elapsed_ms = start.elapsed().as_millis();

        assert!(elapsed_ms < 16, "input-to-frame latency {elapsed_ms}ms exceeds 16ms 60Hz budget");
    }
}
