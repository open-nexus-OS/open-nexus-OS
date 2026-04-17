# Next Task Preparation (Drift-Free)

## Candidate next execution
- **task**: continue `TASK-0023B` Phase 1 — extract the **`updated` family**, the **IPC-kernel/security-probe block**, the **ELF helpers**, and the **`emit_line` shim** from `source/apps/selftest-client/src/os_lite/mod.rs` (currently 2025 lines). After these cuts land, Phase 1 is closed and Phase 2 (`run()` slicing into sub-orchestrators) opens.
- **focus**: keep the periphery cuts (Cuts 0–9) and the service-family cuts (Cuts 10–18) frozen and behavior-preserving while continuing the no-behavior-change extraction toward an `os_lite/{services,updated,ipc_kernel,security,elf,...}/*` topology.

## Current Phase-1 structural state (verified green)
- `source/apps/selftest-client/src/main.rs` = 122 lines (Phase-1 minimal target).
- `source/apps/selftest-client/src/os_lite/mod.rs` ≈ 2025 lines.
- Already extracted modules under `source/apps/selftest-client/src/os_lite/`:
  - `dsoftbus/{quic_os, remote/{mod,resolve,pkgfs,statefs}}`
  - `net/{icmp_ping, local_addr, smoltcp_probe (cfg-gated)}`
  - `ipc/{clients, routing, reply}`
  - `mmio/`, `vfs/`, `timed/`, `probes/{rng, device_key}`
  - `services/{samgrd, bundlemgrd, keystored, policyd, execd, logd, metricsd, statefs, bootctl}/mod.rs` (+ `services/mod.rs` shared `core_service_probe*` helpers)

## Remaining Phase-1 movables in `os_lite/mod.rs` (next session targets)
- `updated` family: `updated_stage`, `updated_log_probe`, `updated_switch`, `updated_get_status`, `updated_boot_attempt`, `init_health_ok`, `updated_expect_status`, `updated_send_with_reply` (+ `SYSTEM_TEST_NXS` const)
- IPC-kernel / security probes: `qos_probe`, `ipc_payload_roundtrip`, `ipc_deadline_timeout_probe`, `nexus_ipc_kernel_loopback_probe`, `cap_move_reply_probe`, `sender_pid_probe`, `sender_service_id_probe`, `ipc_soak_probe`
- ELF helpers: `log_hello_elf_header`, `read_u64_le`
- `emit_line` shim consolidation (currently still in `os_lite/mod.rs`)

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
- `pub fn run()` body: only call-site repath (`xyz()` → `module::xyz()`); no reordering, no marker rename, no `unwrap`/`expect` introduction. `run()`-slicing into sub-orchestrators is explicitly Phase 2.
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
- Start from this frozen green baseline (Cuts 0–18 merged) and request Plan Mode to design the final batch of `TASK-0023B` Phase-1 cuts:
  - propose dedicated module homes for the `updated` family (`os_lite/updated/`), the IPC-kernel/security probes (`os_lite/ipc_kernel/` or `os_lite/probes/{ipc_kernel,security}/`), and the ELF helpers (`os_lite/elf/` or merged into an existing peripheral module if more natural),
  - sequence the cuts so cross-references stay mechanical (`updated` first because it absorbs `SYSTEM_TEST_NXS` and `init_health_ok`; IPC-kernel/security probes after because they share helpers like `cached_*_client`),
  - rerun the Phase-1 Proof-Floor after every cut,
  - stop immediately on marker/order/reject-path drift.
- Closure of full Phase 1 (after these cuts) will trigger the deferred STATUS-BOARD / IMPLEMENTATION-ORDER updates and the Phase-2 plan (`run()` slicing into sub-orchestrators).
