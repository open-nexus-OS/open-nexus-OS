# Current Handoff: TASK-0031 (Zero-copy VMOs v1) — Seed Contract Alignment

**Date**: 2026-04-21  
**Active task**: `tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md` — `In Progress`  
**Contract seed (RFC)**: `docs/rfcs/RFC-0040-zero-copy-vmos-v1-plumbing-host-first-os-gated.md` — `In Progress`  
**Production closure route**: `tasks/TASK-0290-kernel-zero-copy-closure-v1b-vmo-seals-reuse-truth.md`  
**Tier policy**: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`

## Status snapshot

- Seed RFC exists and is linked in task header.
- RFC now states that production-grade closure is mandatory before `Complete`.
- `.cursor` workfiles have been switched from old `TASK-0029` execution posture to `TASK-0031` prep posture.

## Active execution order

1. Keep `TASK-0031` as execution SSOT (plumbing/honesty floor).
2. Keep `RFC-0040` as contract seed and update with each contract decision.
3. Preserve explicit split: `TASK-0031` floor vs `TASK-0290` production-grade closeout.
4. Enforce behavior-first proofs and reject-path coverage before any status promotion.
5. Keep production gate mapping explicit (Gate A + Gate C relevance).

## Guardrails

- No fake success markers (`ok/ready` only after real behavior).
- No unbounded retry/drain loops in proof paths.
- Rust safety expectations are explicit: `newtype`, ownership/lifetime, `#[must_use]`, `Send`/`Sync` discipline.
- No early production-grade claim before `TASK-0290` closure.

## Carry-over

- `TASK-0029` + `RFC-0039` remain done and out of active execution scope.
- `TASK-0023B` external CI replay artifact remains an independent follow-up.
