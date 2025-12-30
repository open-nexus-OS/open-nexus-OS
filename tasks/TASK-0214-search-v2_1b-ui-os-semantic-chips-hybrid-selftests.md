---
title: TASK-0214 Search v2.1b (UI+OS/QEMU): palette semantic chips + query expansion toggle + hybrid explain view + schema + selftests/docs
status: Draft
owner: @ui
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Search v2 UI host overlay: tasks/TASK-0151-search-v2-ui-host-command-palette-model-a11y.md
  - Search v2 UI OS wiring: tasks/TASK-0152-search-v2-ui-os-deeplinks-selftests-postflight-docs.md
  - Search v2.1 host backend: tasks/TASK-0213-search-v2_1a-host-semvec-tags-expansion-hybrid.md
  - Search v2 backend OS wiring/persistence: tasks/TASK-0154-search-v2-backend-os-persistence-selftests-postflight-docs.md
  - Policy caps baseline: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Search v2.1a adds semantic-lite ranking primitives and explain output to the backend.
This task surfaces them in the Search palette UX and wires deterministic OS selftests/markers.

## Goal

Deliver:

1. Palette UX upgrades:
   - semantic chips derived from:
     - backend tag facets and/or per-hit tags
   - clicking a chip applies `tag:<name>` filter deterministically
   - “Explain scoring” dev toggle:
     - shows `bm25`, `cos`, `lambda`, and combined score (bounded precision) per result
   - query expansion toggle (if enabled by schema)
   - markers:
     - `palette: tag filter tag=<t>`
     - `palette: explain on`
2. CLI extensions (if `nx-search` exists already):
   - `sem reindex`, `sem query`, `tags`, `explain` commands
   - NOTE: QEMU selftests must not rely on running host tools inside QEMU
3. Schema:
   - `schemas/search_v2_1.schema.json` with:
     - lambda, K, embedding dims/ngrams, expansion enabled/max terms, tags enabled
   - migration guard:
     - if semvec dims/version mismatch, require reindex
4. OS/QEMU selftests (bounded):
   - reindex with sem enabled:
     - `SELFTEST: search sem reindex ok`
   - tag chips path:
     - query yields Settings/Appearance hit and `tag:settings` chip works
     - `SELFTEST: search sem tags ok`
   - hybrid/explain path:
     - query expansion changes ordering deterministically and explain shows bm25+cos
     - `SELFTEST: search sem hybrid ok`
5. Docs:
   - semantic-lite embedding model and determinism rules
   - tags and lexicon extension
   - explain output format and precision policy

## Non-Goals

- Kernel changes.
- Non-deterministic ML embeddings.
- Online index sources.

## Constraints / invariants (hard requirements)

- Determinism:
  - stable chip ordering
  - bounded numeric formatting for explain (explicit rounding rules)
- No fake success:
  - selftests must validate result ordering and tag filtering via service responses, not log greps
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p search_v2_1_host -- --nocapture` (from v2.1a)
  - `cargo test -p search_v2_ui_host -- --nocapture` updated if UI adds new behaviors

- **Proof (QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=195s ./scripts/qemu-test.sh`
  - Required markers:
    - `SELFTEST: search sem reindex ok`
    - `SELFTEST: search sem tags ok`
    - `SELFTEST: search sem hybrid ok`

## Touched paths (allowlist)

- `userspace/systemui/overlays/search_palette/` (extend)
- `source/services/searchd/` (wiring to expose tags/explain if not already)
- `tools/nx-search/` (extend)
- `source/apps/selftest-client/`
- `schemas/search_v2_1.schema.json` (new)
- `docs/search/` + `docs/tools/nx-search.md` + `docs/ui/testing.md`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. palette semantic chips + tag filter wiring + host tests
2. explain scoring view + rounding policy + host tests
3. schema + selftests + docs + marker contract update

## Acceptance criteria (behavioral)

- In QEMU, semantic-lite reindex/query/tag filtering/hybrid explain behaviors are proven deterministically via selftest markers.

