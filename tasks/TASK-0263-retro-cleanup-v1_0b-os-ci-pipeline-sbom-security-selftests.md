---
title: TASK-0263 Bring-up Retrospective & Cleanup v1.0b (OS/CI): CI pipeline + SBOM/audit + security baseline + docs pass + versioning + selftests
status: Draft
owner: @devx
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Retro core (host-first): tasks/TASK-0262-retro-cleanup-v1_0a-host-hygiene-lints-repro-deterministic.md
  - SBOM baseline: tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md
  - Security baseline: tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need OS/CI integration for Bring-up Retrospective & Cleanup v1.0:

- CI pipeline (GitHub Actions or equivalent),
- SBOM & supply-chain basics,
- security baseline sanity,
- docs pass,
- versioning & changelog.

The prompt proposes CI pipeline and SBOM/audit. `TASK-0029` already plans SBOM for bundles. `TASK-0262` already plans repo hygiene and repro checks. This task delivers the **OS/CI integration** with CI pipeline, SBOM/audit, security baseline, docs pass, and versioning, complementing the host-first hygiene work.

## Goal

On OS/CI:

1. **CI pipeline** (GitHub Actions or equivalent):
   - **lint**: clippy + rustfmt check
   - **unit**: `cargo test --workspace --all-features` (deterministic flags)
   - **host suites**: run all `*_host` packages
   - **os smoke**: `just test-os` with `RUN_UNTIL_MARKER=1` and strict budgets
   - **repro**: run `tools/repro-check.sh`
   - **sbom/audit**: run `cargo auditable` (or `cargo audit` if preferred) and generate SBOM (see below)
   - cache wisely; upload `uart.log` and key artifacts on failure
2. **SBOM & supply-chain basics**:
   - add `tools/sbom/generate.rs` (or script) to emit a **CycloneDX JSON** SBOM: collect crates (name, version, license, checksum); include top-level licenses and our `SPDX` policy
   - add `tools/license-scan.sh`: verify no forbidden licenses (AGPL, custom unknown) in transitive deps
   - CI job: generate SBOM → store at `out/sbom.cdx.json` and attach to artifacts
3. **Security baseline sanity**:
   - re-run **policy compiler**; assert syscall masks present for all profiles; `nx-sec ps` shows non-empty masks
   - add **integration test** that starts a minimal app without a profile and ensures **deny-by-default**
   - ensure `capnp` boundaries fuzzer still green (from earlier fuzz gates)
4. **Docs pass**:
   - create/refresh: `docs/ROADMAP_BRINGUP.md` — the 10-Punkte-Liste mit Status (done/open), `docs/CONTRIBUTING.md` — style, commit tags, testing locally (host vs QEMU), how to keep determinism, `docs/REPRODUCIBLE_BUILDS.md` — environment pinning, `SOURCE_DATE_EPOCH`, how we normalize mtimes/owner
   - update main `README.md` with a concise **"Getting Started in 5 minutes"** and links to nx-CLIs
   - ensure **docs build** (link checker) runs in CI
5. **Version & changelog**:
   - tag internal milestone **`0.4.0-bringup`**: add `CHANGELOG.md` entries (Added/Changed/Fixed/Removed); bump `workspace` version field where applicable
6. **Postflight (umbrella)**:
   - add `tools/postflight-retro-cleanup-v1_0.sh`: lint, unit & host suites, reproducibility, qemu smoke (bounded), budgets
   - create `tools/check-budgets.sh` to read `schemas/budgets_v1.json` and verify sizes/logs; return non-zero on violation
7. **OS selftests + postflight**.

## Non-Goals

- Kernel changes.
- Full reproducible builds for all targets immediately (we start with deterministic hashing + metadata capture + a best-effort gate).
- Full supply-chain hardening (handled by `TASK-0197`; this task focuses on basics).

## Constraints / invariants (hard requirements)

- **No duplicate CI authority**: This task provides CI pipeline. `TASK-0165` already plans SDK CI gates. Both should share the same CI infrastructure to avoid drift.
- **No duplicate SBOM authority**: This task provides OS-level SBOM generation. `TASK-0029` already plans bundle SBOM. Document the relationship explicitly: OS-level SBOM vs bundle SBOM.
- **Determinism**: CI pipeline, SBOM generation, and security baseline must be stable given the same inputs.
- **Bounded resources**: CI runs are bounded; SBOM generation is bounded.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (CI authority drift)**:
  - Do not create parallel CI pipelines. This task provides CI pipeline. `TASK-0165` (SDK CI gates) should share the same CI infrastructure to avoid drift.
- **RED (SBOM authority drift)**:
  - Do not create parallel SBOM formats. This task provides OS-level SBOM (CycloneDX JSON). `TASK-0029` provides bundle SBOM (CycloneDX JSON). Document the relationship explicitly: OS-level SBOM vs bundle SBOM.
- **YELLOW (repro determinism)**:
  - CI repro checks must use `SOURCE_DATE_EPOCH` and normalize mtimes/owner. Document the environment pinning explicitly.

## Contract sources (single source of truth)

- QEMU marker contract: `scripts/qemu-test.sh`
- Retro core: `TASK-0262`
- SBOM baseline: `TASK-0029` (bundle SBOM)
- Security baseline: `TASK-0047` (policy as code)

## Stop conditions (Definition of Done)

### Proof (OS/CI) — gated

CI green across **lint/unit/host/os/repro/sbom**:

- lint: clippy + rustfmt check passes
- unit: `cargo test --workspace --all-features` passes
- host suites: all `*_host` packages pass
- os smoke: `just test-os` with `RUN_UNTIL_MARKER=1` and strict budgets passes
- repro: `tools/repro-check.sh` passes (two consecutive builds produce identical hashes)
- sbom/audit: SBOM generated and license scan passes

Security baseline sanity:

- policy compiler: syscall masks present for all profiles; `nx-sec ps` shows non-empty masks
- deny-by-default integration test: starts a minimal app without a profile and ensures deny-by-default
- `capnp` boundaries fuzzer still green

## Touched paths (allowlist)

- `.github/workflows/ci.yml` (extend: lint/unit/host/os/repro/sbom jobs)
- `tools/sbom/generate.rs` (new)
- `tools/license-scan.sh` (new)
- `tools/postflight-retro-cleanup-v1_0.sh` (new)
- `tools/check-budgets.sh` (new)
- `docs/ROADMAP_BRINGUP.md` (new)
- `docs/CONTRIBUTING.md` (new or extend)
- `docs/REPRODUCIBLE_BUILDS.md` (new)
- `README.md` (extend: "Getting Started in 5 minutes")
- `CHANGELOG.md` (new or extend)
- `source/apps/selftest-client/` (extend: deny-by-default integration test)
- `docs/` (link checker in CI)

## Plan (small PRs)

1. **CI pipeline**
   - lint/unit/host/os/repro/sbom jobs
   - cache configuration
   - artifact upload on failure

2. **SBOM & supply-chain basics**
   - sbom/generate.rs
   - license-scan.sh
   - CI job integration

3. **Security baseline sanity**
   - policy compiler refresh
   - deny-by-default integration test
   - fuzzer check

4. **Docs pass**
   - ROADMAP_BRINGUP.md
   - CONTRIBUTING.md
   - REPRODUCIBLE_BUILDS.md
   - README.md refresh
   - link checker in CI

5. **Version & changelog**
   - 0.4.0-bringup tag
   - CHANGELOG.md entries
   - workspace version bump

6. **Postflight**
   - postflight-retro-cleanup-v1_0.sh
   - check-budgets.sh

## Acceptance criteria (behavioral)

- CI green across lint/unit/host/os/repro/sbom.
- Two consecutive builds produce identical hashes for all key artifacts.
- SBOM generated and license scan passes.
- Security deny-by-default test passes.
- Docs updated and link-checked.
- Budgets enforced (image size/logs).
