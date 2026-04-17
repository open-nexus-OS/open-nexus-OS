# Next Task Preparation (Drift-Free)

## Candidate next execution
- **task**: open `TASK-0023B` **Phase 2** — slice `pub fn run()` in `source/apps/selftest-client/src/os_lite/mod.rs` into sub-orchestrators (`bring_up`, `mmio`, `routing`, `ota`, `policy`, `logd`, `vfs`, `end`); add intra-domain sub-splits in `updated/{stage.rs, switch.rs, health.rs, reply_pump.rs}` and `probes/ipc_kernel/{plumbing.rs, security.rs, soak.rs}`; consolidate the dreifach duplizierten lokalen `ReplyInboxV1`-`impl Client` (in `cap_move_reply_probe`, `sender_pid_probe`, `ipc_soak_probe`) zu einem typed wrapper.
- **focus**: keep Cuts 0–22 frozen and behavior-preserving. Same Proof-Floor cadence per cut. Phase 2 explicitly does not change marker order, marker strings, or reject behavior.

## Current Phase-1 structural state (verified green, Phase 1 closed)
- `source/apps/selftest-client/src/main.rs` = 122 lines (frozen).
- `source/apps/selftest-client/src/os_lite/mod.rs` = **1226** lines (only imports + `mod`-decls + `pub fn run()` body).
- All extracted modules under `source/apps/selftest-client/src/os_lite/`:
  - `dsoftbus/{quic_os, remote/{mod,resolve,pkgfs,statefs}}`
  - `net/{icmp_ping, local_addr, smoltcp_probe (cfg-gated)}`
  - `ipc/{clients, routing, reply}`
  - `mmio/`, `vfs/`, `timed/`
  - `probes/{rng, device_key, ipc_kernel, elf}`
  - `services/{samgrd, bundlemgrd, keystored, policyd, execd, logd, metricsd, statefs, bootctl}/mod.rs` (+ `services/mod.rs` shared `core_service_probe*` helpers)
  - `updated/mod.rs` (`SYSTEM_TEST_NXS` + `SlotId` + 9 fns incl. `updated_send_with_reply` and `init_health_ok`)

## Current proven baseline (must stay green per cut)
- Host:
  - `just test-dsoftbus-quic`
  - `cargo test -p dsoftbus --test quic_selection_contract -- --nocapture`
  - `cargo test -p dsoftbus --test quic_host_transport_contract -- --nocapture`
  - `cargo test -p dsoftbus --test quic_feasibility_contract -- --nocapture`
  - `cargo test -p dsoftbusd -- --nocapture`
- OS:
  - `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`
  - required markers:
    - `dsoftbusd: transport selected quic`
    - `dsoftbusd: auth ok`
    - `dsoftbusd: os session ok`
    - `SELFTEST: quic session ok`
  - forbidden fallback markers:
    - `dsoftbusd: transport selected tcp`
    - `dsoftbus: quic os disabled (fallback tcp)`
    - `SELFTEST: quic fallback ok`
- Hygiene:
  - `just dep-gate && just diag-os`
  - `just fmt-check && just lint`

## Boundaries for Phase 2
- Keep `TASK-0021`, `TASK-0022`, `TASK-0023` closed/done.
- Do not regress `TASK-0023` to fallback-only marker semantics.
- Do not absorb `TASK-0024` transport features into `TASK-0023B`.
- Phase 2 slicing must be behavior-preserving: same marker order, same proof meanings, same reject behavior.
- `main.rs` must remain 122 lines.
- Visibility ceiling: `pub(crate)` (binary crate boundary).
- No new `unwrap`/`expect`. No new dependencies in `Cargo.toml`.

## Linked contracts
- `tasks/TASK-0023B-selftest-client-production-grade-deterministic-test-architecture-refactor.md`
- `docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md`
- `tasks/TASK-0023-dsoftbus-quic-v2-os-enabled-gated.md`
- `docs/rfcs/RFC-0037-dsoftbus-quic-v2-os-enabled-gated.md`
- `tasks/TASK-0024-dsoftbus-udp-sec-v1-os-enabled.md`
- `docs/testing/index.md`
- `docs/distributed/dsoftbus-lite.md`
- `tasks/STATUS-BOARD.md`
- `tasks/IMPLEMENTATION-ORDER.md`

## Ready condition
- Start from this frozen green baseline (Cuts 0–22 merged) and request Plan Mode to design Phase 2:
  - sequence of `run()` slicing into sub-orchestrators (8 cuts, one per domain),
  - intra-domain sub-splits inside `updated/` and `probes/ipc_kernel/`,
  - DRY-Konsolidierung der lokalen `ReplyInboxV1` Kopien,
  - rerun the Phase-1 Proof-Floor after every cut,
  - stop immediately on marker/order/reject-path drift.
- Closure of Phase 2 + Phase 3 will trigger the deferred STATUS-BOARD / IMPLEMENTATION-ORDER finalization for `TASK-0023B`.
