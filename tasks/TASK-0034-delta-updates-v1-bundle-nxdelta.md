---
title: TASK-0034 Delta updates v1 (bundles): nxdelta (rollsum+zstd) + resume + verify-before-commit (host-first, OS-gated)
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Packaging baseline: tasks/TASK-0007-updates-packaging-v1_1-userspace-ab-skeleton.md
  - Supply-chain baseline (SBOM/sign policy): tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md
  - Persistence substrate (resume checkpoints): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - VMO plumbing (optional fast path): tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We want bandwidth-efficient bundle updates via binary deltas:

- produce and apply delta patches deterministically,
- support resume/checkpoint after interruption,
- verify integrity + signature policy **before** committing an installed bundle.

Repo reality today:

- There is no `updated` service and no `.nxs` tooling in-tree (system-set delta orchestration is a follow-up).
- Bundle install/verify exists via `bundlemgrd`, and supply-chain policy work is tracked separately.
- True cross-process “zero-copy VMO” sharing is gated (VMO transfer semantics must be proven).

This task is therefore **bundle-only**, **host-first**, and **OS-gated**.

## Goal

Deliver a deterministic delta format and tooling (`.nxdelta`) plus a bundle-level apply flow that:

- reconstructs the target bytes exactly,
- verifies strong hashes and supply-chain checks before commit,
- can resume after interruption using a checkpoint file/state.

## Non-Goals

- System-set (`.nxs`) delta container and orchestration (separate task).
- Kernel changes.
- Claiming “zero-copy” unless VMO sharing is proven end-to-end.

## Constraints / invariants (hard requirements)

- Kernel untouched.
- Deterministic format and output (stable ordering, stable chunk sizes).
- Bounded memory:
  - capped rolling-window index
  - capped in-flight output buffers
  - bounded record sizes.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success markers (OS markers only after real apply+verify+commit behavior exists).

## Red flags / decision points

- **RED (system delta gating)**:
  - `.nxs`/`updated` do not exist yet. Do not implement or promise system delta in this task.
- **YELLOW (manifest drift)**:
  - Docs say `.nxb` uses `manifest.nxb`, but tooling currently still writes `manifest.json` in some paths.
  - Delta logic must operate on payload bytes and/or a canonical digest, not bake in `manifest.json` as a long-term contract.
- **YELLOW (VMO fast path feasibility)**:
  - VMO-based apply can be added as an optional optimization only after VMO sharing/transfer is proven (TASK-0031).

## Contract sources (single source of truth)

- Supply-chain policy: TASK-0029
- Persistence: TASK-0009
- QEMU marker contract: `scripts/qemu-test.sh` (gated)

## Stop conditions (Definition of Done)

### Proof (Host) — required

New deterministic host tests (`tests/nxdelta_host/`):

- make/apply: base+target → patch → applied output is byte-identical to target
- corruption: tamper ADD block → apply fails deterministically (integrity error)
- resume: interrupt apply mid-stream, persist checkpoint, restart apply → completes and verifies
- determinism: running `make` twice produces identical patch bytes for identical inputs.

### Proof (OS / QEMU) — gated

Once bundle install/update paths exist in OS builds with statefs:

- `bundlemgrd: delta apply start (bundle=<...>)`
- `bundlemgrd: delta verify ok`
- `bundlemgrd: delta commit ok`
- `SELFTEST: delta bundle apply ok`
- `SELFTEST: delta bundle resume ok`
- `SELFTEST: delta integrity deny ok`

Notes:

- Any postflight must delegate to canonical harness/tests; no independent “log greps = success”.

## Touched paths (allowlist)

- `tools/nxdelta/` (new: format + make/apply)
- `tests/` (new: host tests)
- `source/services/bundlemgrd/` (apply+verify+commit; OS-gated)
- `source/apps/selftest-client/` (OS-gated markers)
- `docs/updates/delta.md`
- `docs/testing/index.md`
- `scripts/qemu-test.sh` (gated)

## Plan (small PRs)

1. **Define `.nxdelta` format + library**
   - LE header includes:
     - kind=NXB (bundle)
     - base sha256, target sha256
     - chunk size, algorithm string
   - Records:
     - `COPY { off, len }`
     - `ADD { zstd-compressed bytes }`
   - Trailer includes `records_sha256` for integrity of the patch stream itself.

2. **Host CLI**
   - `nxdelta make --base --target -o patch.nxdelta`
   - `nxdelta apply --base --patch -o out`
   - Deterministic emission: stable scanning order, stable zstd parameters.

3. **Resume / checkpoint**
   - Define a checkpoint file format (JSON/CBOR) containing:
     - patch digest, base digest, target digest
     - last record index applied
     - output digest-so-far (or rolling verification state)
   - Host tests prove resume semantics.
   - OS: checkpoint stored under `/state/update/delta/<bundle>.ckpt` (gated on statefs).

4. **bundlemgrd integration (OS-gated)**
   - Apply patch to a staging area (file or VMO) using streaming reads.
   - Verify:
     - target sha256 matches
     - manifest/SBOM digest checks per TASK-0029
     - signature policy (publisher/key allowlist) per TASK-0029
   - Commit atomically (swap staged bundle and update bundle index).

5. **Docs**
   - `docs/updates/delta.md` describing `.nxdelta`, resume, and verification-before-commit.

## Follow-ups (separate tasks)

- System-set delta container + updated orchestration (see TASK-0035).
- VMO fast path for apply once VMO sharing/transfer is proven.
