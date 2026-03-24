# Current Handoff: TASK-0016B netstackd modular refactor kickoff

**Date**: 2026-03-24  
**Status**: `TASK-0016B` active; task and RFC seed created and now marked `In Progress`.  
**Contract baseline**: `docs/rfcs/RFC-0029-netstackd-modular-daemon-structure-v1.md` (`In Progress`)

---

## What is stable now

- `TASK-0016` handoff state was archived to `.cursor/handoff/archive/TASK-0016-remote-packagefs-ro.md`.
- New execution task exists:
  - `tasks/TASK-0016B-netstackd-refactor-v1-modular-os-daemon-structure.md`
- New contract seed exists:
  - `docs/rfcs/RFC-0029-netstackd-modular-daemon-structure-v1.md`
- RFC index was updated to include `RFC-0029`.
- SSOT cursor files now point to `TASK-0016B` as the next active task.

## Current focus

- Start `TASK-0016B` Phase 0: modularize `source/services/netstackd/src/main.rs` without changing marker or wire semantics.
- Keep scope strict: structural extraction first, then bounded loop/idiom hardening inside the same task.

## Relevant contracts and linked work

- Active task:
  - `tasks/TASK-0016B-netstackd-refactor-v1-modular-os-daemon-structure.md`
- Baseline contracts/docs:
  - `docs/rfcs/RFC-0029-netstackd-modular-daemon-structure-v1.md`
  - `docs/rfcs/RFC-0006-userspace-networking-v1.md`
  - `docs/rfcs/RFC-0017-device-mmio-access-model-v1.md`
  - `docs/adr/0005-dsoftbus-architecture.md`
  - `docs/testing/index.md`
  - `docs/testing/network-distributed-debugging.md`
- Dependency / related tasks:
  - `tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md`
  - `tasks/TASK-0010-device-mmio-access-model.md`
  - `tasks/TASK-0249-bringup-rv-virt-v1_2b-os-virtionetd-netstackd-fetchd-echod-selftests.md`
- Follow-ons (do not absorb into this slice):
  - `tasks/TASK-0194-networking-v1b-os-devnet-gated-real-connect.md`
  - `tasks/TASK-0196-dsoftbus-v1_1b-devnet-udp-discovery-gated.md`

## Immediate next slice

1. Create the `source/services/netstackd/src/os/` skeleton.
2. Move bootstrap, marker helpers, and internal state/context ownership out of `main.rs`.
3. Reduce `main.rs` to environment selection + entry wiring.
4. Keep all existing `netstackd` and `SELFTEST` markers semantically unchanged.
5. Add the first narrow host tests once pure/near-pure seams exist.

## Guardrails

- No fake success markers.
- Keep `netstackd` wire and marker semantics stable unless task/RFC evidence updates contracts explicitly.
- Keep `netstackd` as the networking owner; no duplicate authority or MMIO bypass path.
- Keep bounded retry/failure policy explicit; do not replace it with hidden unbounded helpers.
- Keep QEMU proofs sequential only.
