---
title: TASK-0262 Bring-up Retrospective & Cleanup v1.0a (host-first): repo hygiene + lints + repro checks + schema consolidation + deterministic tests
status: Draft
owner: @devx
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - SBOM baseline: tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md
  - SDK CI gates: tasks/TASK-0165-sdk-v1-part2a-devtools-lints-pack-sign-ci.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need to solidify the bring-up milestone with repo hygiene, determinism checks, and schema consolidation:

- repo hygiene (Clippy, rustfmt, SPDX headers),
- determinism & repro checks,
- schema & config consolidation,
- dead code & TODO sweep.

The prompt proposes repo hygiene and determinism checks. `TASK-0029` already plans SBOM and repro metadata for bundles. `TASK-0165` already plans SDK CI gates. This task delivers the **host-first core** (hygiene, lints, repro checks, schema consolidation) that can be reused by both CI and local development.

## Goal

Deliver on host:

1. **Repo hygiene & lints**:
   - enable **Clippy** (pedantic where reasonable) and **rustfmt** in workspace
   - add `clippy.toml` with selected denies: `dbg_macro`, `todo`, `unwrap_used` (except in tests), `panic`, `print_stdout` (use logging)
   - add `rust-toolchain.toml` pin if not present
   - remove blanket `#![allow(dead_code)]`; replace with **scoped** `#[allow(...)]` only where justified with comment
   - add **SPDX headers** to all Rust/TOML/sh files (`Apache-2.0`) via a small script; fail CI when missing
2. **Determinism & repro check**:
   - add `tools/repro-check.sh`: performs two clean builds back-to-back (`make clean && make image`) and compares SHA-256 of `build/nexus-os.img`, `build/rootfs.squashfs`, `build/pkgfs.img`, all `.lc` catalogs, compiled policies
   - fails if any differ; prints table of hashes
3. **Size & log budgets**:
   - introduce `schemas/budgets_v1.json`: `{ "uart_log_bytes_max": 52428800, "image_bytes_max": 536870912, "rootfs_bytes_max": 268435456 }`
   - enforce in postflight scripts: fail if `uart.log` exceeds budget after bounded runs; fail if image/rootfs exceed thresholds; print friendly diff (prev vs new)
4. **Schema & config consolidation**:
   - ensure all service schemas live under `schemas/` with **versioned filenames**; add `docs/schemas/INDEX.md` that links each schema and states owner/service
   - add a tiny **schema linter** (`tools/schema-lint.py`): keys are snake_case, arrays sorted where declared "sets", default values present, comments ban "TBD"
5. **Dead code & TODO sweep**:
   - search for `TODO`, `FIXME`, `XXX`: convert to **tracked issues** (ID in comment) or delete if obsolete
   - remove unused modules/binaries left from spikes (keep a changelog entry)
   - add `DEPRECATIONS.md`: list removed crates/paths and the replacement
6. **Host tests** proving:
   - repro check: two consecutive builds produce identical hashes for all key artifacts
   - schema linter: catches bad fixtures deterministically
   - budget checker: validates size/log budgets correctly

## Non-Goals

- OS/QEMU integration (deferred to v1.0b).
- Full CI pipeline (handled by `TASK-0263`; this task focuses on tools and checks).
- SBOM generation (handled by `TASK-0029`; this task focuses on repo hygiene).

## Constraints / invariants (hard requirements)

- **No duplicate lint authority**: This task provides lint configuration. `TASK-0165` already plans SDK CI gates. Both should share the same lint configuration to avoid drift.
- **No duplicate schema authority**: This task consolidates schemas. Existing service schemas should be migrated to `schemas/` with versioned filenames. Do not create parallel schema locations.
- **Determinism**: repro checks, schema linter, and budget checker must be stable given the same inputs.
- **Bounded resources**: repro checks are bounded; schema linter is bounded.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (lint authority drift)**:
  - Do not create parallel lint configurations. This task provides lint configuration. `TASK-0165` (SDK CI gates) should share the same lint configuration to avoid drift.
- **RED (schema authority drift)**:
  - Do not create parallel schema locations. This task consolidates schemas under `schemas/` with versioned filenames. Existing service schemas should be migrated.
- **YELLOW (repro determinism)**:
  - Repro checks must use `SOURCE_DATE_EPOCH` and normalize mtimes/owner. Document the environment pinning explicitly.

## Contract sources (single source of truth)

- Testing contract: `scripts/qemu-test.sh`
- SBOM baseline: `TASK-0029` (SBOM and repro metadata for bundles)
- SDK CI gates: `TASK-0165` (SDK CI gates)

## Stop conditions (Definition of Done)

### Proof (Host) — required

`cargo test -p retro_cleanup_v1_0_host` green (new):

- repro check: two consecutive builds produce identical hashes for all key artifacts
- schema linter: catches bad fixtures deterministically
- budget checker: validates size/log budgets correctly

## Touched paths (allowlist)

- `clippy.toml` (new)
- `rust-toolchain.toml` (new or extend)
- `tools/repro-check.sh` (new)
- `tools/schema-lint.py` (new)
- `tools/check-budgets.sh` (new)
- `tools/add-spdx-headers.sh` (new)
- `schemas/budgets_v1.json` (new)
- `schemas/` (consolidate existing schemas)
- `docs/schemas/INDEX.md` (new)
- `DEPRECATIONS.md` (new)
- `tests/retro_cleanup_v1_0_host/` (new)
- `docs/CONTRIBUTING.md` (new or extend)
- `docs/REPRODUCIBLE_BUILDS.md` (new)

## Plan (small PRs)

1. **Repo hygiene & lints**
   - clippy/rustfmt configs
   - SPDX headers script
   - scoped allows sweep
   - host tests

2. **Determinism & repro check**
   - repro-check.sh
   - host tests

3. **Size & log budgets**
   - budgets_v1.json
   - check-budgets.sh
   - host tests

4. **Schema consolidation**
   - schema migration
   - schema-lint.py
   - docs/schemas/INDEX.md
   - host tests

5. **Dead code & TODO sweep**
   - TODO/FIXME/XXX sweep
   - unused code removal
   - DEPRECATIONS.md

6. **Docs**
   - CONTRIBUTING.md
   - REPRODUCIBLE_BUILDS.md

## Acceptance criteria (behavioral)

- Two consecutive builds produce identical hashes for all key artifacts.
- Schema linter catches bad fixtures deterministically.
- Budget checker validates size/log budgets correctly.
- No blanket dead-code allows; SPDX headers present.
