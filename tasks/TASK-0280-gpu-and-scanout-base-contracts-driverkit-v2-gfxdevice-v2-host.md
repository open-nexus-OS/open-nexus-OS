---
title: TASK-0280 Block 4 layer 1 — the base: the GPU and scanout contracts as host-tested types (driver-kit v2, GfxDevice v2, the one timeline, the plane ring)
status: Done 2026-10-12 (P0–P5; recut the same day from "DriverKit v1 core contracts" of 2026-01-09 — the v1 ring, budgets, QoS and DmaBuffer are delivered in `nexus-driverkit` by RFC-0033 Phase 3 and TASK-0286 P4b; this ledger is the layer-1 SSOT)
owner: @gpu @runtime @drivers
created: 2026-01-09
depends-on:
  - TASK-0286
  - TASK-0329
follow-up-tasks:
  - TASK-0216
  - TASK-0329
  - TASK-0169B
  - TASK-0170B
  - TASK-0171
  - TASK-0215
links:
  - RFC: docs/rfcs/RFC-0101-gpu-driver-split-contexts-vm-batches-fence-and-the-scanout-contract.md
  - RFC (fence, driver-kit): docs/rfcs/RFC-0033-soft-real-time-spine-waitset-fence-driverkit.md
  - RFC (compositor boundary): docs/rfcs/RFC-0067-windowd-compositor-boundary.md
  - ADR: docs/adr/0018-driverkit-abi-versioning-and-stability.md
  - Order: tasks/IMPLEMENTATION-ORDER.md ("Block 4 in layers", layer 1)
  - Measurements: docs/board/measurements/2026-10-10-mgs-baseline/README.md, docs/board/measurements/2026-10-10-gpu-stock-baseline/README.md
  - Playbook: CLAUDE.md
---

## Context

- Block 4's layer 0 measured the board (2026-10-10): a present is a CPU composite drawn into the scanned plane, 150 ms median; the same scene through the stock GPU stack costs 60 ms / 23 ms; a frame's fixed cost is 0.64 ms and opaque fill 1.8 ns a pixel. The order file's recut of 2026-10-12 puts **the contracts first, as one host-only layer**, so that each later package (the scanout contract on the board, the device online, the GPU draws, the desktop on the GPU) is built from its contract by whoever picks it up.
- Today: gpud runs one context and only windowd has a route; the v1 `GfxBackend` trait has no buffers, no binding, no queue kinds and a boolean `Fence`; `nexus_gfx::sync` and `resource` are placeholders; `nexus-driverkit` has the ring, the budget, QoS, `DmaBuffer` and `DmaShared` — but no GPU address space, no cache-mode vocabulary, no capabilities.
- **Must not change**: the v1 trait and its users (gpud's CPU, 2D and virgl backends, windowd) keep working until Phase 3 retires them; the display protocol's wire ops (Phase 1 extends them); the kernel's fence syscalls (RFC-0033).

## Three lenses (architecture-review)

- **Scope** — in: `docs/rfcs/RFC-0101-…` (seed), this ledger, `source/libs/nexus-driverkit/src/gpu.rs` (+ `lib.rs` export), `userspace/nexus-gfx/src/sync/timeline.rs`, `userspace/nexus-gfx/src/backend/v2/{mod,scanout,cpu_mock}.rs` (+ `backend/mod.rs`, `Cargo.toml` dependency on `nexus-driverkit`), the indexes. Out, explicitly: any OS code, any wire op, gpud, windowd, the v1 trait's deletion (Phase 3), the `CpuSet` RFC and RFC-0100 seeds (written at S1's and M2's P0 from their own measurements).
- **Invariant** — isolation by the GPU VM: a batch may reference only addresses bound in its own context, a buffer binds only in its owner's contexts; every buffer carries a cache mode by type; the scanned slot is never a render target; the one fence is monotonic. Proven by the reject matrix of RFC-0101 §9 (18 `test_reject_*` by name).
- **Contract** — RFC-0101 (new seed), extending RFC-0033 (fence, driver-kit), RFC-0098 C4 (VMO kinds, coherence) and RFC-0067 (the compositor boundary; the wire form in Phase 1 amends it). No new service, no new syscall, no new cfg.

## Goal

The base for the GPU lane as types with tests: **what a client of the GPU can do** (`GfxDevice` v2: capabilities, contexts, buffers with a cache mode, binding, one batch per submit, the timeline point, the plane ring), **what every accelerator server shares** (`nexus_driverkit::gpu`: queue kinds, priorities, cache modes, the GPU address space with its reject rules, capabilities, the batch bound), **the one sync primitive** modelled (`nexus_gfx::sync::Timeline`), and a **mock device** implementing all of it so the compositor's and the driver's later packages have an oracle on the host.

## Non-Goals

Pixels through the new path (the mock executes the one existing command vocabulary into a bound buffer only as far as proving the data flow); the wire form; gpud or windowd changes; the vendor command encoding; a second command, buffer or fence vocabulary.

## Packages

- **P0 — the contract.** RFC-0101 seeded with §1–§8, the reject matrix §9, the phase table. ✅ 2026-10-12.
- **P1 — driver-kit v2.** `nexus_driverkit::gpu`: `QueueKind`, `Priority`, `CacheMode`, `GpuVa`, `Access`, `VmSpace<N>` (a bounded, alloc-free address space: fixed binding table, page-aligned, overlap-free, in range), `Capabilities`, `MAX_BATCH_COMMANDS`, `Batch` bound check. Tests: the matrix's six.
- **P2 — the one timeline.** `nexus_gfx::sync::timeline::{TimelinePoint, Timeline}` replaces the placeholder: monotonic signal, `reached`, the host model of `fence_signal`/`fence_wait`. Test: `test_reject_signal_backwards_is_a_no_op`.
- **P3 — GfxDevice v2 + the plane ring.** `nexus_gfx::backend::v2`: the `GfxDevice` trait (§1–§7), `scanout::{PlaneRing, Slot, SlotState, Flip, FrameClock, PointerPlane}` (§8). Tests: the ring's four rejects, the flip at the vertical blank, buffer age.
- **P4 — the mock device.** `CpuMockDevice` implements `GfxDevice`: contexts per client, buffers as bytes with their cache mode, binding through `VmSpace`, a batch of `Command`s executed into the bound target (fill only — data flow, not pixels), `Shader` refused for another target, points signalled on the timeline; the plane ring is tested beside it. Tests: the matrix's six.
- **P5 — closure.** `just check`, `just test-host`, `just diag` green; the indexes (RFC README, STATUS-BOARD "RFC Done" waits for RFC-0101's own completion — the RFC is In Progress with Phase 0 ✅; this ledger Done); CHANGELOG; the order file's layer 1 ✅.

## Closure (2026-10-12)

- **Delivered**: `source/libs/nexus-driverkit/src/gpu.rs` (P1), `userspace/nexus-gfx/src/sync/timeline.rs` (P2, the placeholder replaced), `userspace/nexus-gfx/src/backend/v2/{mod,scanout,cpu_mock}.rs` (P3, P4); `nexus-gfx` depends on `nexus-driverkit` for the vocabulary; RFC-0101 seeded with Phase 0 ✅ and in the RFC index.
- **Proof**: `cargo test -p nexus-driverkit -p nexus-gfx` — 38 + 61 tests, the 18 `test_reject_*` of RFC-0101 §9 by name; windowd's and gpud's OS builds warning-free with the new modules; the structure gate green; `just test-all` (see the commit).
- **One rule found while testing**: the ring hands out the free plane most recently on screen (fewest frames of damage to repaint); a never-scanned plane comes last. Written into `PlaneRing::acquire` and its test.
- **Moved with owners**: the `CpuSet` RFC seed → TASK-0330 S1 P0; the RFC-0100 seed → TASK-0286B (M2) P0; the wire form of flip and completion → TASK-0216 (Phase 1); the v1 trait's and the boolean fence's deletion → RFC-0101 Phase 3.

## Proof

- Host: `cargo test -p nexus-driverkit gpu`, `cargo test -p nexus-gfx v2`, `cargo test -p nexus-gfx timeline` — the 18 names of RFC-0101 §9 present and green; `just test-host`.
- OS: none in this layer (no OS code). `just diag` proves the new modules compile warning-free under the OS cfg (nexus-gfx is in gpud's and windowd's OS builds).
- Board: none — the first board proof of this contract is Phase 1's `front=0` (TASK-0216).

## Constraints / invariants (hard requirements)

- Bounded everything: binding tables, batches, damage rects, the pointer rect — refused above the bound, never truncated silently.
- No `unwrap`/`expect` on input; no `#[allow(dead_code)]` — a type with no OS user yet is still used by the mock and its tests.
- One vocabulary: `Command` (existing) for streams, `TimelinePoint` for sync, `CacheMode`/`DmaBuffer` for memory. A second of any is a defect.
- Generic names in code and docs (no vendor or product names; a `compatible` string is data).

## Red flags / decision points

- **RED**: `nexus-gfx` gains a dependency on `nexus-driverkit` (the vocabulary's home). Accepted by the architect lens: the alternative is a second vocabulary. It must stay buildable on the host and under the OS cfg (`just diag`).
- **YELLOW**: the mock executes `Command::DrawTiles` only; everything else in a batch is accepted and counted, not drawn. Pixels through v2 are Phase 3's.
- **GREEN**: the v1 trait, `core::fence::Fence` and the `sync`/`resource` placeholders stay until Phase 3 deletes them with the retired-name gate.

## Definition of Done

1. RFC-0101 seeded, Phase 0 ✅, in the RFC index.
2. P1–P4 in the tree, `just check` + `just test-host` + `just diag` green, the 18 reject tests by name.
3. The order file's layer 1 ✅; this ledger Done with the closure below; CHANGELOG.
