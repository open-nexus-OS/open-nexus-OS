# Current Handoff: TASK-0021 QUIC scaffold (ready)

**Date**: 2026-04-10  
**Status**: `TASK-0020` is archived and `Done`; queue head is now `TASK-0021` (start in strict order).  
**Contract baseline**: `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md` (`In Progress`, execution SSOT for current slice)

---

## What is stable now

- `TASK-0020` closeout is archived:
  - archive: `.cursor/handoff/archive/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`
  - task: `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md` (`Done`)
  - status sync: `tasks/STATUS-BOARD.md` + `tasks/IMPLEMENTATION-ORDER.md` updated to `Done`
- Legacy closure contract remains locked:
  - `docs/rfcs/RFC-0033-dsoftbus-streams-v2-mux-flow-control-keepalive.md` (`Done`)
  - `docs/rfcs/RFC-0034-dsoftbus-production-closure-v1.md` (`Done`, scope limited to `TASK-0001..0020`)
- QEMU and distributed harnesses are stable after closeout:
  - `just test-os-dhcp`
  - `just test-os-dhcp-strict`
  - `just test-dsoftbus-2vm`
  - `just test-all`

## Carry-forward proof baseline

- `TASK-0020` requirement suites remain green:
  - `just test-dsoftbus-mux`
  - `just test-dsoftbus-host`
- OS + distributed proofs remain green:
  - `RUN_UNTIL_MARKER=1 just test-os`
  - `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- Current quality floor remains green:
  - `scripts/fmt-clippy-deny.sh`
  - `just fmt-check && just lint`

## Active contracts for TASK-0021 start

- `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
- `docs/rfcs/RFC-0035-dsoftbus-quic-v1-host-first-os-scaffold.md`
- `docs/adr/0005-dsoftbus-architecture.md`
- `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
- `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md` (`Done`, transport substrate baseline)
- `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md` (boundary only; do not absorb)
- `docs/testing/index.md`
- `scripts/qemu-test.sh`
- `tools/os2vm.sh`

## Next actions

1. Execute `TASK-0021` phase B (behavior-first host proofs for selection + reject paths).
2. Keep scope host-first (`auto|tcp|quic`) and fail-closed for strict QUIC mode.
3. Keep OS QUIC disabled-by-default with deterministic fallback markers only.
4. Do not absorb `TASK-0022` core/no_std extraction scope.
