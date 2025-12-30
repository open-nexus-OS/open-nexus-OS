---
title: TASK-0133 State quotas v1: per-subject accounting + deterministic enforcement (EDQUOTA/ENOSPC) + tests/markers
status: Draft
owner: @runtime
created: 2025-12-25
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Persistence substrate: tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Sandbox quotas (security v2): tasks/TASK-0043-security-v2-sandbox-quotas-egress-abi-audit.md
  - Storage error contract: tasks/TASK-0132-storage-errors-vfs-semantic-contract.md
---

## Context

Your StateFS v3 prompt asks for per-app quotas with soft/hard limits and predictable errors.
We already have quota work tracked as part of sandboxing/security (`TASK-0043`), but it is OS-gated and
namespaces are not shipped yet.

This task defines a **minimal, deterministic quota enforcement surface** for `/state` that can be
proved host-first and later wired into OS once `/state` exists.

## Goal

Deliver:

1. Quota model:
   - per-subject (appId or subject identity) soft/hard bytes
   - used bytes accounting must be deterministic and bounded
   - OS reserve rules may exist later; v1 documents whether reserve is applied
2. Enforcement:
   - writes that exceed hard limit are denied with a stable error:
     - prefer `EDQUOTA` (align with `TASK-0043`)
     - map to `ENOSPC` only if a caller surface cannot represent `EDQUOTA` (must be explicit)
   - soft limit triggers a warning marker but still allows writes
3. Markers:
   - `statefs: quota warn subject=<id> used=... soft=...`
   - `statefs: quota deny subject=<id> used=... hard=...`
4. Host tests proving:
   - deterministic used-bytes accounting
   - deny-on-exceed behavior and stable errors

## Non-Goals

- Kernel changes.
- Strong enforcement against subjects that can bypass the storage surface (this is userspace guardrail).
- Global GC/compaction (separate task).

## Constraints / invariants (hard requirements)

- Determinism and bounded counters/tables.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

New deterministic tests (suggested: `tests/state_quota_host/`):

- set quota; write until limit; deny with `EDQUOTA` (or mapped error) deterministically
- soft limit warning marker emitted once per subject per window deterministically

### Proof (OS/QEMU) — gated

Once `/state` and subject identity propagation exist:

- `SELFTEST: quota deny ok` (or equivalent marker)

## Touched paths (allowlist)

- `source/services/statefsd/` and/or `source/services/contentd/` (enforcement point must be chosen and documented)
- `tests/`
- `docs/storage/quotas.md` (new)

## Plan (small PRs)

1. Decide enforcement point (statefsd vs contentd state provider vs vfsd namespace) and document tradeoffs
2. Implement accounting + enforcement + markers
3. Host tests + docs

