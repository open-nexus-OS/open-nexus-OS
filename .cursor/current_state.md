# Cursor Current State (SSOT)

<!--
CONTEXT
This file is the single source of truth for the current system state.
Keep it compact, explicit, and contract-oriented.
-->

## Current architecture state
- **last_decision**: archive `TASK-0020` handoff and prepare execution state for `TASK-0021` in strict numerical order.
- **rationale**:
  - keep queue-head metadata aligned with execution SSOT,
  - preserve proven `TASK-0020` closure as immutable baseline,
  - start QUIC work host-first without destabilizing default TCP path.
- **active_constraints**:
  - kernel untouched in this slice,
  - `TASK-0021` must not absorb `TASK-0022` core/no_std extraction scope,
  - strict QUIC mode must fail closed (no silent downgrade),
  - `auto` fallback must be deterministic and auditable via stable markers.

## Current focus (execution)
- **active_task**: `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md` (`In Progress`, kickoff slice)
- **seed_contract**:
  - `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
  - `docs/rfcs/RFC-0035-dsoftbus-quic-v1-host-first-os-scaffold.md`
  - `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
  - `docs/adr/0005-dsoftbus-architecture.md`
  - `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md` (`Done` baseline)
- **contract_dependencies**:
  - `tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md`
  - `tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md`
  - `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md`
  - `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`
  - `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md` (boundary only)
  - `scripts/qemu-test.sh`
  - `tools/os2vm.sh`
  - `docs/testing/index.md`
- **phase_now**: `TASK-0020` is fully closed (`Done`); `TASK-0021` is `In Progress` and phase-A contract lock is seeded in `RFC-0035`.
- **baseline_commit**: `a996549` (TASK-0020 status finalized to Done)
- **next_task_slice**:
  - execute phase-B host behavior proofs (smallest honest suite first),
  - lock `auto|tcp|quic` selection semantics and strict-mode fail-closed behavior,
  - keep OS QUIC disabled-by-default with explicit fallback markers.

## Last completed
- `TASK-0019` archived and done:
  - archive: `.cursor/handoff/archive/TASK-0019-security-v2-userland-abi-syscall-filters.md`
  - status: done with green host/OS/QEMU proofs.
- `TASK-0020` archived and done:
  - archive: `.cursor/handoff/archive/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`
  - status: done with green host/OS/distributed/perf/soak/release-evidence proofs.
- `TASK-0018` handoff remains archived:
  - archive: `.cursor/handoff/archive/TASK-0018-crashdumps-v1-minidump-host-symbolize.md`
  - status: done with completed proofs and closeout commits.
- `TASK-0017` remains `Done`.

## Proof baseline currently green
- `TASK-0017`/`TASK-0018`/`TASK-0019` closure baselines remain green.
- `TASK-0020` closure baselines remain green:
  - `just test-dsoftbus-mux`
  - `just test-dsoftbus-host`
  - `RUN_UNTIL_MARKER=1 just test-os`
  - `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- quality floor is green:
  - `scripts/fmt-clippy-deny.sh`
  - `just fmt-check && just lint`
  - `just test-all`

## Active invariants (must hold)
- **security**
  - strict QUIC mode rejects downgrade paths,
  - transport-only success never bypasses DSoftBus authenticated session semantics,
  - ALPN/cert validation failures are deterministic rejects (no warn-and-continue).
- **determinism**
  - stable transport-selection markers for `auto|tcp|quic`,
  - explicit fallback markers when QUIC remains OS-disabled,
  - canonical marker/harness discipline only.
- **scope hygiene**
  - keep `TASK-0021` implementation separate from `TASK-0022` extraction work,
  - execute tasks one-by-one in general order (no preemption of `TASK-0030+`),
  - preserve completed legacy closure scope (`RFC-0034` is limited to `TASK-0001..0020`).

## Open threads / follow-ups
- `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
- `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md`

## Open items (current slice)
- keep task/rfc contract parity between `TASK-0021` and `RFC-0035`.
- lock touched-path allowlist and test plan for phase A/B.
- align `.cursor/pre_flight.md` + `.cursor/stop_conditions.md` checklists with TASK-0021 + behavior-first proof gates.

## DON'T DO (session-local)
- DON'T silently absorb `TASK-0022` core/no_std split into `TASK-0021`.
- DON'T silently downgrade `mode=quic` to TCP.
- DON'T emit QUIC success/fallback markers without real transport selection.
- DON'T destabilize default TCP bring-up while QUIC is OS-disabled.
