# Cursor Current State (SSOT)

<!--
CONTEXT
This file is the single source of truth for the current system state.
Keep it compact, explicit, and contract-oriented.
-->

## Current architecture state
- **last_decision**: move `TASK-0020` and `RFC-0033` from draft setup into active in-progress execution with task-as-SSOT.
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
- **active_task**: `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md` (In Progress, phase-0 contract/determinism lock)
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
- **phase_now**: `TASK-0020` and `RFC-0033` are In Progress; active work is phase-0 lock and host-first proof implementation.
- **baseline_commit**: `74c50a6` (TASK-0019 done closeout commit)
- **next_task_slice**:
  - keep `TASK-0020` host-first and OS-gated while `userspace/dsoftbus` OS backend remains placeholder,
  - execute phase 0 contract/determinism lock before mux feature growth,
  - keep transport evolution/core extraction scope in `TASK-0021`/`TASK-0022`.

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
- `TASK-0020` proofs are pending while implementation is in progress (no completion claims yet).

## Active invariants (must hold)
- **security**
  - mux runs only on authenticated session context,
  - deterministic fail-closed stream/window/credit validation,
  - no hidden unbounded buffering paths.
- **determinism**
  - stable reject labels and bounded retry budgets,
  - canonical marker/harness discipline only.
- **scope hygiene**
  - keep `TASK-0020` separate from `TASK-0021` and `TASK-0022`,
  - do not claim OS mux closure before OS backend gate is actually met.

## Open threads / follow-ups
- `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`
- `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
- `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md`

## DON'T DO (session-local)
- DON'T silently absorb QUIC (`TASK-0021`) or core/no_std extraction (`TASK-0022`) scope.
- DON'T add unbounded stream/window/credit behavior.
- DON'T emit mux success markers before real multiplexed roundtrip proof.
- DON'T introduce unsafe `Send`/`Sync` shortcuts for mux/session state.
