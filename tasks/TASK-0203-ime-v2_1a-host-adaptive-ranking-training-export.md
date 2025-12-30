---
title: TASK-0203 IME v2.1a (host-first): adaptive deterministic ranking + training model (freq/recency/bigrams) + user dict core + export/import + quota eviction + tests
status: Draft
owner: @ui
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - IME v2 Part 1 core: tasks/TASK-0146-ime-text-v2-part1a-imed-keymaps-host.md
  - IME v2 Part 2 engines + dict learning baseline: tasks/TASK-0149-ime-v2-part2-cjk-engines-userdict.md
  - Policy model (scoped grants/expiry) direction: tasks/TASK-0167-policy-v1_1-host-scoped-grants-expiry-enumeration.md
  - SecureFS storage wiring (OS-gated): tasks/TASK-0183-encryption-at-rest-v1b-os-securefsd-unlock-ui-migration-cli-selftests.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

IME v2 already plans deterministic engines and a bounded learning dictionary (`TASK-0149`).
IME v2.1 adds a more explicit, deterministic adaptive ranking pipeline and user-personalization features:

- personal frequency and recency,
- context bigram boosting,
- “forget suggestion” semantics,
- deterministic export/import,
- deterministic quota enforcement/eviction.

This task is host-first and defines ranking + storage semantics independently of SecureFS.
OS persistence under `state:/secure/...` is implemented in v2.1b.

## Goal

Deliver:

1. `userspace/libs/ime_ranker`:
   - deterministic scoring with fixed-point math (Q8.8 weights)
   - stable tie-breakers:
     - candidate text lexicographic, then engine order, then stable candidate id
   - features:
     - personal frequency (`pf`)
     - context bigram boost (`pc`)
     - length term (`pl`) for tie breaking in applicable engines
     - locale prior (`ploc`) (engine-provided)
     - recency bucket (`pr`) computed from a clock-injected “now”
2. Training model (pure Rust; deterministic):
   - on commit:
     - bump user dict frequency
     - bump `(prev_commit, candidate)` bigram frequency
     - store `last_seen_bucket` (bucket index), not raw timestamps, to keep determinism
   - “forget suggestion”:
     - deterministic decrement or delete rule (documented)
3. Storage interface `ime_personal_store`:
   - trait-based store with:
     - `upsert_dict`, `upsert_bigram`, `forget`, `top_stats`, `export_ndjson`, `import_ndjson`
   - reference implementation for host tests uses a tempdir + JSONL or a compact binary format
   - NOTE: SQLite/libSQL may exist as an optional host-only backend, but must not be required for OS viability
4. NDJSON export/import format:
   - stable keys and stable ordering rules
   - merge rules and clamps to quotas are deterministic
5. Quotas + eviction:
   - per-locale caps (rows/bytes) with deterministic eviction:
     - least-recent bucket, then lowest freq, then stable key ordering
6. Deterministic host tests `tests/ime_v2_1_host/`:
   - ranking uses personalization to move candidate to top
   - bigram boosting increases rank after a prior commit
   - forget causes drop deterministically
   - export/import round-trip yields identical top-k
   - quota enforcement evicts deterministically

## Non-Goals

- SecureFS persistence and OS UI/CLI wiring (v2.1b).
- Claims of privacy/security beyond offline local storage (documented later).

## Constraints / invariants (hard requirements)

- Determinism:
  - fixed-point math only in scoring
  - injected clock for tests; production uses monotonic bucketization
  - stable ordering everywhere
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (libSQL in OS)**:
  - Do not require SQLite/libSQL for OS builds. Keep any SQL backend host-only behind a feature flag and prove pure-Rust backend.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p ime_v2_1_host -- --nocapture`

## Touched paths (allowlist)

- `userspace/libs/ime_ranker/` (new)
- `userspace/libs/ime_personal_store/` (new; naming to be aligned with existing ime-* crates)
- `tests/ime_v2_1_host/` (new)
- `docs/ime/personalization.md` (may land in v2.1b)

## Plan (small PRs)

1. ranker crate + golden scoring tests
2. store trait + pure Rust reference backend + tests
3. NDJSON export/import + tests
4. quota eviction + tests

## Acceptance criteria (behavioral)

- Host tests deterministically prove adaptive ranking, training updates, forget semantics, export/import, and quota eviction.

