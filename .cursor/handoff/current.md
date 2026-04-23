# Current Handoff: TASK-0031 (Zero-copy VMOs v1) — Seed Contract Alignment

**Date**: 2026-04-21  
**Active task**: `tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md` — `In Review`  
**Contract seed (RFC)**: `docs/rfcs/RFC-0040-zero-copy-vmos-v1-plumbing-host-first-os-gated.md` — `Done`  
**Production closure route**: `tasks/TASK-0290-kernel-zero-copy-closure-v1b-vmo-seals-reuse-truth.md`  
**Tier policy**: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`

## Status snapshot

- Seed RFC exists and is linked in task header.
- RFC scope is now strictly v1 plumbing; kernel production closure is explicitly delegated to `TASK-0290`.
- `.cursor` workfiles have been switched from old `TASK-0029` execution posture to `TASK-0031` prep posture.
- Current implementation status:
  - `userspace/memory` crate (`nexus-vmo`) with typed host-first API and deterministic counters.
  - Host reject proofs green: `test_reject_unauthorized_transfer`, `test_reject_oversized_mapping`, `test_ro_mapping_enforced`, short file-range reject, host slot-transfer reject.
  - OS compile seam green: `cargo +nightly-2025-01-15 check -p nexus-vmo --target riscv64imac-unknown-none-elf --no-default-features --features os-lite`.
  - OS marker ladder green in `just test-os`: `vmo: producer sent handle`, `vmo: consumer mapped ok`, `vmo: sha256 ok`, `SELFTEST: vmo share ok` (real producer/consumer task split with slot-directed transfer).

## Active execution order

1. Keep `TASK-0031` as execution SSOT (plumbing/honesty floor).
2. Keep `RFC-0040` as contract seed and update with each contract decision.
3. Preserve explicit split: `TASK-0031` floor vs `TASK-0290` production-grade closeout.
4. Keep behavior-first proofs and reject-path coverage as closure evidence.
5. Treat `TASK-0290` as the exclusive owner of kernel production-grade closure.

## Guardrails

- No fake success markers (`ok/ready` only after real behavior).
- No unbounded retry/drain loops in proof paths.
- Rust safety expectations are explicit: `newtype`, ownership/lifetime, `#[must_use]`, `Send`/`Sync` discipline.
- No production-grade claim on `TASK-0031`/`RFC-0040`; use `TASK-0290` closure artifacts.

## Carry-over

- `TASK-0029` + `RFC-0039` remain done and out of active execution scope.
- `TASK-0023B` external CI replay artifact remains an independent follow-up.
