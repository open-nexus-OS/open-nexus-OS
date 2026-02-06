# Current Handoff: IPC Correlation v1 + QEMU modern virtio-mmio (RFC-0019)

**Date**: 2026-02-06  
**Goal**: Make QEMU smoke gates deterministic end-to-end by fixing the root cause: **request/reply correlation** under shared inboxes, while keeping virtio-mmio policy drift-free.

---

## What changed (contract-level)

- **New RFC**: `docs/rfcs/RFC-0019-ipc-request-reply-correlation-v1.md`
  - Defines nonce-based request/reply correlation + a bounded dispatcher for shared inboxes.
  - Also owns the normative **QEMU harness policy** for modern virtio-mmio default (legacy opt-in only).
- **RFC-0018 note**: `docs/rfcs/RFC-0018-statefs-journal-format-v1.md` now links to RFC-0019 for correlation, to keep StateFS v1 scope drift-free.
- **Host determinism tests**:
  - `userspace/nexus-ipc` now contains deterministic host tests for:
    - budgeted non-blocking IPC loops (`nexus_ipc::budget`)
    - logd v1+v2 wire parsing (`nexus_ipc::logd_wire`, including LO v2 nonce frames)
    - policyd v2/v3 response decoding (`nexus_ipc::policyd_wire`)
  - Proof: `cargo test -p nexus-ipc`

## Current reality (QEMU smoke)

- **Modern virtio-mmio**:
  - Canonical harness (`scripts/run-qemu-rv64.sh`) defaults to `-global virtio-mmio.force-legacy=off`.
  - Legacy mode remains opt-in via `QEMU_FORCE_LEGACY=1` (debug/bisect).
- **What is green now**:
  - Default `RUN_TIMEOUT=90s just test-os` reaches `SELFTEST: end` (after fixing init-lite wiring races, shared reply inbox filtering in statefs client, and updated bootctrl persist corruption).
  - `RUN_TIMEOUT=90s REQUIRE_QEMU_DHCP=1 just test-os` is green (non-strict policy accepts honest static fallback when DHCP does not bind).
  - `RUN_TIMEOUT=90s REQUIRE_QEMU_DHCP=1 REQUIRE_QEMU_DHCP_STRICT=1 just test-os` is green and proves **DHCP bound** (RX works again) + dependent proofs (`SELFTEST: net ping ok`, `SELFTEST: net udp dns ok`, `SELFTEST: icmp ping ok`).

## Additional progress since last handoff update

- **StateFS shared-inbox correlation**:
  - `userspace/statefs` now supports `SF v2` frames with an explicit `nonce:u64` after the header.
  - `statefsd` echoes the nonce in replies and the OS-lite client uses it to deterministically match replies on the shared `@reply` inbox (removes “drain stale replies” from the StateFS proof path).
- **Routing ctrl-plane determinism**:
  - init-lite routing responder now supports a backwards-compatible **routing v1+nonce extension**.
  - Key clients (`bundlemgrd` route-status, `rngd`, `statefsd`, `logd` bootstrap routing, `selftest-client` probes) use it to avoid stale-drain patterns on ctrl slot 2.
- **Shared `@reply` hygiene**:
  - `samgrd` and `bundlemgrd` “core service log probe” paths now deterministically wait for the logd APPEND ACK (bounded), preventing reply inbox buildup.
  - `rngd` uses policyd **v2 delegated-cap** replies (nonce-correlated) with strict matching on the shared inbox.
- **logd LO v2 (nonce frames)**:
  - logd now supports LO v2 nonce-correlated frames for APPEND/QUERY/STATS, enabling safe multiplexing over a shared reply inbox.
  - QEMU proof paths (`RUN_PHASE=logd`) use strict nonce matching (no stale-drain patterns).

## Why this is the root cause

- Without a nonce echoed in replies, any multi-step or concurrent IPC over a shared inbox can desync.
- “Drain/yield/budget” loops reduce flakiness but do not make matching **correct-by-construction**.
- RFC-0019 standardizes the correct pattern so future OS work does not repeat the same class of bugs.

## Next steps (implementation slice; task-owned)

1. **DHCP determinism decision (harness policy)**:
   - Keep `REQUIRE_QEMU_DHCP=1` permissive (accept deterministic static fallback) as the default CI proof.
   - Gate `REQUIRE_QEMU_DHCP_STRICT=1` only on backends/environments where inbound RX is proven deterministic (or after a QEMU/backend change that restores RX).
2. **RFC-0019 adoption audit (keep it honest)**:
   - Inventory which clients still rely on uncorrelated replies on shared inboxes.
   - Convert remaining multi-step flows to nonce-correlated request/reply (or explicit “no-reply” fire-and-forget).

## Drift guards (do not regress)

- QEMU runs used for proofs MUST continue to default to modern virtio-mmio (`virtio-mmio.force-legacy=off`).
- Any new OS service protocol that expects a reply on a shared inbox MUST adopt RFC-0019 nonce correlation (or a successor RFC).
- Do not run multiple QEMU smoke runs concurrently (they contend on `build/blk.img` and can trip QEMU “write lock” errors).
