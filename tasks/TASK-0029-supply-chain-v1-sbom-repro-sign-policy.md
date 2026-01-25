---
title: TASK-0029 Supply-Chain v1: bundle SBOM (CycloneDX) + repro metadata + signature allowlist policy (host-first, OS-gated)
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Packaging baseline: tasks/TASK-0007-updates-packaging-v1_1-userspace-ab-skeleton.md
  - Policy baseline: docs/security/signing-and-policy.md
  - Depends-on (device identity/keys): tasks/TASK-0008B-device-identity-keys-v1-virtio-rng-rngd-keystored-keygen.md
  - Depends-on (audit sink): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We want auditable supply-chain basics:

- SBOM per bundle,
- reproducibility evidence / reproducibility gates,
- install-time signature policy enforcement (publisher/key allowlist).

Repo reality today:

- `tools/nxb-pack` exists but currently writes `manifest.json` (known format drift vs `manifest.nxb` direction).
- `bundlemgrd` exists (host and OS-lite paths), and already uses `keystored` for signature verification.
- There is no `tools/nxs-pack` and no `updated` service in-tree yet, so “system set SBOM” and “updated enforcement” must be gated.

## Goal

Deliver supply-chain v1 for **bundles** first:

- Generate and embed a CycloneDX JSON SBOM inside each `.nxb`.
- Capture reproducibility metadata and provide a deterministic host-side verify step.
- Enforce a publisher/key allowlist policy in `bundlemgrd` install path.
- Provide host tests and (once OS bundle install path exists) OS selftest markers.

## Non-Goals

- System-set (`.nxs`) SBOMs and updated-stage enforcement (follow-up once tools/services exist).
- Full reproducible builds for *all* targets immediately (we start with deterministic hashing + metadata capture + a best-effort gate).
- Kernel changes.

## Constraints / invariants (hard requirements)

- Kernel untouched.
- No fake success: markers only after real verification steps.
- Deterministic parsing:
  - SBOM JSON must be canonicalised or hashed based on canonical fields (avoid “timestamp drift”).
  - Repro metadata must use `SOURCE_DATE_EPOCH` and explicitly record relevant inputs.
- No blanket `allow(dead_code)`; avoid `unwrap/expect`.

## Red flags / decision points

- **RED (manifest format drift)**:
  - Today `tools/nxb-pack` writes `manifest.json`, while docs/tasks are moving toward a canonical `manifest.nxb`.
  - Supply-chain v1 must not cement the wrong format. This task should **depend on** the packaging baseline task and
    either:
    - switch `nxb-pack` to the canonical manifest format first, or
    - explicitly document a transitional embedding strategy (SBOM location independent of manifest format).
- **YELLOW (reproducibility scope)**:
  - “Bit-for-bit reproducible” across environments is hard. v1 should gate on:
    - stable digest of payload bytes,
    - build metadata capture (rustc, flags, target, crate graph hash),
    - optional verification re-run in CI/host toolchain.
- **YELLOW (policy authority)**:
  - Publisher allowlists should be rooted in a single authority. Prefer `keystored` (anchors/keys) and `policyd` for decisions,
    with `bundlemgrd` enforcing at install time.

## Contract sources (single source of truth)

- Bundle install path: `source/services/bundlemgrd`
- Signature verification primitive: `keystored` capnp verify API
- Packaging direction: `docs/packaging/nxb.md` + TASK-0007
- QEMU marker contract: `scripts/qemu-test.sh`

## Stop conditions (Definition of Done)

### Proof (Host) — required

- Deterministic host tests that:
  - create a test bundle with embedded SBOM and repro metadata,
  - verify install succeeds with an allowed publisher/key,
  - reject install with unknown publisher/key (stable error),
  - reject install if payload digest mismatch (stable integrity error),
  - verify repro metadata schema is stable and `repro-verify` produces deterministic results.

### Proof (OS / QEMU) — gated

Once OS bundle install path + keystored are functional in QEMU:

- `bundlemgrd: sign policy ok (publisher=<...>)`
- `SELFTEST: sign policy allow ok`
- `SELFTEST: sign policy deny ok`

Notes:

- Postflight scripts must delegate to canonical tests/harness; no independent “log greps = success”.

## Touched paths (allowlist)

- `tools/nxb-pack/` (embed SBOM + repro metadata into bundle)
- `tools/sbom/` (new: generate CycloneDX JSON)
- `tools/repro/` (new: repro metadata capture + verify tool)
- `source/services/bundlemgrd/` (publisher/key allowlist enforcement)
- `source/services/keystored/` (expose allowlist check and/or key registry API)
- `recipes/signing/` (new allowlist TOML)
- `tests/` (host tests)
- `docs/supplychain/` (new docs)
- `docs/testing/index.md`
- `scripts/qemu-test.sh` (gated marker update)

## Plan (small PRs)

1. **SBOM generator (host tool)**
   - Add `tools/sbom` to emit CycloneDX JSON (v1.5) with:
     - bundle name/version/publisher
     - sha256 hashes (payload + important meta files)
     - dependency list (best effort: Cargo.lock slice or crate list if available)
     - build environment (rustc version, target triple, flags).
   - Embed into `.nxb` as `meta/sbom.json` (path stable regardless of manifest format).

2. **Repro metadata + verifier**
   - Add `tools/repro`:
     - write `meta/repro.env.json` (schema versioned, timestamps from `SOURCE_DATE_EPOCH`)
     - `repro-verify` checks:
       - payload digest matches manifest metadata
       - env schema present and valid.

3. **Publisher/key allowlist policy**
   - Add `recipes/signing/publishers.toml` with:
     - allowed publishers
     - allowed algorithms
     - allowed keys (fingerprints or raw pubkeys).
   - Implement `keystored::is_key_allowed(publisher, alg, pubkey)` (or equivalent RPC).
   - `bundlemgrd` install path enforces:
     - signature verifies
     - publisher is allowed
     - key is allowed
     - payload digest matches.
   - Emit deterministic allow/deny markers (UART) and audit events via logd (once TASK-0006 exists).

4. **Tests**
   - Host tests for allow/deny/tamper + sbom presence + repro verify.

5. **OS selftest + markers (gated)**
   - Add selftest install of an allowed test bundle and an unallowed test bundle.

## Docs (English)

- `docs/supplychain/sbom.md`: where SBOM lives in `.nxb`, how to inspect.
- `docs/supplychain/repro.md`: schema, `SOURCE_DATE_EPOCH`, verifier usage.
- `docs/supplychain/sign-policy.md`: publisher allowlist format, key rotation, failure modes.
- `docs/testing/index.md`: how to run host tests; expected OS markers once enabled.

## Follow-ups (separate tasks)

- System set (`.nxs`) SBOM aggregation and updated-stage enforcement once `nxs-pack`/`updated` exist.
- Stronger reproducibility gates in CI for selected artifacts.
- Supply-chain hardening v2 (sigchain envelope + local transparency log + SBOM validation/provenance + anti-downgrade enforcement) is tracked as `TASK-0197`/`TASK-0198`.
