---
title: TASK-0129 Packages v1a: NXB v1 format alignment (manifest.nxb) + pkgr tool (build/sign/verify/inspect) + host tests
status: Draft
owner: @runtime
created: 2025-12-25
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Packaging baseline + manifest.nxb contract: docs/packaging/nxb.md
  - Packaging drift decision: tasks/TASK-0007-updates-packaging-v1_1-userspace-ab-skeleton.md
  - Supply chain signing policy direction: tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md
  - DevX nx CLI (tool aggregation): tasks/TASK-0045-devx-nx-cli-v1.md
---

## Context

Repo contract today:

- `.nxb` is a deterministic **directory**:
  - `manifest.nxb` (canonical, versioned binary manifest)
  - `payload.elf`

Some older plans/prompts still reference `manifest.json` and/or “zip bundles”; those are drift and must
not be reintroduced without an explicit task.

We want a single, developer-facing tool (`pkgr`) that creates and verifies bundles in the canonical
format without inventing new “truths”.

## Goal

Deliver:

1. NXB v1 authoring tool (`pkgr`) in `tools/pkgr/`:
   - `pkgr build`:
     - consumes an ELF entrypoint + metadata flags
     - emits a deterministic `.nxb/` directory with:
       - `manifest.nxb`
       - `payload.elf`
     - does **not** emit `manifest.json` as a contract (may provide `--print-json` view output)
   - `pkgr sign` / `pkgr verify`:
     - signs/verifies the canonical manifest bytes and payload digest according to the current signing policy direction
     - **must align with** `bundlemgrd` verification primitives (no parallel/contradicting signature semantics)
   - `pkgr inspect`:
     - prints manifest fields and computed digests deterministically
2. Determinism:
   - stable ordering in any printed tables
   - stable timestamps (if any are included in “views”, they must be explicit and not part of canonical bytes)
3. Markers (host tool output, not OS markers):
   - `pkgr: built <name>@<ver>`
   - `pkgr: signed key=<fingerprint>`
   - `pkgr: verified ok`
4. Host tests:
   - build/sign/verify a tiny test bundle deterministically
   - verify “views” (`--print-json`) are derived from `manifest.nxb` and stable across runs

## Non-Goals

- Kernel changes.
- Introducing a zip-based `.nxb` contract.
- Full asset packaging (`assets/**`, `lib/**`) unless the repo explicitly revives multi-file bundles (separate task).
- Alternative bundle format names (use NXB only; see `TASK-0238`).

## Constraints / invariants (hard requirements)

- **Single source of truth**: `manifest.nxb` is canonical; JSON/TOML are derived views only.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Avoid duplicating trust/policy semantics that belong to `bundlemgrd`/`keystored`/`policyd`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/pkg_v1_host/` (or `tests/pkgr_host/`):

- `pkgr build` produces deterministic bytes for `manifest.nxb` and `payload.elf` copy
- `pkgr sign/verify` roundtrips deterministically (using a fixed test key)
- `pkgr inspect` output is stable (golden)

## Touched paths (allowlist)

- `tools/pkgr/` (new)
- `tools/nxb-pack/` (may be reused or wrapped; do not fork contracts)
- `docs/packaging/nxb.md` (only if clarifications are required)
- `tests/` (new host tests)

## Plan (small PRs)

1. Implement `pkgr build/inspect` as a thin wrapper over the canonical manifest writer
2. Add signing/verification aligned with bundlemgrd policy direction
3. Add deterministic host tests
