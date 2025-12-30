---
title: TASK-0161 Backup/Restore v1a (host-first): NBK v1 deterministic bundle format + pack/verify/restore engine + retention/quota + tests
status: Draft
owner: @runtime
created: 2025-12-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Identity/Keystore v1.1 (seal/unseal API): tasks/TASK-0159-identity-keystore-v1_1-host-keystored-lifecycle-nonexportable.md
  - Search v2 backend (optional index backup): tasks/TASK-0153-search-v2-backend-host-index-ranking-analyzers-sources.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We want a strictly offline, deterministic backup bundle that can be:

- created deterministically (byte-for-byte stable given the same inputs),
- verified (checksums),
- restored (with a dry-run plan),
- managed under retention/quota rules.

This task defines and proves the **NBK v1** format and implements host-first pack/verify/restore logic.
OS/QEMU service wiring, Settings UI, CLI, and selftests are in `TASK-0162`.

## Goal

Deliver:

1. NBK v1 format specification:
   - deterministic ZIP layout (sorted entries, normalized timestamps, normalized permissions)
   - `manifest.json` + `checksums.sha256` rules
   - stable IDs (`<created_ns>-<short_hash>`) derived deterministically (seeded)
2. Deterministic pack/verify/restore engine (host-first library):
   - build deterministic entry ordering
   - compute and verify `checksums.sha256` for every entry
   - restore supports:
     - dry-run: enumerate changes deterministically
     - apply: restore files with stable permissions and paths
3. Retention/quota policy (host-first):
   - keep last N bundles and enforce soft/hard byte quotas deterministically
   - deterministic eviction ordering (oldest first; stable ties)
4. Sensitive data wrapping contract:
   - define how “secure blobs” are wrapped (device-bound via `keystored.seal/unseal`)
   - this task proves the contract with a **mock keystore** and deterministic fixtures
5. Deterministic host tests:
   - bundle bytes hash stable across runs
   - verify detects tampering
   - restore reproduces tree
   - retention evicts deterministically

## Non-Goals

- Kernel changes.
- OS/QEMU service wiring, Settings UI, policyd caps, selftests (see `TASK-0162`).
- Passphrase export mode (future v1.1).

## Constraints / invariants (hard requirements)

- Determinism:
  - normalized ZIP timestamps and entry ordering
  - JSON output uses canonical ordering (no host-map iteration drift)
  - no wallclock dependence in proofs (inject clock)
- Bounded memory:
  - bounded file sizes for v1 (documented caps) or chunked streaming
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake security: device-bound wrapping is an interface; “secure on OS” depends on OS keystore entropy story.

## Red flags / decision points (track explicitly)

- **RED (ZIP determinism / no_std feasibility)**:
  - ZIP tooling is typically `std`-heavy. This task is host-first by design.
  - OS enablement must be gated on either:
    - a no_std-capable ZIP reader/writer, or
    - keeping NBK creation on host and only verifying/restoring in OS via a minimal parser.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - Command(s):
    - `cargo test -p backup_restore_v1_host -- --nocapture`
  - Required tests:
    - deterministic create (bundle hash stable)
    - verify detects tamper
    - restore reproduces expected files
    - retention/quota eviction determinism

## Touched paths (allowlist)

- `userspace/libs/nbk/` (new; NBK format + pack/verify/restore helpers; host-first)
- `tests/backup_restore_v1_host/` (new)
- `docs/backup/overview.md` (added in `TASK-0162`)

## Plan (small PRs)

1. Define NBK v1 spec + canonical JSON + checksums rules
2. Implement pack/verify/restore + retention/quota logic
3. Add deterministic host tests with fixture trees

## Acceptance criteria (behavioral)

- Host tests deterministically prove NBK v1 create/verify/restore/retention behavior.

