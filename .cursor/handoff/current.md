# Current Handoff: TASK-0016 remote packagefs RO kickoff

**Date**: 2026-03-12  
**Status**: `TASK-0015` is `Done`; `TASK-0016` is the active next execution slice.  
**Contract baseline**: `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md` (`Completed`)

---

## What is stable now

- `TASK-0015` was closed and archived at `.cursor/handoff/archive/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md`.
- `dsoftbusd` structure is stabilized for follow-on work:
  - thin `source/services/dsoftbusd/src/main.rs`,
  - orchestration split across `source/services/dsoftbusd/src/os/{entry,session,gateway,netstack,observability,...}`.
- Security-negative seam tests are in place (`p0_unit`, `reject_transport_validation`, `session_steps`).
- Latest proof floor is green:
  - `just dep-gate`
  - `just diag-os`
  - `just diag-host`
  - `just test-all`
  - `make build`
  - `make test`
  - `make run`

## Current focus

- Start `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md` on top of stabilized seams.
- Keep scope strict: remote packagefs RO protocol/handler behavior only (no mux/quic/core split spillover).

## Relevant contracts and linked work

- Active task:
  - `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md`
- Baseline contracts/docs:
  - `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
  - `docs/adr/0005-dsoftbus-architecture.md`
  - `docs/distributed/dsoftbus-lite.md`
  - `docs/testing/index.md`
- Dependency tasks:
  - `tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md`
  - `tasks/TASK-0003B-dsoftbus-noise-xk-os.md`
  - `tasks/TASK-0003C-dsoftbus-udp-discovery-os.md`
  - `tasks/TASK-0004-networking-dhcp-icmp-dsoftbus-dual-node.md`
  - `tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md`
  - `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md`
- Follow-ons (do not absorb into this slice):
  - `tasks/TASK-0017-dsoftbus-remote-statefs-rw.md`
  - `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`
  - `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
  - `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md`

## Immediate next slice

1. Finalize `TASK-0016` protocol surface for bounded RO packagefs calls (`STAT/OPEN/READ/CLOSE`).
2. Implement server handler via `dsoftbusd` gateway/session seams (no `main.rs` growth).
3. Add security-negative tests (`test_reject_*`) for unauthenticated/path traversal/non-scheme/oversize requests.
4. Prove host-first, then sequential single-VM + 2-VM markers.

## Guardrails

- No fake success markers.
- No write opcodes for packagefs in this task.
- Reject non-`pkg:/` and non-`/packages/` paths deterministically.
- Keep wire/marker semantics stable unless task evidence updates contracts explicitly.
- Keep QEMU proofs sequential only.
