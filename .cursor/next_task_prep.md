# Next Task Preparation (Drift-Free)

## Candidate next execution
- **task**: continue `TASK-0023B` Phase 1 — extract the **service-probe families** and the `updated` / IPC-kernel / security-probe blocks from `source/apps/selftest-client/src/os_lite/mod.rs` (currently 4404 lines).
- **focus**: keep the periphery cuts (Cuts 0–9) frozen and behavior-preserving while continuing the no-behavior-change extraction toward an `os_lite/services/*` and `os_lite/{updated,ipc_kernel,security}/*` topology.

## Current Phase-1 structural state (verified green)
- `source/apps/selftest-client/src/main.rs` = 122 lines (Phase-1 minimal target).
- `source/apps/selftest-client/src/os_lite/mod.rs` ≈ 4404 lines.
- Already extracted modules under `source/apps/selftest-client/src/os_lite/`:
  - `dsoftbus/{quic_os, remote/{mod,resolve,pkgfs,statefs}}`
  - `net/{icmp_ping, local_addr, smoltcp_probe (cfg-gated)}`
  - `ipc/{clients, routing, reply}`
  - `mmio/`, `vfs/`, `timed/`, `probes/{rng, device_key}`

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

## Boundaries for next slice
- Keep `TASK-0021`, `TASK-0022`, `TASK-0023` closed/done.
- Do not regress `TASK-0023` to fallback-only marker semantics.
- Do not absorb `TASK-0024` transport features into `TASK-0023B`.
- Keep `TASK-0023B` behavior-preserving: same marker order, same proof meanings, same reject behavior.
- `main.rs` must remain 122 lines until full Phase-1 closure.
- `pub fn run()` body: only call-site repath (`xyz()` → `module::xyz()`); no reordering, no marker rename, no `unwrap`/`expect` introduction.
- Visibility ceiling: `pub(crate)` (binary crate boundary).

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
- Start from this frozen green baseline (Cuts 0–9 merged) and request Plan Mode to design the next batch of `TASK-0023B` Phase-1 cuts:
  - propose a service-family topology (e.g. `os_lite/services/{samgrd,bundlemgrd,keystored,policyd,execd,logd,metricsd,statefs,bootctl}/mod.rs`),
  - sequence the cuts so cross-references stay mechanical (e.g. policyd before keystored if helper sharing emerges),
  - rerun the Phase-1 Proof-Floor after every cut,
  - stop immediately on marker/order/reject-path drift.
- Closure of full Phase 1 will trigger the deferred STATUS-BOARD / IMPLEMENTATION-ORDER / RFC-0038 Implementation Checklist updates in a dedicated wrap-up session.
