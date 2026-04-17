# Current Handoff: TASK-0023B Phase 1 — service-family extraction landed

**Date**: 2026-04-17
**Status**: `TASK-0023B` Phase 1 in progress. Cuts 0–18 of the structural extraction are merged/staged; `main.rs` is structurally minimal at 122 lines. `os_lite/mod.rs` shrunk from ~6771 → 2025 lines via 19 behavior-preserving cuts. The next Phase-1 session will tackle the remaining `updated` family, the IPC-kernel/security-probe block, the ELF helpers, and the `emit_line` shim — and then close Phase 1 so Phase 2 (`run()` slicing into sub-orchestrators) can start.
**Execution SSOT**: `tasks/TASK-0023B-selftest-client-production-grade-deterministic-test-architecture-refactor.md`
**Contract SSOT**: `docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md`

## What changed in the latest session
- Cuts 10–18 executed under plan `task-0023b_cuts_10-18_services_1bbc82d7.plan.md`:
  - Cut 10 → `os_lite/services/{mod.rs, samgrd/mod.rs}` (incl. `core_service_probe`, `core_service_probe_policyd`, `samgrd_v1_register/_lookup`, `fetch_sender_service_id_from_samgrd`)
  - Cut 11 → `os_lite/services/bundlemgrd/mod.rs` (`bundlemgrd_v1_{list,fetch_image,fetch_image_slot,set_active_slot,route_status}`)
  - Cut 12 → `os_lite/services/keystored/mod.rs` (`keystored_ping`, `resolve_keystored_client`, `keystored_cap_move_probe`)
  - Cut 13 → `os_lite/services/policyd/mod.rs` (`policy_check`, `policyd_check_cap`, `keystored_sign_denied`, `policyd_requester_spoof_denied`, `policyd_fetch_abi_profile`)
  - Cut 14 → `os_lite/services/execd/mod.rs` (`execd_spawn_image{,_raw_requester}`, `execd_report_exit_with_dump{,_status,_status_legacy}`, `wait_for_pid`, `emit_line_with_pid_status`)
  - Cut 15 → `os_lite/services/logd/mod.rs` (`logd_append_status_v2`, `logd_append_probe`, `logd_hardening_reject_probe`, `logd_query_probe`, `logd_stats_total`, `logd_query_count`, `logd_query_contains_since_paged`)
  - Cut 16 → `os_lite/services/metricsd/mod.rs` (`metricsd_security_reject_probe`, `wait_rate_limit_window`, `metricsd_semantic_probe`; cross-refs `super::samgrd::*` and `super::logd::*`)
  - Cut 17 → `os_lite/services/statefs/mod.rs` (`statefs_send_recv`, `statefs_put_get_list`, `statefs_unauthorized_access`, `statefs_persist`, `statefs_has_crash_dump`, `grant_statefs_caps_to_child`, `locate_minidump_for_crash`)
  - Cut 18 → `os_lite/services/bootctl/mod.rs` (`bootctl_persist_check`)
- All call sites in `os_lite::run()` repointed to `services::<svc>::*`; visibility kept at `pub(crate)`; ordering and marker strings byte-identical.
- Phase-1 Proof-Floor was rerun after every cut (9 cuts × `cargo test -p dsoftbusd` + `just test-dsoftbus-quic` + `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`); full marker ladder unchanged, no marker drift, no fallback markers.
- Hygiene at session end: `just fmt-check` and `just lint` green.
- No edits to `main.rs`, `markers.rs`, `Cargo.toml`, or `build.rs` this session.

## Current execution posture
- Phase order is fixed:
  - Phase 1: structural extraction without behavior change (in progress; service-families done; `updated` + IPC-kernel/security-probe + ELF helpers + `emit_line` shim remaining),
  - Phase 2: maintainability/extensibility cleanup including `run()` slicing into sub-orchestrators,
  - Phase 3: standards + closure review with full proof floor.
- The full ladder in `scripts/qemu-test.sh` remains authoritative, not only the QUIC subset.
- `main.rs` stays at 122 lines until Phase 1 is complete.

## Frozen baseline that must stay green (verified after every cut)
- Host:
  - `cargo test -p dsoftbusd -- --nocapture`
  - `just test-dsoftbus-quic`
- OS:
  - `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`
  - required QUIC subset markers:
    - `dsoftbusd: transport selected quic`
    - `dsoftbusd: auth ok`
    - `dsoftbusd: os session ok`
    - `SELFTEST: quic session ok`
- Forbidden fallback markers (must remain absent):
  - `dsoftbusd: transport selected tcp`
  - `dsoftbus: quic os disabled (fallback tcp)`
  - `SELFTEST: quic fallback ok`

## Next handoff target
- Continue `TASK-0023B` Phase 1 by extracting from `os_lite/mod.rs`:
  - `updated` family (`updated_stage`, `updated_log_probe`, `updated_switch`, `updated_get_status`, `updated_boot_attempt`, `init_health_ok`, `updated_expect_status`, `updated_send_with_reply`) plus the `SYSTEM_TEST_NXS` const,
  - IPC-kernel/security-probe block (`qos_probe`, `ipc_payload_roundtrip`, `ipc_deadline_timeout_probe`, `nexus_ipc_kernel_loopback_probe`, `cap_move_reply_probe`, `sender_pid_probe`, `sender_service_id_probe`, `ipc_soak_probe`),
  - ELF helpers (`log_hello_elf_header`, `read_u64_le`),
  - `emit_line` shim consolidation (move into `markers` or a tiny `os_lite/markers_shim` per cut).
- Plan-first, contract-first; same Proof-Floor cadence per cut.
- Do not absorb `TASK-0024` feature work (UDP-sec, recovery-flow breadth) into the refactor.
- Defer STATUS-BOARD / IMPLEMENTATION-ORDER updates until full Phase-1 closure (after the remaining cuts above land).
