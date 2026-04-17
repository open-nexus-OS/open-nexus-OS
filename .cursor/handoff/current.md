# Current Handoff: TASK-0023B Phase 1 — periphery extraction in flight

**Date**: 2026-04-17
**Status**: `TASK-0023B` Phase 1 in progress. Cuts 0–9 of the structural extraction are merged/staged; `main.rs` is structurally minimal at 122 lines. `os_lite/mod.rs` shrunk from ~6771 → 4404 lines via 10 behavior-preserving cuts. The next Phase-1 session will tackle the service-probe families (samgrd / bundlemgrd / keystored / policyd / execd / logd / metricsd / statefs / bootctl) and the `updated`/IPC-kernel/security-probe blocks.
**Execution SSOT**: `tasks/TASK-0023B-selftest-client-production-grade-deterministic-test-architecture-refactor.md`
**Contract SSOT**: `docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md`

## What changed in the latest session
- Cuts 4–9 executed under plan `task-0023b_phase_1_cuts_4-9_5dd1c046.plan.md`:
  - Cut 4 → `os_lite/mmio/mod.rs` (`MmioBus`, `mmio_map_probe`, `cap_query_{mmio,vmo}_probe`)
  - Cut 5 → `os_lite/vfs/mod.rs` (`verify_vfs`)
  - Cut 6 → `os_lite/net/smoltcp_probe.rs` (full `#[cfg(feature = "smoltcp-probe")]` block; pre-existing dead-code typo `dev_va` → `MMIO_VA` corrected so the gate compiles, behavior unchanged)
  - Cut 7 → `os_lite/dsoftbus/remote/{mod,resolve,pkgfs,statefs}.rs` (12× `dsoftbusd_remote_*` + `REMOTE_DSOFTBUS_WAIT_MS`)
  - Cut 8 → `os_lite/timed/mod.rs` (`timed_align_up/register/cancel/sleep_until/fail/coalesce_probe`)
  - Cut 9 → `os_lite/probes/{rng,device_key}.rs` (rng + device-key selftests)
- All call sites in `os_lite::run()` repointed; visibility kept at `pub(crate)`; ordering and marker strings byte-identical.
- No edits to `main.rs`, `markers.rs`, `Cargo.toml`, or `build.rs` this session.

## Current execution posture
- Phase order is fixed:
  - Phase 1: structural extraction without behavior change (in progress, periphery done; service-families remaining),
  - Phase 2: maintainability/extensibility cleanup without feature drift,
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
- Continue `TASK-0023B` Phase 1 by extracting the service-probe families from `os_lite/mod.rs` (samgrd, bundlemgrd, keystored, policyd, execd, logd, metricsd, statefs, bootctl) plus the `updated` / IPC-kernel / security-probe blocks. Plan-first, contract-first; same Proof-Floor cadence per cut.
- Do not absorb `TASK-0024` feature work (UDP-sec, recovery-flow breadth) into the refactor.
- Defer STATUS-BOARD / IMPLEMENTATION-ORDER / RFC-0038 Implementation Checklist updates until full Phase-1 closure.
