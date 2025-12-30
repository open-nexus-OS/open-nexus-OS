---
title: TASK-0276 Parallelism v1: deterministic threadpools policy (safe-by-default, no-proof-drift)
status: Draft
owner: @arch
created: 2025-12-30
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Keystone gates: tasks/TRACK-KEYSTONE-GATES.md
  - Authority & naming registry: tasks/TRACK-AUTHORITY-NAMING.md
  - Kernel SMP plan (roadmap): tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md
---

## Context

Rust enables **memory-safe parallelism**, but parallelism is not “free” in an OS:

- naive thread pools can break determinism, increase tail latency, and create scheduler-dependent bugs,
- yet many workloads (rendering, decoding, parsing, indexing) benefit massively from structured parallel compute.

This policy defines a **best-for-OS** approach:

- determinism **where it matters** (tests/proofs/contracts),
- parallelism **where it pays** (hot compute), with deterministic reduction and bounded resources,
- no ideology: allow both single-threaded and parallel implementations, but lock the contracts so we don’t drift.

## Goal

Define a v1 “Deterministic Parallelism” contract for userland services and UI stacks:

1. When thread pools are allowed
2. The required invariants (fixed worker counts, partitioning, canonical reduction)
3. Required proofs (host-first + QEMU markers where applicable)
4. Anti-drift rules (no ad-hoc per-service pool semantics)

## Non-Goals

- Mandating parallelism everywhere.
- Mandating determinism everywhere (only where it is required for proof/contracts).
- Kernel scheduling policy design (SMP itself is tracked separately).

## Decision summary (v1 rules)

### Rule 1 — Determinism boundary

- **External contracts must be deterministic**:
  - IR formats, paging tokens, on-disk/wire formats, policy snapshot artifacts, and test outputs.
- **Internal parallelism is allowed** if it:
  - produces the same observable result for the same inputs,
  - uses canonical reduction rules,
  - stays within bounded resource caps.

### Rule 2 — Fixed pool shapes

If a component uses worker threads:

- worker count must be **explicit and fixed by configuration**:
  - host tests use a fixed `workers=1` and `workers=N` matrix (N small, e.g. 4),
  - QEMU uses a fixed, documented default.
- thread creation is bounded; no unbounded “spawn per task”.

### Rule 3 — Structured partitioning + canonical reduction

Parallelism must be structured as:

- **partition** work deterministically (tiles, chunks, spans, fixed ranges),
- compute in parallel,
- **reduce** results in a deterministic order (canonical merge order).

Examples:

- Rendering: fixed tile grid, reduce in row-major tile order.
- Parsing/sanitizing: partition input into deterministic segments, canonicalize output (stable ordering).
- Indexing: shard docs deterministically, merge postings in stable key order.

### Rule 4 — Bounded queues and backpressure

Every pool has:

- bounded job queue length,
- bounded per-job memory,
- explicit backpressure behavior (drop/coalesce/slow path) that is deterministic and testable.

### Rule 5 — Proof strategy (avoid scheduler-dependent tests)

- Host tests validate determinism by running the same workload under:
  - `workers=1` vs `workers=N` and asserting identical outputs,
  - randomized *scheduling* is not used as a proof; only output equality is used.
- QEMU proofs use markers that report **results**, not timing.

## Recommended usage (where parallelism pays)

### A) UI rendering/compositing (`windowd` + renderer backend)

Preferred model:

- tiling/occlusion gives deterministic partitioning,
- tile workers can rasterize tiles in parallel,
- merge order is canonical (tile order).

### B) Sanitizers/decoders (WebView, PNG/TTF, media)

- parallel decode/parse is allowed,
- output must be canonicalized (stable DOM ordering / stable frame ordering),
- bounded buffers and deterministic error handling.

### C) Search/indexing

- deterministic sharding of documents,
- stable merge order of ranked outputs (ties broken deterministically).

## Anti-drift rules

- Do not introduce a new “pool per subsystem” framework without documenting it here.
- Prefer a single shared `userspace/libs/workpool` contract if/when we standardize (optional future follow-up).
- If a task introduces parallel compute, it must link this policy and state:
  - partitioning strategy,
  - reduction order,
  - caps (workers, queue, memory),
  - proof plan (workers=1 vs N equivalence).
