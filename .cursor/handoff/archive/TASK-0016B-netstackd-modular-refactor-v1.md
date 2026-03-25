# Current Handoff: TASK-0016B netstackd modular refactor + optimization + address-governance sync (implemented)

**Date**: 2026-03-24  
**Status**: `TASK-0016B` implementation + optimization + address-governance sync complete; proofs green, ready for follow-on tasks.  
**Contract baseline**: `docs/rfcs/RFC-0029-netstackd-modular-daemon-structure-v1.md` (`Complete`)

---

## What is stable now

- `main.rs` is thin entry/wiring (`emit_ready_marker` -> `bootstrap_network` -> `run_facade_loop`).
- Former runtime monolith was split into:
  - `source/services/netstackd/src/os/facade/runtime.rs` (orchestration loop only)
  - `source/services/netstackd/src/os/facade/state.rs`
  - `source/services/netstackd/src/os/facade/dispatch.rs`
  - `source/services/netstackd/src/os/facade/handlers/*.rs`
- UDP handlers are now split into dedicated submodules:
  - `source/services/netstackd/src/os/facade/handlers/udp/bind.rs`
  - `source/services/netstackd/src/os/facade/handlers/udp/send_to.rs`
  - `source/services/netstackd/src/os/facade/handlers/udp/recv_from.rs`
- IPC helpers were consolidated in:
  - `source/services/netstackd/src/os/ipc/parse.rs`
  - `source/services/netstackd/src/os/ipc/reply.rs`
- Typed boundary hardening landed:
  - `StreamId` used for loopback peer/pending state
  - typed reply-cap newtype (`ReplyCapSlot`) used in facade context/runtime path
- Determinism/observability hardening landed:
  - `SELFTEST: net udp dns ok` only on real DNS success
  - DNS miss marker: `netstackd: net dns proof fail`
  - additive MMIO/net fail-code markers: `netstackd: net fail-code 0x....`
  - stable halt-reason markers before intentional park loops
- Modular host tests now include:
  - `source/services/netstackd/tests/ipc_parse_reply.rs`
  - `source/services/netstackd/tests/handler_rejects.rs`
- Address-profile governance is now explicit and centralized:
  - `docs/architecture/network-address-matrix.md`
  - `docs/adr/0026-network-address-profiles-and-validation.md`
- Runtime code now uses centralized address-profile constants (instead of scattered literals) in:
  - `source/services/netstackd/src/os/entry_pure.rs` + facade/bootstrap callsites
  - `source/services/dsoftbusd/src/os/{entry,entry_pure}.rs`
  - `source/services/dsoftbusd/src/os/session/{single_vm,cross_vm}.rs`
- Proof gates are green:
  - `cargo test -p netstackd --tests -- --nocapture`
  - `cargo test -p dsoftbusd --tests -- --nocapture`
  - `just dep-gate`
  - `just diag-os`
  - `just test-os-dhcp-strict`
  - `RUN_OS2VM=1 RUN_TIMEOUT=180s OS2VM_PROFILE=ci RUN_PHASE=end tools/os2vm.sh` (`summary: result=success`)

## Current focus

- Handoff stable seams to networking follow-ons (`TASK-0194`, `TASK-0196`, `TASK-0249`) without widening scope in this completed task.

## Relevant contracts and linked work

- Active task:
  - `tasks/TASK-0016B-netstackd-refactor-v1-modular-os-daemon-structure.md`
- Baseline contracts/docs:
  - `docs/rfcs/RFC-0029-netstackd-modular-daemon-structure-v1.md`
  - `docs/rfcs/RFC-0006-userspace-networking-v1.md`
  - `docs/rfcs/RFC-0017-device-mmio-access-model-v1.md`
  - `docs/adr/0005-dsoftbus-architecture.md`
  - `docs/adr/0025-qemu-smoke-proof-gating.md`
  - `docs/adr/0026-network-address-profiles-and-validation.md`
  - `docs/architecture/network-address-matrix.md`
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

1. Start `TASK-0194` with the stabilized facade boundaries and typed handle state model.
2. Keep marker contract honest-green (`ok` only on proven behavior) while extending functionality.
3. Reuse consolidated reply/validation/tcp retry helpers instead of reintroducing per-handler duplication.

## Guardrails

- No fake success markers.
- Keep `netstackd` wire and marker semantics stable unless task/RFC evidence updates contracts explicitly.
- Keep `netstackd` as the networking owner; no duplicate authority or MMIO bypass path.
- Keep bounded retry/failure policy explicit; do not replace it with hidden unbounded helpers.
- Keep QEMU proofs sequential only.
