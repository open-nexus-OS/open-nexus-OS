---
title: TASK-0203 IME v2.1a (host-first): deterministic adaptive ranking (Q8.8 freq/recency/bigram) + training + export/import + quota eviction
status: Draft
owner: @ui
created: 2025-12-27
updated: 2026-07-21 (rewritten against repo reality; store trait targets statefsd in TASK-0204, not securefsd)
depends-on:
  - TASK-0149
follow-up-tasks:
  - TASK-0204 (OS persistence on statefsd + Settings UI)
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Contract: docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md
  - CJK engines + user-dict API: tasks/TASK-0149-ime-v2-part2-cjk-engines-userdict.md
  - OS persistence (follow-up): tasks/TASK-0204-ime-v2_1b-os-statefs-personal-dict-ui-cli-selftests.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

TASK-0149 ships engines with table-order candidates and an in-memory user-dict
API. This task adds the deterministic personalization layer on host: adaptive
ranking that learns from commits without ever becoming nondeterministic or
unbounded. Storage stays behind a trait — TASK-0204 binds it to statefsd
(`state:/ime/…`); securefsd does not exist (TASK-0183 Superseded), so the old
SecureFS gating is dropped. Encryption-at-rest is TASK-0300 (seed).

## Goal

1. `userspace/ime-ranker` (new, no_std-capable, zero deps): fixed-point Q8.8
   scoring — personal frequency, context bigram `(prev, cand)`, length prior,
   recency bucket (coarse buckets, never raw timestamps); **stable
   tie-breakers** (score, then base table order, then codepoint).
2. Training on commit: frequency increment + bigram upsert +
   `last_seen_bucket` update; bounded counters (saturating).
3. `PersonalStore` trait: `upsert_dict/upsert_bigram/forget/top_stats/
   export_ndjson/import_ndjson` — storage-agnostic, deterministic iteration order.
4. NDJSON export/import (versioned header line; import validates bounds,
   rejects oversized/malformed lines fail-closed).
5. Per-locale quota eviction: deterministic order (lowest score, oldest
   bucket, stable tiebreak), caps configurable, defaults ≤ 4096 entries/lang.

## Non-Goals

- No OS wiring, no persistence backend, no UI, no markers (TASK-0204).
- No raw timestamps, no RNG, no floating point (Q8.8 only — reproducible).
- No cross-user concepts (single-user session today).

## Constraints / invariants (hard requirements)

- Deterministic: identical training sequence → identical ranking + identical
  export bytes (golden test).
- Bounded: all counters saturate; import enforces quotas; line length ≤ 256 B.
- No `unwrap`/`expect` on import input; fail-closed line-by-line with a
  bounded error count.

## Security considerations

### Threat model
- Import of a hostile NDJSON profile (oversized, malformed, quota-busting).
- Learning as a side channel (password/secret content entering the store).

### Security invariants (MUST hold)
- Import is fail-closed and bounded before parsing each line.
- The training API takes only committed candidate IDs + language — never raw
  field text; password-field commits must not reach `train` (caller-gated in
  imed, re-proven in TASK-0204's OS tests).
- Export contains only dict entries/bigrams the user trained — no session data.

### Security proof
- `test_reject_import_oversize_line`, `test_reject_import_bad_version`,
  `test_import_quota_enforced`, determinism goldens.

## Contract sources (single source of truth)

- **Ranking/store semantics**: RFC-0075 personalization section (extended by
  this task) + `userspace/ime-ranker/tests/` goldens (export bytes = contract).

## Stop conditions (Definition of Done)

- **Proof (host)**: `cargo test -p ime-ranker` — ranking goldens (trained
  candidate overtakes table order after N commits; bigram boost only in
  context), eviction determinism, export/import round-trip byte-identical,
  reject tests green.
- **Gates**: `just check` + `just test-host` green.

## Touched paths (allowlist)

- `userspace/ime-ranker/` (new: src/ + tests/)
- `Cargo.toml` (workspace member), `docs/dev/ui/input/ime.md` (personalization
  section), `CHANGELOG.md`

## Plan (small PRs)

1. Q8.8 scorer + training + goldens.
2. PersonalStore trait + in-memory impl + eviction + goldens.
3. NDJSON export/import + reject matrix + docs.

## Acceptance criteria (behavioral)

- After a deterministic training script, ranking output changes exactly as the
  goldens specify and exports byte-identically across runs.
- A hostile import cannot exceed quotas, panic, or corrupt existing state.
