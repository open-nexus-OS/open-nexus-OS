# Cursor Current State (SSOT)

<!--
CONTEXT
This file is the single source of truth for the current system state.
Keep it compact, explicit, and contract-oriented.
-->

## Current architecture state
- **last_decision**: close `TASK-0020` with proven legacy-production gates and lock `RFC-0033`/`RFC-0034` as complete for 0001..0020 scope.
- **rationale**:
  - preserve low-drift sequencing after `TASK-0019` closeout,
  - lock mux/flow-control contract boundaries before implementation growth,
  - keep host-first execution while OS backend remains explicitly gated.
- **active_constraints**:
  - kernel untouched in this slice,
  - no scope absorption from `TASK-0021` (QUIC) or `TASK-0022` (core/no_std split),
  - bounded stream/window/credit behavior with deterministic reject paths,
  - explicit ownership + Rust API hygiene (`newtype`, `#[must_use]`, no unsafe `Send`/`Sync` shortcuts),
  - OS proofs only via canonical harnesses with modern virtio-mmio defaults.

## Current focus (execution)
- **active_task**: `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md` (Done, legacy 0001..0020 production-closure gates proven)
- **seed_contract**:
  - `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`
  - `docs/rfcs/RFC-0033-dsoftbus-streams-v2-mux-flow-control-keepalive.md`
  - `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
  - `docs/adr/0005-dsoftbus-architecture.md`
- **contract_dependencies**:
  - `tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md`
  - `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md`
  - `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md`
  - `tasks/TASK-0016B-netstackd-refactor-v1-modular-os-daemon-structure.md`
  - `tasks/TASK-0017-dsoftbus-remote-statefs-rw.md`
  - `scripts/qemu-test.sh`
  - `docs/testing/index.md`
- **phase_now**: `TASK-0020` is `Done`, `RFC-0033` is `Complete`, and `RFC-0034` legacy-scope closure for `TASK-0001..0020` is `Complete`.
- **baseline_commit**: `74c50a6` (TASK-0019 done closeout commit)
- **next_task_slice**:
  - start `TASK-0021` in strict numerical order,
  - keep `TASK-0021` host-first and scoped (no absorption of `TASK-0022` core split),
  - preserve no-fake-success harness discipline established in TASK-0020 closure.

## Last completed
- `TASK-0019` archived and done:
  - archive: `.cursor/handoff/archive/TASK-0019-security-v2-userland-abi-syscall-filters.md`
  - status: done with green host/OS/QEMU proofs.
- `TASK-0018` handoff remains archived:
  - archive: `.cursor/handoff/archive/TASK-0018-crashdumps-v1-minidump-host-symbolize.md`
  - status: done with completed proofs and closeout commits.
- `TASK-0017` remains `Done`.

## Proof baseline currently green
- `TASK-0017`/`TASK-0018`/`TASK-0019` closure baselines remain green.
- `TASK-0020` requirement-based host proofs are green:
  - `cargo test -p dsoftbus --test mux_contract_rejects_and_bounds -- --nocapture`
  - `cargo test -p dsoftbus --test mux_frame_state_keepalive_contract -- --nocapture`
  - `cargo test -p dsoftbus --test mux_open_accept_data_rst_integration -- --nocapture`
  - `cargo test -p dsoftbus -- --nocapture`
  - per-slice regressions: `just test-e2e`, `just test-os-dhcp`
- OS harnesses executed:
  - `RUN_UNTIL_MARKER=1 just test-os` (green),
  - `just test-dsoftbus-2vm` (green summary artifacts reviewed: `os2vm_1775990226`).
- `TASK-0020` OS/QEMU mux marker ladder is proven in canonical smoke with `REQUIRE_DSOFTBUS=1`.
- `TASK-0020` distributed mux ladder is proven via `tools/os2vm.sh` (`phase: mux`) with run evidence `os2vm_1775990226`.
- `TASK-0020` deterministic distributed performance budget gate is proven via `tools/os2vm.sh` (`phase: perf`) with summary-contained observed vs budget metrics.
- `TASK-0020` bounded distributed soak hardening gate is proven via `tools/os2vm.sh` (`phase: soak`) with zero fail/panic marker hits across two soak rounds.
- `TASK-0020` release evidence bundle is emitted per green 2-VM run (`release-evidence.json`).
- legacy closure gates were executed under `TASK-0020` (no preemption).

## Active invariants (must hold)
- **security**
  - mux runs only on authenticated session context,
  - deterministic fail-closed stream/window/credit validation,
  - no hidden unbounded buffering paths.
- **determinism**
  - stable reject labels and bounded retry budgets,
  - canonical marker/harness discipline only.
- **scope hygiene**
  - keep `TASK-0021` separate from `TASK-0022`,
  - execute tasks one-by-one in general order (no preemption of `TASK-0030+`),
  - preserve completed legacy closure scope (`RFC-0034` is limited to `TASK-0001..0020`).

## Open threads / follow-ups
- `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
- `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md`

## Open items (current slice)
- none (current slice closed; next execution target is `TASK-0021` in normal order).

## DON'T DO (session-local)
- DON'T silently absorb QUIC (`TASK-0021`) or core/no_std extraction (`TASK-0022`) scope.
- DON'T add unbounded stream/window/credit behavior.
- DON'T emit mux success markers before real multiplexed roundtrip proof.
- DON'T introduce unsafe `Send`/`Sync` shortcuts for mux/session state.
