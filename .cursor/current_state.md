# Cursor Current State (SSOT)

## Current architecture state
- **last_decision**: `TASK-0023B` Phase 1 periphery extraction (Cuts 0–9) is in flight; `os_lite/mod.rs` reduced from ~6771 → 4404 lines while `main.rs` remains structurally minimal at 122 lines. Service-probe families (samgrd / bundlemgrd / keystored / policyd / execd / logd / metricsd / statefs / bootctl) plus `updated` / IPC-kernel / security-probe blocks are the next Phase-1 batch.
- **active_constraints**:
  - keep `TASK-0021`, `TASK-0022`, `TASK-0023` frozen as done baselines,
  - keep marker honesty strict (`ok`/`ready` only after real behavior),
  - keep `TASK-0023B` behavior-preserving (no marker rename, no reordering, no new `unwrap`/`expect`, visibility ceiling `pub(crate)`),
  - keep `main.rs` at 122 lines until full Phase-1 closure,
  - keep no_std-safe boundaries explicit (no hidden std/runtime coupling),
  - keep `TASK-0024` blocked behind `TASK-0023B`,
  - keep `TASK-0044` as follow-up tuning scope (no silent scope absorption),
  - defer STATUS-BOARD / IMPLEMENTATION-ORDER / RFC-0038 Implementation Checklist updates until full Phase-1 closure (per-cut updates create drift).

## Current focus (execution)
- **active_task**: `TASK-0023B` selftest-client production-grade deterministic test architecture refactor — Phase 1, periphery cuts merged/staged, service-family cuts pending.
- **seed_contract**:
  - `tasks/TASK-0023B-selftest-client-production-grade-deterministic-test-architecture-refactor.md`
  - `docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md`
  - `tasks/TASK-0023-dsoftbus-quic-v2-os-enabled-gated.md`
  - `docs/rfcs/RFC-0037-dsoftbus-quic-v2-os-enabled-gated.md`
  - `docs/testing/index.md`
  - `tasks/STATUS-BOARD.md`
  - `tasks/IMPLEMENTATION-ORDER.md`

## TASK-0023B Phase 1 structural snapshot (2026-04-17)
- `source/apps/selftest-client/src/main.rs` = 122 lines (unchanged; Phase-1 structural target met).
- `source/apps/selftest-client/src/os_lite/mod.rs` ≈ 4404 lines (target ≤ 4500 after Cuts 4–9; further reduction expected once service families are extracted).
- Already extracted modules under `source/apps/selftest-client/src/os_lite/`:
  - `dsoftbus/{quic_os, remote/{mod,resolve,pkgfs,statefs}}`
  - `net/{icmp_ping, local_addr, smoltcp_probe (cfg-gated)}`
  - `ipc/{clients, routing, reply}`
  - `mmio/`, `vfs/`, `timed/`, `probes/{rng, device_key}`
- Behavior-parity gates honored each cut: `pub fn run()` call-order unchanged; marker strings byte-identical; visibility kept at `pub(crate)`.
- Pre-existing dead-code typo `dev_va` corrected to `MMIO_VA` in the moved smoltcp-probe block so the `--features smoltcp-probe` gate compiles; documented as a no-behavior-change correction (the path is not on the default marker ladder).

## Frozen baseline (must stay green per cut)
- Host proofs:
  - `cargo test -p dsoftbusd -- --nocapture`
  - `just test-dsoftbus-quic`
  - `cargo test -p dsoftbus --test quic_selection_contract -- --nocapture`
  - `cargo test -p dsoftbus --test quic_host_transport_contract -- --nocapture`
  - `cargo test -p dsoftbus --test quic_feasibility_contract -- --nocapture`
- OS proof:
  - `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`
  - required markers:
    - `dsoftbusd: transport selected quic`
    - `dsoftbusd: auth ok`
    - `dsoftbusd: os session ok`
    - `SELFTEST: quic session ok`
  - forbidden fallback markers (must remain absent):
    - `dsoftbusd: transport selected tcp`
    - `dsoftbus: quic os disabled (fallback tcp)`
    - `SELFTEST: quic fallback ok`
- Hygiene gates:
  - `just dep-gate && just diag-os`
  - `just fmt-check && just lint`
- Build sanity (Cut-6 invariant):
  - `RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' cargo +nightly check -p selftest-client --target riscv64imac-unknown-none-elf --no-default-features --features os-lite,smoltcp-probe`

## Scope boundaries reaffirmed
- `TASK-0023`: closed as real OS session path (production-floor scope).
- `TASK-0023B`: in flight; Phase-1 periphery cuts done, service-family cuts and Phases 2/3 (typed wrappers, Send/Sync, `#[must_use]`, closure docs) pending.
- `TASK-0024`: follow-up transport breadth work after `TASK-0023B` (no reopening `TASK-0023` closure semantics).
- `TASK-0044`: explicit tuning/performance breadth follow-up.

## Next handoff target
- Plan-first Phase-1 follow-on session: extract service-probe families and the `updated` / IPC-kernel / security-probe blocks from `os_lite/mod.rs` under the same Phase-1 Proof-Floor cadence. Trigger the deferred STATUS-BOARD / IMPLEMENTATION-ORDER / RFC-0038 Implementation Checklist updates only at full Phase-1 closure.
