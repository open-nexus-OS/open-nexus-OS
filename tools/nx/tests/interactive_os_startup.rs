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
            && makefile.contains("QEMU_MARKER_LEVEL=minimal"),
        "`make run` must select the interactive-minimal runner posture"
    );
    assert!(
        makefile.contains("NEXUS_SELFTEST_MODE=interactive-minimal")
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
fn interactive_qemu_exposes_keyboard_and_pointer_devices() {
    let runner = read_repo_file("scripts/run-qemu-rv64.sh");

    assert!(
        runner.contains("QEMU_SESSION_MODE")
            && runner.contains("virtio-keyboard-device")
            && runner.contains("virtio-tablet-device")
            && runner.contains("virtio-mouse-device"),
        "interactive QEMU display starts must expose keyboard plus absolute and relative pointer devices"
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
        harness.contains("NEXUS_SELFTEST_MODE=${NEXUS_SELFTEST_MODE:-proof}")
            && harness.contains("NEXUS_SELFTEST_PROFILE=${NEXUS_SELFTEST_PROFILE:-full}"),
        "`scripts/qemu-test.sh` visible-bootstrap path must request proof-only guest semantics"
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
        "mouse-following pixel",
        "hover/click rectangle",
        "keyboard-input rectangle",
        "must still be proven end-to-end in QEMU",
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
fn live_framebuffer_has_dedicated_vmo_budget_and_error_label() {
    let mm = read_repo_file("source/kernel/neuron/src/mm/mod.rs");
    let display = read_repo_file("source/apps/selftest-client/src/os_lite/display_bootstrap.rs");

    assert!(
        mm.contains("USER_VMO_ARENA_LEN: usize = 32 * 1024 * 1024"),
        "live ramfb bootstrap must have enough VMO arena headroom after service bring-up"
    );
    assert!(
        mm.contains("USER_VMO_ARENA_BASE: usize = 0x8100_0000")
            && mm.contains("KERNEL_PAGE_POOL_BASE: usize = 0x8080_0000")
            && mm.contains("KERNEL_PAGE_POOL_LEN: usize = 2 * 1024 * 1024"),
        "VMO arena and kernel page-pool windows must stay explicit and non-overlapping"
    );
    assert!(
        display.contains("bootstrap: failed framebuffer-vmo"),
        "framebuffer allocation failures must emit a stable userspace label"
    );
}

#[test]
fn interactive_scene_presents_initial_committed_frame() {
    let display = read_repo_file("source/apps/selftest-client/src/os_lite/display_bootstrap.rs");

    assert!(
        display.contains(".present_tick()")
            && display.contains("debug8cde1d: interactive present-tick-ok"),
        "interactive scene setup must present the initially committed scene before waiting for live input"
    );
}

#[test]
fn interactive_visible_state_writer_clamps_rows_and_stride_to_surface_bounds() {
    let display = read_repo_file("source/apps/selftest-client/src/os_lite/display_bootstrap.rs");

    assert!(
        display.contains("let row_len = mode.stride as usize;")
            && display.contains("let width = core::cmp::min(mode.width as usize, windowd::VISIBLE_BOOTSTRAP_WIDTH as usize);")
            && display.contains("let idx = x.checked_mul(4).ok_or(BootstrapFailure::FrameWrite)?;")
            && display.contains("let dst_offset = y as usize * row_len;"),
        "interactive visible-state row writes must stay bounded to source surface geometry"
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
        "send_fail={}",
        "rebinds={}",
        "idle_yields={}",
    ] {
        assert!(hidrawd.contains(needle), "`hidrawd` service telemetry must include `{needle}`");
    }
}

#[test]
fn inputd_service_owns_periodic_chain_fps_telemetry() {
    let inputd = read_repo_file("source/services/inputd/src/os_lite.rs");

    for needle in [
        "fps: inputd recv_hz=",
        "hid_ok_hz={}",
        "poll_hz={}",
        "malformed={}",
        "overflow={}",
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
        inputd.contains("inputd fallback bind")
            && inputd.contains("mode=fallback recv_slot={recv_slot} send_slot={send_slot}"),
        "inputd fallback route must publish explicit slot evidence for bring-up triage"
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
            && hidrawd.contains("live_devices = open_live_devices(&mut service);"),
        "hidrawd must retry live device open when init-lite transfers MMIO caps after process spawn"
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
        runner.contains("QEMU_INPUT_AUTOINJECT=${QEMU_INPUT_AUTOINJECT:-0}")
            && runner.contains("-qmp \"unix:$QEMU_QMP_SOCKET,server=on,wait=off\"")
            && runner.contains("python3 \"$QEMU_INPUT_INJECTOR_PY\" \"$QEMU_QMP_SOCKET\""),
        "run-qemu runner must expose a QMP socket and launch the deterministic input injector when requested"
    );
    assert!(
        injector.contains("\"execute\": \"input-send-event\"")
            && injector.contains("\"type\": \"rel\"")
            && injector.contains("\"button\": \"left\"")
            && injector.contains("\"type\": \"key\"")
            && injector.contains("\"data\": \"a\""),
        "QMP injector must drive real pointer move, click, and keyboard input into the guest"
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
    let display = read_repo_file("source/apps/selftest-client/src/os_lite/display_bootstrap.rs");
    let end_phase = read_repo_file("source/apps/selftest-client/src/os_lite/phases/end.rs");

    assert!(
        display.contains("observe_live_visible_input_proof")
            && display.contains("fetch_live_visible_state")
            && display.contains("state.virtio_raw_seen")
            && display.contains("state.hid_normalized_seen"),
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
    let display = read_repo_file("source/apps/selftest-client/src/os_lite/display_bootstrap.rs");

    assert!(
        display.contains("const OBSERVER_MAX_POLLS: usize = 128;")
            && display.contains("const OBSERVER_YIELDS_BETWEEN_POLLS: usize = 4096;"),
        "observer-only proof mode must keep a bounded but large enough wait budget to observe delayed live input injection honestly"
    );
}

#[test]
fn interactive_live_tick_renders_rows_without_heap_allocating_a_full_surface() {
    let display = read_repo_file("source/apps/selftest-client/src/os_lite/display_bootstrap.rs");

    assert!(
        display.contains("let mut row = [0u8; windowd::VISIBLE_BOOTSTRAP_WIDTH as usize * 4];")
            && display.contains("write_interactive_visible_state_row("),
        "interactive live repaint must render through a fixed row buffer instead of allocating a fresh full frame per tick"
    );
    assert!(
        !display.contains("let mut surface = visible_input_scene_surface(\n        windowd::CallerCtx::from_service_metadata(0x55),\n        91,"),
        "interactive live repaint must not allocate a new visible input surface on every observer tick"
    );
}

#[test]
fn interactive_end_phase_uses_polled_visible_state_as_live_repaint_seam() {
    let end_phase = read_repo_file("source/apps/selftest-client/src/os_lite/phases/end.rs");

    assert!(
        end_phase.contains("display_bootstrap::interactive_live_tick()")
            && end_phase.contains("if let Some(state) = display_bootstrap::interactive_live_tick()"),
        "interactive end phase must repaint from polled visible state rather than from self-authored input state"
    );
    assert!(
        end_phase.contains("interactive_mode == Some(RuntimeMode::InteractiveFull)")
            && end_phase.contains("emit_line(windowd::INPUT_VISIBLE_ON_MARKER)")
            && end_phase.contains("emit_line(windowd::CURSOR_MOVE_VISIBLE_MARKER)"),
        "interactive full breadcrumbs must remain downstream observations of the live repaint seam"
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
        "last_fps_hidrawd=$(grep -aF \"fps: hidrawd\"",
        "last_fps_inputd=$(grep -aF \"fps: inputd\"",
        "\\\"line_fps_hidrawd\\\":$line_fps_hidrawd",
        "\\\"line_fps_inputd\\\":$line_fps_inputd",
        "\\\"last_fps_hidrawd\\\":\\\"$last_fps_hidrawd\\\"",
        "\\\"last_fps_inputd\\\":\\\"$last_fps_inputd\\\"",
    ] {
        assert!(
            harness.contains(needle),
            "visible-bootstrap failure summary must carry service FPS evidence `{needle}`"
        );
    }
}
