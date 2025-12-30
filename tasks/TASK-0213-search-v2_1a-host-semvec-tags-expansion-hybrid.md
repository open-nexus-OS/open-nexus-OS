---
title: TASK-0213 Search v2.1a (host-first): semantic-lite vectors + tags extraction + query expansion + hybrid BM25+cos rerank + explain + deterministic tests
status: Draft
owner: @ui
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Search v2 backend baseline: tasks/TASK-0153-search-v2-backend-host-index-ranking-analyzers-sources.md
  - Search v2 OS persistence + selftests: tasks/TASK-0154-search-v2-backend-os-persistence-selftests-postflight-docs.md
  - Search v2 UI surface: tasks/TASK-0151-search-v2-ui-host-command-palette-model-a11y.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Search v2 (`TASK-0153`) provides a deterministic lexical engine (analyzers + BM25-like ranking + facets/snippets).
Search v2.1 adds an offline, deterministic “semantic-lite” layer that improves ranking without ML deps:

- hashed char n-gram vectors (embeddings-lite),
- rule/dictionary-based semantic tags,
- lexicon-based query expansion,
- hybrid scoring: \(score = bm25 + \lambda \cdot cos(q, d)\), with stable tie-breakers and deterministic explain output.

This is host-first and extends the Search v2 engine; OS/QEMU wiring is v2.1b.

## Goal

Deliver:

1. `userspace/libs/semvec` (embeddings-lite):
   - normalization: NFKC + lowercase (explicit rules, reused from analyzers)
   - char n-grams:
     - default n ∈ {3,4,5}, dims D=256
     - CJK hint may add bigrams deterministically
   - hashing: FNV-1a (documented) into buckets
   - quantization: int8 vector with a per-vector scale
   - cosine similarity implemented deterministically:
     - prefer fixed-point / integer dot products + deterministic scaling (avoid float drift)
     - if floats are used, they must be bounded and tests must compare with explicit tolerance
2. `userspace/libs/semantics` (semantic tags):
   - multi-lang dictionary/rule extractor producing stable tag lists
   - tags are normalized (lowercase, no spaces) and deduped deterministically
3. `userspace/libs/lexicon` (query expansion):
   - per-language synonym sets loaded from fixtures
   - deterministic cap on added terms (original tokens first)
4. Search engine integration (in `searchd`/searchcore from v2):
   - store semvec + tags alongside documents in the in-memory engine
   - query path:
     - compute query semvec
     - get lexical top-K candidates (default K=200)
     - rerank deterministically with hybrid score
   - filters:
     - support `tag:<name>` filter and expose tag facet counts (stable)
   - explain:
     - report bm25, cos, lambda, and top contributing n-grams (bounded list) deterministically
5. Deterministic host tests `tests/search_v2_1_host/`:
   - semvec determinism (byte-for-byte under fixed inputs)
   - hybrid reorder case (BM25-only A>B, hybrid B>A deterministically)
   - tags extraction + `tag:` filter correctness
   - query expansion improves rank deterministically
   - explain output stable (bounded and deterministic)

## Non-Goals

- Kernel changes.
- On-disk storage/schema migrations in this step (handled in `TASK-0154` / v2.1b if needed).
- ML embeddings or external model downloads.

## Constraints / invariants (hard requirements)

- Determinism:
  - stable ordering and tie-breakers: `(final_score desc, id asc)` remains the rule
  - avoid floating nondeterminism (prefer fixed-point)
- Bounded work:
  - cap candidates reranked (K)
  - cap explain details
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **YELLOW (float drift)**:
  - cosine similarity and normalization can introduce float drift across platforms. Prefer fixed-point math and integer dot products.

- **YELLOW (hash collisions)**:
  - hashed n-grams collide by design. This is “semantic-lite”; document limitations and keep tie-breakers stable.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p search_v2_1_host -- --nocapture`

## Touched paths (allowlist)

- `userspace/libs/semvec/` (new)
- `userspace/libs/semantics/` (new)
- `userspace/libs/lexicon/` (new)
- `source/services/searchd/` and/or `userspace/search/` (extend v2 engine)
- `tests/search_v2_1_host/` (new)
- fixtures under `pkg://fixtures/search/` and `pkg://fixtures/semantics/` (new/extend)

## Plan (small PRs)

1. semvec + deterministic cosine + tests
2. tags extractor + fixtures + tests
3. lexicon expansion + fixtures + tests
4. hybrid rerank + tag facets + explain + tests

## Acceptance criteria (behavioral)

- Host tests deterministically prove semantic-lite embedding determinism, hybrid ranking effects, tags/filtering, expansion, and explain stability.

