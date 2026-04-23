# Current Handoff: TASK-0032 (packagefs v2 pkgimg) — Prep and Contract Alignment

**Date**: 2026-04-23  
**Active task**: `tasks/TASK-0032-packagefs-v2-ro-image-index-fastpath.md` — `Draft`  
**Tier policy**: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate C: Storage, PackageFS & Content, `production-grade`)  
**Immediate follow-up route**: `tasks/TASK-0033-packagefs-v2b-vmo-splice-from-image.md`  
**Production dependency route**: `tasks/TASK-0286`, `tasks/TASK-0287`, `tasks/TASK-0290`

## Status snapshot

- Previous handoff has been archived at `.cursor/handoff/archive/TASK-0031-zero-copy-vmos-v1-plumbing.md`.
- `TASK-0032` now reflects real packagefs baseline:
  - host path uses in-memory registry (`std_server.rs`),
  - os-lite path fetches image bytes from `bundlemgrd` and decodes `bundleimg` (`os_lite.rs`).
- Header and body now carry explicit follow-ups (`TASK-0033`, `TASK-0286`, `TASK-0287`, `TASK-0290`).
- Security section is explicit (threat model, invariants, required reject tests).
- Red-flag drift on manifest format is resolved (`manifest.nxb` baseline aligned in docs/tooling).

## Active execution order

1. Keep `TASK-0032` as execution SSOT for deterministic RO image + bounded index mount/read semantics.
2. Keep Gate-C production-grade mapping explicit in task text (no hidden scope shifts).
3. Preserve follow-up boundary: `TASK-0033` owns VMO splice; kernel closure truths remain in `TASK-0286/0287/0290`.
4. Require behavior-first proofs with deterministic reject-path coverage.

## Guardrails

- No fake success markers (`packagefsd: v2 mounted` only after real image validation + index load).
- No unbounded parsing/loops; explicit caps for image/index/entry/path ranges.
- No payload-derived identity/policy trust; keep channel-authoritative decisions.
- Keep kernel untouched in this task.
