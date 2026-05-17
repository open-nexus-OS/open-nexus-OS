// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Interactive OS start contract tests
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal test
//! TEST_COVERAGE: `cargo test -p nx --test interactive_os_startup`
//! ADR: docs/adr/0017-service-architecture.md

use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tools/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn read_repo_file(path: &str) -> String {
    fs::read_to_string(repo_root().join(path)).unwrap_or_else(|err| panic!("read {path}: {err}"))
}

fn read_map_symbol(map: &str, symbol: &str) -> usize {
    let line = map
        .lines()
        .find(|line| line.contains(symbol))
        .unwrap_or_else(|| panic!("missing linker map symbol {symbol}"));
    let raw = line
        .split_whitespace()
        .next()
        .unwrap_or_else(|| panic!("missing address for linker map symbol {symbol}"));
    usize::from_str_radix(raw, 16)
        .unwrap_or_else(|err| panic!("parse address for linker map symbol {symbol}: {err}"))
}

#[test]
fn make_run_uses_interactive_minimal_runtime_mode_without_rebuild() {
    let makefile = read_repo_file("Makefile");

    assert!(
        makefile.contains("NEXUS_SKIP_BUILD=1"),
        "`make run` must reuse `make build` artifacts"
    );
    assert!(
        makefile.contains("QEMU_SESSION_MODE=interactive")
            && makefile.contains("QEMU_MARKER_LEVEL=full"),
        "`make run` must select the interactive-minimal runner posture"
    );
    assert!(
        makefile.contains("NEXUS_SELFTEST_MODE=interactive-full")
            && makefile.contains("NEXUS_SELFTEST_PROFILE=bringup"),
        "`make run` must switch the guest at runtime without recompiling"
    );
    assert!(
        makefile.contains("RUN_UNTIL_MARKER=0") && makefile.contains("./scripts/run-qemu-rv64.sh"),
        "`make run` must launch the long-lived interactive runner, not the proof harness"
    );
}

#[test]
fn just_start_builds_then_runs_full_interactive_breadcrumbs() {
    let justfile = read_repo_file("justfile");

    assert!(justfile.contains("start *args:"), "`just start` recipe must exist");
    assert!(
        justfile.contains("make build"),
        "`just start` must perform its own build before launching"
    );
    assert!(
        justfile.contains("QEMU_MARKER_LEVEL=full")
            && justfile.contains("NEXUS_SELFTEST_MODE=interactive-full"),
        "`just start` must request the richer interactive breadcrumb ladder"
    );
    assert!(
        justfile.contains("NEXUS_SELFTEST_PROFILE=bringup")
            && justfile.contains("RUN_UNTIL_MARKER=0"),
        "`just start` must keep the interactive guest alive in bringup profile"
    );
    assert!(
        justfile.contains(concat!(
            "QEMU_PROOF_POINTER_SOURCE=$",
            "{",
            "QEMU_PROOF_POINTER_SOURCE:-mouse",
            "}"
        )),
        "`just start` must default to the relative mouse path while still allowing an explicit pointer-source override"
    );
    assert!(
        justfile.contains("scripts/run-qemu-rv64.sh {{args}}"),
        "`just start` must launch the same live QEMU runner as `make run` and forward QEMU args"
    );
}

#[test]
fn run_qemu_runner_passes_runtime_mode_and_profile_via_fw_cfg() {
    let runner = read_repo_file("scripts/run-qemu-rv64.sh");

    for needle in [
        "QEMU_SESSION_MODE",
        "QEMU_MARKER_LEVEL",
        "NEXUS_SELFTEST_MODE",
        "NEXUS_SELFTEST_PROFILE",
        "opt/org.open-nexus/selftest-mode",
        "opt/org.open-nexus/selftest-profile",
    ] {
        assert!(runner.contains(needle), "`scripts/run-qemu-rv64.sh` must contain `{needle}`");
    }
    assert!(
        runner.contains("RUN_TIMEOUT=0") || runner.contains("\"$RUN_TIMEOUT\" == \"0\""),
        "`scripts/run-qemu-rv64.sh` must allow no-timeout interactive sessions"
    );
}

#[test]
fn interactive_minimal_timeout_is_only_accepted_after_scene_ready() {
    let runner = read_repo_file("scripts/run-qemu-rv64.sh");

    for needle in [
        "INTERACTIVE_READY_SENTINEL",
        "windowd: interactive scene ready",
        "QEMU reached interactive scene readiness before timeout; accepting time-capped make run",
        "\"$status\" -eq 124",
        "\"$QEMU_SESSION_MODE\" == \"interactive\"",
        "\"$QEMU_MARKER_LEVEL\" == \"minimal\"",
        "| tee >(monitor_uart) >(monitor_agent_uart) \\",
    ] {
        assert!(runner.contains(needle), "`scripts/run-qemu-rv64.sh` must contain `{needle}`");
    }
}

#[test]
fn interactive_qemu_exposes_keyboard_and_pointer_devices() {
    let runner = read_repo_file("scripts/run-qemu-rv64.sh");

    assert!(
        runner.contains("QEMU_SESSION_MODE")
            && runner.contains("virtio-keyboard-device")
            && runner.contains("virtio-tablet-device")
            && runner.contains("virtio-mouse-device"),
        "interactive QEMU display starts must expose keyboard plus absolute and relative pointer devices"
    );
    assert!(
        runner.contains("grab-on-hover=on")
            && runner.contains("show-tabs=off")
            && runner.contains("resolve_qemu_display_backend"),
        "interactive GTK starts must prefer pointer-capture-friendly display defaults for the live host path"
    );
}

#[test]
fn just_start_defaults_to_mouse_relative_pointer_source_for_live_host_input() {
    let justfile = read_repo_file("justfile");
    let runner = read_repo_file("scripts/run-qemu-rv64.sh");

    assert!(
        justfile.contains(concat!(
            "QEMU_PROOF_POINTER_SOURCE=$",
            "{",
            "QEMU_PROOF_POINTER_SOURCE:-mouse",
            "}"
        )),
        "`just start` must default to mouse-relative input so GTK host movement produces hidrawd raw ingress"
    );
    assert!(
        runner.contains("QEMU_PROOF_POINTER_SOURCE")
            && runner.contains("NEXUS_PROFILE_INPUT_TOUCH=0")
            && runner.contains("NEXUS_PROFILE_INPUT_MOUSE=1")
            && runner.contains("-device virtio-mouse-device"),
        "the runner must map the selected mouse pointer source to a single relative virtio mouse device"
    );
}

#[test]
fn init_and_policy_wire_virtio_input_mmio_for_hidrawd_owner() {
    let init = read_repo_file("source/init/nexus-init/src/os_payload.rs");
    let policy = read_repo_file("policies/base.toml");

    assert!(
        init.contains("VIRTIO_DEVICE_ID_INPUT: u32 = 18")
            && init.contains("INPUT_MMIO_CAP_SLOT_BASE")
            && init.contains("\"hidrawd\"")
            && init.contains("device.mmio.input"),
        "init-lite must discover virtio-input MMIO windows and transfer bounded caps"
    );
    assert!(
        policy.contains("\"hidrawd\" = [") && policy.contains("\"device.mmio.input\""),
        "policy must explicitly authorize hidrawd as the live input MMIO owner"
    );
}

#[test]
fn qemu_test_harness_stays_in_proof_mode_for_visible_bootstrap() {
    let harness = read_repo_file("scripts/qemu-test.sh");

    assert!(
        harness.contains("QEMU_SESSION_MODE=proof") && harness.contains("QEMU_MARKER_LEVEL=proof"),
        "`scripts/qemu-test.sh` must force proof runner semantics"
    );
    assert!(
        harness.contains(concat!("NEXUS_SELFTEST_MODE=$", "{", "NEXUS_SELFTEST_MODE:-proof", "}"))
            && harness.contains(concat!(
                "NEXUS_SELFTEST_PROFILE=$",
                "{",
                "NEXUS_SELFTEST_PROFILE:-bringup",
                "}"
            )),
        "`scripts/qemu-test.sh` visible-bootstrap path must request proof-only guest semantics without broad runtime phases"
    );
    assert!(
        harness.contains("\"init: start fbdevd\"")
            && harness.contains("\"SELFTEST: ui visible input ok\"")
            && harness.contains("\"SELFTEST: ui visible wheel ok\"")
            && !harness.contains("\"${expected_sequence[@]:0:$(( ${#expected_sequence[@]} - 7 ))}\""),
        "visible-bootstrap expected markers must be a focused display/input ladder, not the broad full-profile ladder"
    );
}

#[test]
fn qemu_test_harness_only_requires_hidrawd_live_payload_when_display_bootstrap_is_on() {
    let harness = read_repo_file("scripts/qemu-test.sh");

    assert!(
        harness.contains("declare -a INPUT_STARTUP_MARKERS=(")
            && harness.contains("\"touchd: os service payload ready\"")
            && harness.contains("\"inputd: os service payload ready\""),
        "qemu-test must build input-startup markers from a shared list"
    );
    assert!(
        harness.contains("if [[ \"${NEXUS_DISPLAY_BOOTSTRAP:-0}\" == \"1\" ]]; then")
            && harness.contains("INPUT_STARTUP_MARKERS+=(\"hidrawd: os service payload ready\")")
            && harness.contains("PHASE_END_MARKER[\"input-startup\"]=\"inputd: os service payload ready\""),
        "headless profiles must not fail on hidrawd live payload markers when the runner did not expose virtio-input devices"
    );
    assert!(
        harness.contains("\"${INPUT_STARTUP_MARKERS[@]}\""),
        "both the full marker ladder and RUN_PHASE=input-startup slice must reuse the same display-aware marker list"
    );
}

#[test]
fn proof_manifest_allows_inputd_startup_markers_outside_visible_bootstrap() {
    let ui_markers = read_repo_file("source/apps/selftest-client/proof-manifest/markers/ui.toml");

    assert!(
        ui_markers.contains("[marker.\"inputd: ready\"]")
            && !ui_markers.contains(
                "[marker.\"inputd: ready\"]\nphase = \"end\"\nemit_when = { profile = \"visible-bootstrap\" }"
            ),
        "proof-manifest must accept `inputd: ready` in non-visible profiles once inputd is part of the normal service set"
    );
    assert!(
        ui_markers.contains("[marker.\"inputd: keymap=de\"]")
            && !ui_markers.contains(
                "[marker.\"inputd: keymap=de\"]\nphase = \"end\"\nemit_when = { profile = \"visible-bootstrap\" }"
            ),
        "proof-manifest must accept the deterministic inputd keymap marker outside visible-bootstrap when the service starts in other harness profiles"
    );
}

#[test]
fn interactive_markers_are_breadcrumbs_not_proof_markers() {
    let ui_markers = read_repo_file("source/apps/selftest-client/proof-manifest/markers/ui.toml");
    let windowd_markers = read_repo_file("source/services/windowd/src/markers.rs");
    let end_phase = read_repo_file("source/apps/selftest-client/src/os_lite/phases/end.rs");

    for marker in [
        "windowd: interactive scene ready",
        "windowd: interactive click target ready",
        "windowd: interactive keyboard target ready",
        "windowd: interactive full markers on",
    ] {
        assert!(
            ui_markers.contains(&format!("[marker.\"{marker}\"]")),
            "`ui.toml` must declare `{marker}`"
        );
        assert!(
            !marker.starts_with("SELFTEST:"),
            "interactive breadcrumbs must not masquerade as proof markers"
        );
        assert!(
            windowd_markers.contains(marker),
            "`windowd` marker surface must export `{marker}`"
        );
    }
    assert!(
        end_phase.contains("RuntimeMode::InteractiveMinimal")
            && end_phase.contains("RuntimeMode::InteractiveFull")
            && end_phase.contains("proof_completed = false"),
        "interactive starts must remain breadcrumbs and must not close the deterministic proof"
    );
}

#[test]
fn task_and_rfc_require_real_live_mouse_and_keyboard_before_closure() {
    let task = read_repo_file(
        "tasks/TASK-0253-input-v1_0b-os-hidrawd-touchd-inputd-ime-hooks-selftests.md",
    );
    let rfc = read_repo_file(
        "docs/rfcs/RFC-0053-input-v1_0b-os-qemu-live-input-hidrawd-touchd-inputd-contract.md",
    );

    for needle in [
        "one visible pixel that follows routed pointer motion",
        "bottom-left square that changes color on hover and click",
        "right-side square changes color on keyboard input",
        "host-driven live OS start proves the same scene",
    ] {
        assert!(task.contains(needle), "`TASK-0253` must contain `{needle}`");
    }
    for needle in [
        "real mouse movement",
        "hover/click rectangle reaction",
        "keyboard rectangle reaction",
        "Final host-driven live QEMU proof",
    ] {
        assert!(rfc.contains(needle), "`RFC-0053` must contain `{needle}`");
    }
}

#[test]
fn kernel_linker_keeps_private_selftest_stack() {
    let linker_script = read_repo_file("source/kernel/neuron-boot/kernel.ld");
    let map = read_repo_file("neuron-boot.map");

    assert!(
        linker_script.contains("KEEP(*(.bss.selftest_stack_body))"),
        "private selftest stack storage must survive linker garbage collection"
    );
    let guard_lo = read_map_symbol(&map, "__selftest_stack_guard_lo");
    let base = read_map_symbol(&map, "__selftest_stack_base");
    let top = read_map_symbol(&map, "__selftest_stack_top");

    assert_eq!(base - guard_lo, 0x1000, "selftest stack low guard must be one page");
    assert_eq!(top - base, 0x8000, "private selftest stack must be 32 KiB");
}

#[test]
fn runtime_profile_waits_for_late_fw_cfg_capability() {
    let boot_cfg = read_repo_file("source/apps/selftest-client/src/os_lite/boot_cfg.rs");
    let profile = read_repo_file("source/apps/selftest-client/src/os_lite/profile.rs");

    assert!(
        boot_cfg.contains("runtime_mode_with_retry")
            && boot_cfg.contains("runtime_profile_with_retry")
            && boot_cfg.contains("RUNTIME_CFG_RETRY_YIELDS"),
        "runtime boot config must tolerate init-lite transferring fw_cfg after process spawn"
    );
    assert!(
        !boot_cfg.contains("MAP_STATE_UNAVAILABLE"),
        "a transient missing fw_cfg cap must not be cached as permanent"
    );
    assert!(
        profile.contains("boot_cfg::runtime_profile_with_retry()")
            && profile.contains("boot_cfg::runtime_mode_with_retry()"),
        "profile selection must use the bounded fw_cfg retry path"
    );
}

#[test]
fn live_framebuffer_service_keeps_dedicated_vmo_budget_and_error_label() {
    let mm = read_repo_file("source/kernel/neuron/src/mm/mod.rs");
    let fbdevd_error = read_repo_file("source/services/fbdevd/src/error.rs");
    let framebuffer = read_repo_file("source/services/fbdevd/src/backend/framebuffer.rs");

    assert!(
        mm.contains("USER_VMO_ARENA_LEN: usize = 32 * 1024 * 1024"),
        "service-owned ramfb scanout must have enough VMO arena headroom after service bring-up"
    );
    assert!(
        mm.contains("USER_VMO_ARENA_BASE: usize = 0x8180_0000")
            && mm.contains("KERNEL_PAGE_POOL_BASE: usize = 0x80c0_0000")
            && mm.contains("KERNEL_PAGE_POOL_LEN: usize = 8 * 1024 * 1024"),
        "VMO arena and kernel page-pool windows must stay explicit and non-overlapping"
    );
    assert!(
        fbdevd_error.contains("fbdevd: fail framebuffer-vmo")
            && framebuffer.contains("validate_framebuffer_cap("),
        "framebuffer allocation failures must emit a stable fbdevd label and validate the framebuffer capability before scanout"
    );
}

#[test]
fn fbdevd_service_owns_initial_committed_frame() {
    let fbdevd = read_repo_file("source/services/fbdevd/src/os_lite.rs");

    assert!(
        fbdevd.contains("FramebufferOwner::allocate(mode)")
            && fbdevd.contains("configure_ramfb(framebuffer.base, mode)")
            && fbdevd.contains("register_framebuffer_with_windowd(")
            && fbdevd.contains("encode_send_composed_frame_vmo()")
            && fbdevd.contains("decode_status(&frame, OP_SEND_COMPOSED_FRAME_VMO) == Some(STATUS_OK)"),
        "fbdevd must own scanout setup and register its framebuffer with windowd before observer polling proceeds"
    );
}

#[test]
fn fbdevd_framebuffer_writer_clamps_rows_to_valid_handoff_geometry() {
    let framebuffer = read_repo_file("source/services/fbdevd/src/backend/framebuffer.rs");

    assert!(
        framebuffer.contains("let row_len = self.mode.stride as usize;")
            && framebuffer
                .contains("let mut row = [0u8; windowd::VISIBLE_BOOTSTRAP_WIDTH as usize * 4];")
            && framebuffer.contains("handoff")
            && framebuffer.contains(".copy_row(y, &mut row[..row_len])")
            && framebuffer.contains("validate_handoff(handoff)?;")
            && framebuffer.contains("vmo_write(self.handle, offset, &row[..row_len])"),
        "fbdevd row writes must stay bounded to the validated framebuffer geometry"
    );
}

#[test]
fn hidrawd_service_owns_periodic_chain_fps_telemetry() {
    let hidrawd = read_repo_file("source/services/hidrawd/src/os_lite.rs");

    for needle in [
        "fps: hidrawd ingress_hz=",
        "sent_hz={}",
        "raw_batches={}",
        "wire_batches={}",
        "wire_skip={}",
        "raw_events={}",
        "norm_events={}",
        "kbd_batches={}",
        "mouse_rel={}",
        "tablet_abs={}",
        "touch_abs={}",
        "send_fail={}",
        "rebinds={}",
        "idle_yields={}",
    ] {
        assert!(hidrawd.contains(needle), "`hidrawd` service telemetry must include `{needle}`");
    }
}

#[test]
fn hidrawd_live_classification_keeps_keyboard_as_explicit_device_class() {
    let hidrawd = read_repo_file("source/services/hidrawd/src/os_lite.rs");

    assert!(
        hidrawd.contains("enum LiveDeviceClass")
            && hidrawd.contains("LiveDeviceClass::Keyboard")
            && hidrawd.contains("service.register_keyboard(self.device_id)")
            && hidrawd.contains("hidrawd: device kbd"),
        "hidrawd live ingress hardening must keep keyboard as an explicit confirmed device class so pointer-source work cannot silently drop keyboard registration"
    );
}

#[test]
fn hidrawd_upgrades_mouse_button_only_batches_out_of_keyboard_safe_probe_mode() {
    let hidrawd = read_repo_file("source/services/hidrawd/src/os_lite.rs");

    assert!(
        hidrawd.contains("event.kind() == RawIngressEventKind::Key && event.code() >= 0x110")
            && hidrawd.contains("LiveDeviceClass::Pointer(PointerSource::MouseRelative)"),
        "hidrawd live classification must upgrade button-only mouse batches into a pointer source before relative motion arrives, or click-only regressions will slip past host proofs"
    );
}

#[test]
fn proof_end_phase_reports_v2a_smoke_failures_explicitly() {
    let end_phase = read_repo_file("source/apps/selftest-client/src/os_lite/phases/end.rs");

    assert!(
        end_phase.contains("windowd: v2a smoke err")
            && end_phase.contains("windowd: v2a scheduler off")
            && end_phase.contains("windowd: v2a input off")
            && end_phase.contains("windowd: v2a click off"),
        "proof end phase must emit explicit v2a failure breadcrumbs so missing markers map to the failing gate before another QEMU rerun"
    );
}

#[test]
fn inputd_service_owns_periodic_chain_fps_telemetry() {
    let inputd = read_repo_file("source/services/inputd/src/os_lite.rs");

    for needle in [
        "fps: inputd recv_hz=",
        "hid_ok_hz={}",
        "poll_hz={}",
        "malformed={}",
        "hid_unsupported={}",
        "overflow={}",
        "frame_malformed={}",
        "wire_count={}",
        "wire_kind={}",
        "wire_source={}",
        "wire_event={}",
        "wire_mode={}",
        "abs_cal={}",
        "abs_axis={}",
        "apply_ovf={}",
        "deliver_ovf={}",
        "dispatch={}",
        "delivered={}",
        "ptr_d={}",
        "kbd_d={}",
        "ptr_deliv={}",
        "kbd_deliv={}",
        "idle_yields={}",
    ] {
        assert!(inputd.contains(needle), "`inputd` service telemetry must include `{needle}`");
    }
}

#[test]
fn inputd_route_contract_has_named_bind_and_deterministic_fallback() {
    let inputd = read_repo_file("source/services/inputd/src/os_lite.rs");

    assert!(
        inputd.contains("KernelServer::new_for(\"inputd\")")
            && inputd.contains("inputd: route fallback")
            && inputd.contains("KernelServer::new_with_slots(3, 4)"),
        "inputd must try named routing first and fail closed to deterministic service slots"
    );
    assert!(
        inputd.contains("inputd: fallback slots 3/4") && !inputd.contains("agent8cde1d"),
        "inputd fallback route must publish compact stable slot evidence without oversized agent debug logs"
    );
}

#[test]
fn inputd_os_lite_drains_stale_dispatch_log_before_live_batch_accounting() {
    let inputd = read_repo_file("source/services/inputd/src/os_lite.rs");

    assert!(
        inputd.contains("self.input.clear_dispatches();"),
        "inputd OS-lite path must drain stale dispatch history so live keyboard/pointer batches do not degrade into overflow-only delivery"
    );
}

#[test]
fn inputd_os_lite_avoids_per_batch_delivery_allocations_in_live_path() {
    let inputd = read_repo_file("source/services/inputd/src/os_lite.rs");

    assert!(
        inputd.contains("self.input.apply_hid_batch_in_place(&hid_batch).is_err()")
            && inputd.contains("let dispatches = self.input.recent_dispatches();"),
        "inputd live path must use in-place dispatch accounting rather than allocating a fresh dispatch vector for every batch"
    );
    assert!(
        inputd.contains(".drain_input_events(self.launcher, self.surface)")
            && !inputd.contains("self.input.router_mut().take_input_events(self.launcher, self.surface)"),
        "inputd live path must drain windowd input queues without allocating per-batch delivery vectors"
    );
}

#[test]
fn hidrawd_waits_for_mmio_caps_before_emitting_ready() {
    let hidrawd = read_repo_file("source/services/hidrawd/src/os_lite.rs");

    assert!(
        hidrawd.contains("let mut ready_emitted = false")
            && hidrawd.contains("if live_devices.is_empty()")
            && hidrawd.contains("let mut reprobe_budget = 0usize;")
            && hidrawd.contains("if reprobe_budget == 0")
            && hidrawd.contains("live_devices = open_live_devices(&mut missing_slots_logged);")
            && hidrawd.contains("reprobe_budget = EMPTY_DEVICE_REPROBE_YIELDS;"),
        "hidrawd must retry live device open with a bounded reprobe budget when init-lite transfers MMIO caps after process spawn"
    );
    assert!(
        hidrawd.contains("if !ready_emitted && !live_devices.is_empty() && client.is_some()")
            && hidrawd.contains("hidrawd: os service payload ready"),
        "hidrawd ready markers must only appear after both live devices and inputd route are real"
    );
}

#[test]
fn init_wires_input_caps_and_routes_for_owner_chain() {
    let init = read_repo_file("source/init/nexus-init/src/os_payload.rs");

    assert!(
        init.contains("let input_req =")
            && init.contains("let input_rsp =")
            && init.contains("init: hidrawd inputd slots send=0x")
            && init.contains("init: inputd slots recv=0x"),
        "init-lite must provision dedicated request/response endpoints for hidrawd -> inputd"
    );
    assert!(
        init.contains("init: selftest inputd slots send=0x")
            && init.contains("name == b\"inputd\""),
        "routing service must expose explicit lookup entries for input owner chain and observer access"
    );
}

#[test]
fn init_wires_fbdevd_caps_and_routes_for_service_owned_display_observer_chain() {
    let init = read_repo_file("source/init/nexus-init/src/os_payload.rs");
    let observer =
        read_repo_file("source/apps/selftest-client/src/os_lite/display_bootstrap_observer.rs");

    assert!(
        init.contains("let fbdev_req =")
            && init.contains("let fbdev_rsp =")
            && init.contains("init: fbdevd slots recv=0x"),
        "init-lite must provision dedicated request/response endpoints for fbdevd's own service loop"
    );
    assert!(
        init.contains("init: selftest fbdevd slots send=0x")
            && init.contains("name == b\"fbdevd\""),
        "routing service must expose explicit fbdevd lookup entries for observer-only clients"
    );
    assert!(
        observer.contains("route_with_retry(\"fbdevd\")")
            && observer.contains("cached_reply_client()"),
        "selftest-client must stay observer-only and query fbdevd through routed service IPC plus @reply"
    );
}

#[test]
fn rng_selftest_tracks_current_init_lite_rngd_slot_pair() {
    let init = read_repo_file("source/init/nexus-init/src/os_payload.rs");
    let rng_probe = read_repo_file("source/apps/selftest-client/src/os_lite/probes/rng.rs");

    assert!(
        init.contains("init: selftest rngd slots send=0x"),
        "init-lite must log and own the rngd slot distribution that the selftest proof depends on"
    );
    assert!(
        rng_probe.contains("const RNGD_SEND_SLOT: u32 = 0x1f;")
            && rng_probe.contains("const RNGD_RECV_SLOT: u32 = 0x20;")
            && !rng_probe.contains("const RNGD_SEND_SLOT: u32 = 0x1e;")
            && !rng_probe.contains("const RNGD_RECV_SLOT: u32 = 0x1f;"),
        "rng selftest must stay aligned with the current init-lite rngd slot pair so slot drift fails on host before QEMU"
    );
}

#[test]
fn virtio_input_driver_tolerates_missing_optional_event_bitmaps() {
    let driver = read_repo_file("source/drivers/input/virtio-input/src/lib.rs");

    assert!(
        driver.contains("config_unavailable_as_empty")
            && driver.contains("detect_role_defaults_keyboard_when_optional_event_bitmaps_are_absent"),
        "virtio-input role detection must treat missing optional event-bit configs as keyboard-safe, not fatal"
    );
}

#[test]
fn visible_bootstrap_runner_injects_real_input_through_qmp() {
    let harness = read_repo_file("scripts/qemu-test.sh");
    let runner = read_repo_file("scripts/run-qemu-rv64.sh");
    let injector = read_repo_file("tools/qmp_visible_input_inject.py");

    assert!(
        harness.contains("QEMU_INPUT_AUTOINJECT=1"),
        "visible-bootstrap proof must enable deterministic real-input injection instead of local selftest replay"
    );
    assert!(
        harness.contains(concat!(
            "QEMU_INPUT_AUTOINJECT=\"$",
            "{",
            "QEMU_INPUT_AUTOINJECT:-0",
            "}\""
        ))
            && harness.contains("QEMU_SESSION_MODE=\"$QEMU_SESSION_MODE\"")
            && harness.contains("QEMU_MARKER_LEVEL=\"$QEMU_MARKER_LEVEL\"")
            && harness.contains("RUN_UNTIL_MARKER=\"SELFTEST: ui v2b assets ok\""),
        "qemu-test must forward visible-bootstrap autoinject/session env into run-qemu and stop on the terminal wheel-proof marker so the proof runner cannot silently disable real input injection or linger past closure"
    );
    assert!(
        runner.contains(concat!("QEMU_INPUT_AUTOINJECT=$", "{", "QEMU_INPUT_AUTOINJECT:-0", "}"))
            && runner.contains("-qmp \"unix:$QEMU_QMP_SOCKET,server=on,wait=off\"")
            && runner.contains("python3 \"$QEMU_INPUT_INJECTOR_PY\" \"$QEMU_QMP_SOCKET\"")
            && runner.contains("QEMU_PROOF_POINTER_SOURCE")
            && runner.contains("unsupported QEMU_PROOF_POINTER_SOURCE"),
        "run-qemu runner must expose a QMP socket, launch the deterministic input injector when requested, and honor a proof-only single-pointer source override"
    );
    assert!(
        injector.contains("\"execute\": \"input-send-event\"")
            && injector.contains("env_flag(\"NEXUS_PROFILE_INPUT_TOUCH\")")
            && injector.contains("env_flag(\"NEXUS_PROFILE_INPUT_MOUSE\")")
            && injector.contains("env_flag(\"NEXUS_PROFILE_INPUT_KBD\")")
            && injector.contains("QEMU_SESSION_MODE")
            && injector.contains("wait_for_uart_marker")
            && injector.contains("QEMU_UART_LOG_PATH")
            && injector.contains("QEMU_INPUT_INJECT_WAIT_MARKER")
            && injector.contains("SELFTEST: ui visible present ok")
            && injector.contains("proof_prefers_single_pointer_source")
            && injector.contains("QEMU_ABS_MAX = 32767")
            && injector.contains("HOVER_TARGET_ROUTE_X = 8")
            && injector.contains("HOVER_TARGET_ROUTE_Y = 40")
            && injector.contains("CURSOR_START_ROUTE_X = 24")
            && injector.contains("CURSOR_START_ROUTE_Y = 12")
            && injector.contains("REL_STEP_LIMIT = 256")
            && injector.contains("bounded_rel_steps")
            && injector.contains("POINTER_DOWN_HOLD_S = 0.25")
            && injector.contains("KEY_DOWN_HOLD_S = 0.25")
            && injector.contains("WHEEL_PULSE_SETTLE_S = 0.20")
            && injector.contains("\"type\": \"rel\"")
            && injector.contains("\"type\": \"abs\"")
            && injector.contains("qemu_abs_value(target_display_x, VISIBLE_DISPLAY_WIDTH)")
            && injector.contains("qemu_abs_value(target_display_y, VISIBLE_DISPLAY_HEIGHT)")
            && injector.contains("\"button\": \"left\"")
            && injector.contains("\"button\": \"wheel-up\"")
            && injector.contains("\"type\": \"key\"")
            && injector.contains("\"data\": \"a\"")
            && injector.contains("wheel injection sent"),
        "QMP injector must drive real pointer move to the hover target, click, keyboard input, and wheel input into the guest while keeping proof-mode mixed pointer injection source-isolated"
    );
    assert!(
        injector.contains("reply[\"error\"].get(\"class\") == \"DeviceNotFound\"")
            && injector.contains("pointer device fallback without explicit qmp device")
            && injector.contains("requested_device")
            && injector.contains("fallback_arguments = {\"events\": events}")
            && injector.contains("console: int | None = None"),
        "QMP injector must degrade cleanly when a QEMU build does not expose `video0`, and it must not hardcode the `console` argument on QEMU builds that reject it"
    );
}

#[test]
fn visible_bootstrap_runner_derives_qemu_input_devices_from_systemui_profile() {
    let harness =
        read_repo_file("source/apps/selftest-client/proof-manifest/profiles/harness.toml");
    let runner = read_repo_file("scripts/run-qemu-rv64.sh");
    let helper = read_repo_file("tools/systemui_profile_qemu_devices.py");
    let desktop_profile =
        read_repo_file("source/services/systemui/manifests/profiles/desktop/profile.toml");

    assert!(
        harness.contains("NEXUS_SYSTEMUI_PROFILE = \"desktop\""),
        "visible-bootstrap harness profile must forward an explicit SystemUI profile so QEMU input device selection is profile-owned"
    );
    assert!(
        harness.contains("QEMU_PROOF_POINTER_SOURCE = \"mouse\""),
        "visible-bootstrap harness profile must pin a single proof pointer source instead of exposing mixed mouse+tablet devices to the automated proof lane"
    );
    assert!(
        runner.contains("QEMU_PROFILE_INPUT_HELPER")
            && runner.contains("load_systemui_input_profile")
            && runner.contains(concat!(
                "NEXUS_SYSTEMUI_PROFILE=$",
                "{",
                "NEXUS_SYSTEMUI_PROFILE:-desktop",
                "}"
            ))
            && runner.contains(concat!(
                "QEMU_PROOF_POINTER_SOURCE=$",
                "{",
                "QEMU_PROOF_POINTER_SOURCE:-",
                "}"
            ))
            && runner.contains("NEXUS_PROFILE_INPUT_TOUCH")
            && runner.contains("NEXUS_PROFILE_INPUT_MOUSE")
            && runner.contains("NEXUS_PROFILE_INPUT_KBD")
            && runner.contains("-device virtio-tablet-device")
            && runner.contains("-device virtio-mouse-device")
            && runner.contains("-device virtio-keyboard-device"),
        "run-qemu must derive visible input devices from the selected SystemUI profile and then allow a proof-only single-pointer override instead of hardcoding a mixed mouse+tablet set"
    );
    assert!(
        helper.contains("tomllib")
            && helper.contains("NEXUS_PROFILE_INPUT_TOUCH")
            && helper.contains("NEXUS_PROFILE_INPUT_MOUSE")
            && helper.contains("NEXUS_PROFILE_INPUT_KBD"),
        "profile helper must parse the SystemUI profile TOML and export input capability env vars"
    );
    assert!(
        desktop_profile.contains("[input]")
            && desktop_profile.contains("touch = true")
            && desktop_profile.contains("mouse = true")
            && desktop_profile.contains("kbd = true"),
        "desktop profile must remain the source-of-truth seed for visible QEMU device selection, including the interactive tablet-capable desktop lane"
    );
}

#[test]
fn fbdevd_polls_windowd_with_owned_cap_move_reply_inbox() {
    let init = read_repo_file("source/init/nexus-init/src/os_payload.rs");
    let fbdevd = read_repo_file("source/services/fbdevd/src/os_lite.rs");

    assert!(
        init.contains("init: fbdevd slots recv=0x")
            && init.contains("chan.input_send_slot = Some(send_slot);")
            && init.contains("chan.input_recv_slot = Some(recv_slot);")
            && init.contains("chan.reply_send_slot = Some(reply_send_slot);"),
        "init-lite must wire fbdevd service slots plus observer wiring with dedicated reply capability distribution"
    );
    assert!(
        fbdevd.contains("KernelClient::new_for(\"windowd\")")
            && fbdevd.contains("KernelClient::new_for(\"@reply\")")
            && fbdevd.contains("client.send_with_cap_move_wait(&request, reply_send_clone, send_wait)")
            && fbdevd.contains("const RPC_TIMEOUT_MS: u64 = 2;")
            && fbdevd.contains("DisplayReactor::new(windowd::VISIBLE_BOOTSTRAP_HZ)")
            && fbdevd.contains("TickBudget::new(4)"),
        "fbdevd must poll windowd through a short bounded CAP_MOVE reply inside a budgeted display reactor"
    );
}

#[test]
fn visible_input_fake_green_guard_requires_sequential_live_gates() {
    let harness = read_repo_file("scripts/qemu-test.sh");

    for marker in [
        "hidrawd: virtio-input mmio ready",
        "hidrawd: virtio-input keyboard ready",
        "hidrawd: virtio-input pointer ready",
        "hidrawd: virtio-input raw event seen",
        "hidrawd: ingress adapter ready",
        "inputd: live pointer route on",
        "inputd: live keyboard route on",
    ] {
        assert!(
            harness.contains(marker),
            "visible-input fake-green guard must require upstream gate `{marker}` before the final selftest marker"
        );
    }
}

#[test]
fn proof_mode_selftest_is_observer_only_for_live_input() {
    let display =
        read_repo_file("source/apps/selftest-client/src/os_lite/display_bootstrap_observer.rs");
    let observer = read_repo_file("source/apps/selftest-client/src/os_lite/display_observer.rs");
    let end_phase = read_repo_file("source/apps/selftest-client/src/os_lite/phases/end.rs");

    assert!(
        display.contains("observe_live_visible_input_proof")
            && display.contains("fetch_live_visible_state")
            && observer.contains("state.virtio_raw_seen")
            && observer.contains("state.hid_normalized_seen"),
        "proof-mode selftest must observe inputd-visible upstream gate state rather than replaying local input services"
    );
    assert!(
        !display.contains("fps:")
            && !display.contains("report_if_due")
            && !display.contains("last_report_ns"),
        "observer-only proof mode must not compute or publish local FPS telemetry"
    );
    assert!(
        !end_phase.contains("M_HIDRAWD_READY")
            && !end_phase.contains("M_INPUTD_READY")
            && !end_phase.contains("M_TOUCHD_READY"),
        "observer-only proof mode must not synthesize upstream service markers from selftest"
    );
}

#[test]
fn proof_mode_observer_keeps_enough_wait_budget_for_delayed_live_injection() {
    let display =
        read_repo_file("source/apps/selftest-client/src/os_lite/display_bootstrap_observer.rs");
    let end_phase = read_repo_file("source/apps/selftest-client/src/os_lite/phases/end.rs");

    assert!(
        display.contains("const OBSERVER_MAX_POLLS: usize = 128;")
            && display.contains("const OBSERVER_YIELDS_BETWEEN_POLLS: usize = 4096;"),
        "observer-only proof mode must keep a bounded but large enough wait budget to observe delayed live input injection honestly"
    );
    assert!(
        end_phase.contains("let mut proof_witness = ProofVisibleInputWitness::new();")
            && end_phase.contains("proof_witness.observe(state);")
            && end_phase.contains("if !proof_completed && proof_witness.ready()")
            && end_phase.contains("display_bootstrap::interactive_live_tick()")
            && end_phase.contains("emit_line(crate::markers::M_SELFTEST_END);"),
        "proof-mode end phase must keep polling visible state after startup and latch transient hold-state observations so late real input injection completes the observer-only proof instead of timing out falsely"
    );
}

#[test]
fn display_observer_visible_state_rpc_is_bounded_instead_of_blocking_forever() {
    let display =
        read_repo_file("source/apps/selftest-client/src/os_lite/display_bootstrap_observer.rs");

    assert!(
        display.contains("const VISIBLE_STATE_RPC_TIMEOUT_MS: u64 = 50;")
            && display.contains("Wait::Timeout(Duration::from_millis(VISIBLE_STATE_RPC_TIMEOUT_MS))"),
        "observer visible-state RPC must use an explicit bounded timeout so missing service replies fail honestly"
    );
    assert!(
        !display
            .contains("client.send_with_cap_move_wait(&request, reply_send_clone, Wait::Blocking)")
            && !display.contains("let frame = reply.recv(Wait::Blocking).ok()?;"),
        "observer visible-state RPC must not block forever on service send/recv"
    );
}

#[test]
fn display_bootstrap_markers_emit_before_visible_input_wait() {
    let display =
        read_repo_file("source/apps/selftest-client/src/os_lite/display_bootstrap_observer.rs");
    let end_phase = read_repo_file("source/apps/selftest-client/src/os_lite/phases/end.rs");

    assert!(
        display.contains("pub(crate) fn observe_display_evidence()")
            && display.contains("observe_live_visible_input_proof()?"),
        "display bootstrap markers must be emitted after service-owned display evidence, before waiting for live input evidence"
    );
    assert!(
        end_phase.contains("if let Ok(display) = display_bootstrap::observe_display_evidence()")
            && end_phase.contains("emit_line(windowd::DISPLAY_BOOTSTRAP_MARKER)")
            && end_phase.contains("emit_line(windowd::SELFTEST_UI_VISIBLE_PRESENT_MARKER)")
            && end_phase.find("observe_display_evidence()")
                < end_phase.find("display_bootstrap::run()"),
        "end phase must emit display/present markers before entering the live-input observer proof"
    );
}

#[test]
fn interactive_live_tick_is_observer_only_for_display_refresh() {
    let display =
        read_repo_file("source/apps/selftest-client/src/os_lite/display_bootstrap_observer.rs");

    assert!(
        display.contains("pub(crate) fn interactive_live_tick() -> Option<VisibleState>")
            && display.contains("fetch_live_visible_state()"),
        "interactive live ticks must stay observer-only and poll service-owned visible state"
    );
    assert!(
        !display.contains("vmo_write(")
            && !display.contains("configure_ramfb(")
            && !display.contains("write_handoff("),
        "interactive live ticks must not perform final scanout writes"
    );
}

#[test]
fn inputd_live_visible_feedback_uses_held_input_state_instead_of_sticky_dispatches() {
    let inputd = read_repo_file("source/services/inputd/src/os_lite.rs");

    assert!(
        inputd.contains("let pointer_held = self.input.primary_pointer_held();")
            && inputd.contains("let keyboard_held = self.input.held_non_modifier_key_count() > 0;")
            && inputd.contains("self.visible_state.launcher_click_visible = pointer_held;")
            && inputd.contains("self.visible_state.keyboard_visible = keyboard_held;"),
        "live input feedback must be driven by actual held mouse/key state so click and keyboard highlights return to idle immediately on release"
    );
}

#[test]
fn inputd_live_visible_feedback_exposes_transient_wheel_direction_indicators() {
    let inputd = read_repo_file("source/services/inputd/src/os_lite.rs");
    let renderer = read_repo_file("source/services/windowd/src/visible_state.rs");

    assert!(
        inputd.contains("InputDispatch::PointerWheel { delta_y }")
            && inputd.contains("self.note_wheel_indicator(pointer_wheel_delta, now_ns);")
            && inputd.contains("self.visible_state.wheel_up_visible")
            && inputd.contains("self.visible_state.wheel_down_visible"),
        "live input runtime must convert wheel batches into short-lived up/down visible-state pulses"
    );
    assert!(
        renderer.contains("wheel_triangle_contains(route_x, route_y, VISIBLE_INPUT_WHEEL_UP_Y, true)")
            && renderer.contains("wheel_triangle_contains(route_x, route_y, VISIBLE_INPUT_WHEEL_DOWN_Y, false)")
            && renderer.contains("VISIBLE_INPUT_WHEEL_ACTIVE_BGRA"),
        "windowd visible renderer must paint dedicated up/down wheel indicators next to the mouse target"
    );
}

#[test]
fn interactive_end_phase_uses_polled_visible_state_as_observer_seam() {
    let end_phase = read_repo_file("source/apps/selftest-client/src/os_lite/phases/end.rs");

    assert!(
        end_phase.contains("display_bootstrap::interactive_live_tick()")
            && end_phase.contains("if let Some(state) = display_bootstrap::interactive_live_tick()"),
        "interactive end phase must observe polled visible state rather than authoring display state"
    );
    assert!(
        end_phase.contains("interactive_mode == Some(RuntimeMode::InteractiveFull)")
            && end_phase.contains("emit_line(windowd::INPUT_VISIBLE_ON_MARKER)")
            && end_phase.contains("emit_line(windowd::CURSOR_MOVE_VISIBLE_MARKER)"),
        "interactive full breadcrumbs must remain downstream observations of the live display seam"
    );
}

#[test]
fn interactive_end_phase_polls_visible_state_on_explicit_live_refresh_cadence() {
    let end_phase = read_repo_file("source/apps/selftest-client/src/os_lite/phases/end.rs");

    assert!(
        end_phase.contains("INTERACTIVE_VISIBLE_STATE_POLL_INTERVAL_NS: u64 = 16_000_000")
            && end_phase.contains("INTERACTIVE_VISIBLE_STATE_POLL_FALLBACK_TICKS: u32 = 64")
            && end_phase.contains("should_poll_interactive_visible_state("),
        "interactive repaint must use an explicit live refresh cadence instead of an opaque large yield mask"
    );
    assert!(
        !end_phase.contains("(idle_ticks & 0x0fff) == 0"),
        "interactive repaint must not stay behind the old sparse 0x0fff yield gate"
    );
}

#[test]
fn visible_bootstrap_failure_summary_carries_service_fps_traces() {
    let harness = read_repo_file("scripts/qemu-test.sh");

    for needle in [
        "line_fps_hidrawd=$(grep -aFn \"fps: hidrawd\"",
        "line_fps_inputd=$(grep -aFn \"fps: inputd\"",
        "line_fps_windowd=$(grep -aFn \"fps: windowd\"",
        "line_fps_fbdevd=$(grep -aFn \"fps: fbdevd\"",
        "last_fps_hidrawd=$(grep -aF \"fps: hidrawd\"",
        "last_fps_inputd=$(grep -aF \"fps: inputd\"",
        "last_fps_windowd=$(grep -aF \"fps: windowd\"",
        "last_fps_fbdevd=$(grep -aF \"fps: fbdevd\"",
        "\\\"line_fps_hidrawd\\\":$line_fps_hidrawd",
        "\\\"line_fps_inputd\\\":$line_fps_inputd",
        "\\\"line_fps_windowd\\\":$line_fps_windowd",
        "\\\"line_fps_fbdevd\\\":$line_fps_fbdevd",
        "\\\"last_fps_hidrawd\\\":\\\"$last_fps_hidrawd\\\"",
        "\\\"last_fps_inputd\\\":\\\"$last_fps_inputd\\\"",
        "\\\"last_fps_windowd\\\":\\\"$last_fps_windowd\\\"",
        "\\\"last_fps_fbdevd\\\":\\\"$last_fps_fbdevd\\\"",
        "\\\"line_display_fail\\\":$line_display_fail",
        "\\\"last_display_gate\\\":\\\"$last_display_gate\\\"",
    ] {
        assert!(
            harness.contains(needle),
            "visible-bootstrap failure summary must carry service FPS evidence `{needle}`"
        );
    }
}

#[test]
fn visible_bootstrap_harness_requires_service_owned_display_markers() {
    let harness = read_repo_file("scripts/qemu-test.sh");
    let ui_markers = read_repo_file("source/apps/selftest-client/proof-manifest/markers/ui.toml");

    for marker in
        ["fbdevd: ready", "fbdevd: map ok", "fbdevd: ramfb configured", "fbdevd: flush ok"]
    {
        assert!(
            harness.contains(marker),
            "visible-bootstrap ladder must require service-owned display marker `{marker}`"
        );
        assert!(
            ui_markers.contains(&format!("[marker.\"{marker}\"]")),
            "proof-manifest must declare service-owned display marker `{marker}`"
        );
    }
}
