---
title: TASK-0197 Supply-Chain hardening v2a (host-first): sigchain envelope + translog Merkle log + SBOM (SPDX subset) + provenance records + deterministic tests/tools
status: Draft
owner: @security
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Supply-chain v1 baseline (CycloneDX + allowlist policy): tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md
  - Trust store unification (roots/keys): tasks/TASK-0160-identity-keystore-v1_1-os-attestd-trust-unification-selftests.md
  - Updates v2 orchestration (consumer): tasks/TASK-0179-updated-v2-offline-feed-delta-health-rollback.md
  - Store v1 (consumer): tasks/TASK-0180-store-v1a-host-storefeedd-storemgrd-ratings.md
  - Packages install authority (consumer): tasks/TASK-0130-packages-v1b-bundlemgrd-install-upgrade-uninstall-trust.md
---

## Context

Supply-chain v1 (`TASK-0029`) establishes:

- SBOM generation (CycloneDX) and signature allowlist enforcement,
- deterministic host tests,
- OS gating.

This v2 hardening adds:

- a signature-chain “envelope” that binds artifact hash + SBOM + provenance,
- a local, append-only transparency log (Merkle tree) for inclusion checks,
- SBOM validation (SPDX JSON subset; host-first),
- provenance attestations/records,
- and anti-downgrade/anti-rollback policy hooks for consumers (wired in v2b).

This task is host-first: it defines formats, builds deterministic fixtures, and proves verification logic.

## Goal

Deliver:

1. `schemas/supply.schema.json` (config knobs and gates):
   - `require_translog_inclusion`, `threshold`, `anti_downgrade`, retention windows, sbom required, etc.
2. SigChain envelope v1 (deterministic JSON):
   - subject { uri, sha256 }
   - sbom { uri, sha256 } (SPDX JSON v2.3 subset; see below)
   - provenance { builder, vcs, repro }
   - signatures[] (Ed25519) referencing trust store key IDs
   - translog inclusion reference { log_id, leaf_hash, checkpoint }
   - deterministic encoding rules:
     - stable key ordering (canonical writer), no timestamps unless explicitly part of the signed payload
3. `translogd` (host-first service or library + small service wrapper):
   - local Merkle tree log (sha256) with deterministic checkpoint signatures
   - persists under a host tempdir in tests; OS persistence is v2b and `/state` gated
4. `userspace/libs/sigchain`:
   - verifies:
     - subject hash matches expected
     - signature threshold met under trust store keys
     - translog inclusion present and checkpoint signature verifies
     - SBOM file exists and hash matches
     - provenance fields are well-formed
   - returns structured report with stable failure reasons
5. `userspace/libs/sbom` (SPDX subset):
   - validate SPDX JSON v2.3 subset deterministically
   - compute stable summary (packages/licenses) and stable digest
   - host-only generator tool (`sbomgen`) for fixtures (small SBOMs)
   - NOTE: CycloneDX remains the v1 SBOM format; v2 supports SPDX validation for enforcement where requested.
6. Provenance records:
   - host-side record model and JSONL encoding (append-only)
   - OS service wiring in v2b
7. Deterministic host tests (`tests/supply_hardening_v2_host/`):
   - sigchain ok
   - translog inclusion required/denied
   - SBOM hash mismatch denied
   - key rotation allowlist behavior (if supported) is deterministic
   - anti-downgrade decision helpers (pure function) are deterministic

## Non-Goals

- Kernel changes.
- Network transparency log (this is a local/offline log).
- Replacing supply-chain v1: this is additive, with explicit format choices.

## Constraints / invariants (hard requirements)

- Determinism: all exported artifacts (envelopes, checkpoints, SBOM summaries) must be byte-stable for fixed inputs.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (format proliferation)**:
  - v1 uses CycloneDX, this prompt asks for SPDX. Supporting both can drift.
  - v2 must document:
    - which format is required for enforcement (if any),
    - and which digest fields are treated as canonical.

- **YELLOW (transparency log meaning offline)**:
  - A local translog provides append-only evidence inside the device/fixture world, not global transparency.
  - Keep claims modest and document threat model.

## Stop conditions (Definition of Done)

- `cargo test -p supply_hardening_v2_host -- --nocapture` passes deterministically.

## Touched paths (allowlist)

- `schemas/supply.schema.json` (new)
- `source/services/translogd/` (new; host-first impl acceptable)
- `userspace/libs/sigchain/` (new)
- `userspace/libs/sbom/` + `tools/sbomgen/` (new)
- `userspace/libs/provenance/` (new; JSONL encoding + query helpers)
- `tests/supply_hardening_v2_host/` (new)
- `docs/supply/` (docs may land in v2b to keep PRs small)

## Plan (small PRs)

1. schema + envelope types + deterministic JSON writer + tests
2. translog core + checkpoint signing + tests
3. sigchain verifier + tests
4. sbom SPDX subset validator + sbomgen tool + tests
5. provenance record model + tests

## Acceptance criteria (behavioral)

- Host tests deterministically prove signature-chain verification, translog inclusion checks, SBOM validation, and stable report outputs.

