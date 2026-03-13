# Current Handoff: TASK-0015 dsoftbusd refactor v1

**Date**: 2026-03-12  
**Status**: `TASK-0015` is `Completed` (RFC-0027 Phase 3 complete; full sequential gate green).  
**Contract seed**: `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`

---

## What is stable now

- `TASK-0014` remains closed and archived at `.cursor/handoff/archive/TASK-0014-observability-v2-metrics-tracing.md`.
- `TASK-0015` slices 1+2+3A+3B are landed:
  - `source/services/dsoftbusd/src/os/mod.rs`
  - `source/services/dsoftbusd/src/os/entry.rs`
  - `source/services/dsoftbusd/src/os/observability.rs`
  - `source/services/dsoftbusd/src/os/service_clients.rs`
  - `source/services/dsoftbusd/src/os/netstack/{mod.rs,ids.rs,rpc.rs,stream_io.rs}`
  - `source/services/dsoftbusd/src/os/session/{mod.rs,fsm.rs,handshake.rs,records.rs,single_vm.rs,selftest_server.rs,cross_vm.rs}`
  - `source/services/dsoftbusd/src/os/discovery/{mod.rs,state.rs}`
  - `source/services/dsoftbusd/src/os/gateway/{mod.rs,local_ipc.rs,remote_proxy.rs}`
  - `source/services/dsoftbusd/src/main.rs` reduced to entry/wiring shell (now ~85 LOC) with high-level routing only.
  - additional bootstrap delegation into `source/services/dsoftbusd/src/os/entry.rs` (slot wait, local-ip wait, UDP bind retry, listen retry) and orchestration extraction into `src/os/session/*` runners.
- Cross-VM harness service-list parity was synced in `tools/os2vm.sh` by adding `metricsd`, restoring deterministic `SELFTEST: metrics ...` behavior in 2-VM runs.
- Proof floor (including completion-gate tests) was executed successfully:
  - `cargo test -p dsoftbusd -- --nocapture`
  - `cargo test -p remote_e2e -- --nocapture`
  - `just dep-gate`
  - `just diag-os`
  - `just diag-host`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`

## Current focus

- `TASK-0015` is closed; keep the resulting seams stable while follow-on tasks start.
- Next intended task is `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md` (consume stabilized transport/session/gateway boundaries without reopening monolithic orchestration).

## Relevant contracts and linked work

- Task SSOT:
  - `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md`
- RFC/ADR/docs:
  - `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
  - `docs/adr/0005-dsoftbus-architecture.md`
  - `docs/distributed/dsoftbus-lite.md`
  - `docs/testing/index.md`
- Baseline dependency tasks:
  - `tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md`
  - `tasks/TASK-0003B-dsoftbus-noise-xk-os.md`
  - `tasks/TASK-0003C-dsoftbus-udp-discovery-os.md`
  - `tasks/TASK-0004-networking-dhcp-icmp-dsoftbus-dual-node.md`
  - `tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md`
- Follow-on tasks:
  - `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md`
  - `tasks/TASK-0017-dsoftbus-remote-statefs-rw.md`
  - `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`
  - `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
  - `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md`

## Immediate next slice

1. Kick off `TASK-0016` on top of the stabilized `dsoftbusd` module seams.
2. Preserve marker/wire semantics and fail-closed boundaries established by `TASK-0015`.
3. Keep sequential proof discipline for any DSoftBus follow-on touching OS transport/session paths.

## Guardrails

- No fake success markers.
- No wire format / ABI / marker semantics changes.
- No `netstackd` behavior or ownership changes.
- No shared-core extraction into `userspace/dsoftbus` in this task.
- Keep retry budgets / nonce-correlation / marker timing semantics intact.
- Run QEMU proofs sequentially only.
