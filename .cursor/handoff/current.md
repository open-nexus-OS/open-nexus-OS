# Current Handoff: TASK-0023B Phase 1 — closed (cuts 0–22 landed)

**Date**: 2026-04-17
**Status**: `TASK-0023B` Phase 1 **complete**. Cuts 0–22 of the structural extraction are merged/staged; `main.rs` is structurally minimal at 122 lines. `os_lite/mod.rs` shrunk from ~6771 → 1226 lines via 23 behavior-preserving cuts. `os_lite/mod.rs` now contains only top-level imports + module declarations + the `pub fn run()` orchestrator body. The next session can start Phase 2 (`run()` slicing into sub-orchestrators, Phase-2 maintainability sub-splits inside `updated/`, `probes/ipc_kernel/`, etc.).
**Execution SSOT**: `tasks/TASK-0023B-selftest-client-production-grade-deterministic-test-architecture-refactor.md`
**Contract SSOT**: `docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md`

## What changed in the latest session
- Cuts 19–22 executed under plan `task-0023b_cuts_19-22_914398f4.plan.md`:
  - Cut 19 → `os_lite/updated/mod.rs` (`SYSTEM_TEST_NXS` const + `SlotId` enum + `updated_stage`, `updated_log_probe`, `updated_switch`, `updated_get_status`, `updated_boot_attempt`, `init_health_ok`, `updated_expect_status`, `updated_send_with_reply`)
  - Cut 20 → `os_lite/probes/ipc_kernel/mod.rs` (`qos_probe`, `ipc_payload_roundtrip`, `ipc_deadline_timeout_probe`, `nexus_ipc_kernel_loopback_probe`, `cap_move_reply_probe`, `sender_pid_probe`, `sender_service_id_probe`, `ipc_soak_probe`)
  - Cut 21 → `os_lite/probes/elf.rs` (`log_hello_elf_header`; `read_u64_le` reduced to file-private)
  - Cut 22 → `emit_line` shim removal in `os_lite/mod.rs`; replaced with direct `use crate::markers::emit_line`
- Imports in `os_lite/mod.rs` aggressively cleaned up after each cut (`AtomicU64`, `Ordering`, `Duration`, `task_qos_*`, `ipc_*_v1*`, `QosClass`, `OsClock`, `deadline_after`, `recv_match_until`, `ReplyBuffer`, `IpcError`, `cached_reply_client`, `cached_samgrd_client`, `HELLO_ELF`, `crate::markers` module alias).
- All call sites in `os_lite::run()` repointed to `updated::*` / `probes::ipc_kernel::*` / `probes::elf::*`; visibility kept at `pub(crate)`; ordering and marker strings byte-identical.
- Phase-1 Proof-Floor was rerun after every cut (4 cuts × `cargo test -p dsoftbusd` + `just test-dsoftbus-quic` + `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`); full marker ladder unchanged, no marker drift, no fallback markers.
- Hygiene at session end: `just fmt-check` and `just lint` green (one rustfmt-only adjustment in `mod.rs` for multi-arg `updated::*` call sites).
- No edits to `main.rs`, `markers.rs`, `Cargo.toml`, or `build.rs` this session.

## Current execution posture
- Phase 1 (structural extraction without behavior change) is **closed**. Open work:
  - Phase 2: maintainability/extensibility cleanup including `run()` slicing into sub-orchestrators (`bring_up`, `mmio`, `routing`, `ota`, `policy`, `logd`, `vfs`, `end`) and intra-domain sub-splits (`updated/{stage.rs, switch.rs, health.rs, reply_pump.rs}`, `probes/ipc_kernel/{plumbing.rs, security.rs, soak.rs}`).
  - Phase 3: standards + closure review with full proof floor (newtype/Send/Sync/`#[must_use]` review, DRY-Konsolidierung der lokalen `ReplyInboxV1` Kopien).
- The full ladder in `scripts/qemu-test.sh` remains authoritative, not only the QUIC subset.
- `main.rs` stays at 122 lines.

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
- Open Phase 2 of `TASK-0023B`:
  - Slice `pub fn run()` into sub-orchestrators (`bring_up`, `mmio`, `routing`, `ota`, `policy`, `logd`, `vfs`, `end`) without touching marker order.
  - Intra-domain sub-splits: `updated/{stage.rs, switch.rs, health.rs, reply_pump.rs}`, `probes/ipc_kernel/{plumbing.rs, security.rs, soak.rs}`.
  - DRY-Konsolidierung der dreifach duplizierten lokalen `ReplyInboxV1`-`impl Client` (in `cap_move_reply_probe`, `sender_pid_probe`, `ipc_soak_probe`) zu einem typed wrapper.
- Plan-first, contract-first; same Proof-Floor cadence per cut.
- Do not absorb `TASK-0024` feature work (UDP-sec, recovery-flow breadth) into the refactor.
- STATUS-BOARD / IMPLEMENTATION-ORDER updates can now reflect "TASK-0023B Phase 1 closed; Phase 2 ready (run() slicing)".
