---
title: TASK-0153 Search v2 backend (host-first): real searchd index + analyzers + ranking + sources + CLI + deterministic tests
status: Draft
owner: @ui
created: 2025-12-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Existing search placeholder ADR: docs/adr/0010-search-architecture.md
  - Search v9a task (baseline): tasks/TASK-0071-ui-v9a-searchd-command-palette.md
  - Search v2 UI surface (host): tasks/TASK-0151-search-v2-ui-host-command-palette-model-a11y.md
  - Search v2 UI execution (OS): tasks/TASK-0152-search-v2-ui-os-deeplinks-selftests-postflight-docs.md
  - Config broker: tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Quotas model (for later OS persistence): tasks/TASK-0133-statefs-quotas-v1-accounting-enforcement.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Today `source/services/searchd` and `userspace/search` are placeholders (no real query/suggest/index).
Search v2 UI (`TASK-0151/0152`) requires a real backend with deterministic behavior:

- deterministic analyzers (EN/DE + CJK fallback tokenization),
- deterministic ranking (BM25-like + usage/freshness boosts),
- deterministic sources (apps/settings/files/recents) in offline mode,
- deterministic suggest/prefix behavior.

This task is **host-first** and provides the core engine + tests + CLI.
OS persistence and QEMU selftests are handled in `TASK-0154`.

## Goal

Deliver:

1. `searchd` backend engine with stable API contract:
   - `upsert/remove/query/suggest/reindex/stats`
   - stable document IDs (`kind://...`) and stable tie-breaking rules
2. Multilingual analyzers (offline, deterministic):
   - Normalization:
     - NFKC, lowercase, whitespace collapse, punctuation stripping (explicit rule set)
   - EN/DE:
     - word tokenizer, stemming OFF
     - diacritics folding (explicit), German umlaut folding handled deterministically
     - small bundled stopword set (explicit)
   - JA/ZH:
     - deterministic character bigrams (and optional trigrams) as a safe fallback
     - optional transliteration keys (romaji/pinyin) only if implemented deterministically (no external data downloads)
   - KO:
     - Hangul syllable → jamo decomposition for query-time normalization
     - plus deterministic 2-gram syllable fallback
   - deterministic edge-ngrams for `suggest`
3. Ranking:
   - BM25-like relevance + usage boost + freshness decay
   - injected clock for tests (no wallclock dependence)
   - stable ordering: `(final_score desc, id asc)`
4. Facets + snippet/highlight (deterministic, offline):
   - facets:
     - stable `kind/lang/tags` keys on docs
     - filtering reduces candidate set deterministically
   - snippet:
     - deterministic first-match window (clamped)
     - emphasis markers are plain-text only (no HTML), e.g. `«match»`
5. Source adapters (host fixtures):
   - apps/settings/files/recents adapters operate over deterministic fixture registries
   - optional store feed adapter (offline): indexes store cards (title/summary) when available
   - OS adapters exist as stubs here (wired in `TASK-0154`)
6. CLI `nx search` (host-first):
   - `query`, `suggest`, `reindex`, `stats`
   - deterministic output lines for parsing
7. Deterministic host tests:
   - analyzers (DE folding + CJK bigrams)
   - ranking (usage/freshness)
   - suggest stability
   - reindex over fixtures
   - facets + snippet determinism

## Non-Goals

- Kernel changes.
- Network indexing or remote sources.
- Full ICU correctness / large dictionaries.
- OS persistence (index-on-disk) in this step.

## Constraints / invariants (hard requirements)

- Deterministic behavior across runs given the same fixtures and injected time.
- Bounded memory usage: caps on docs per kind, token lengths, and posting lists.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success markers: do not emit “query ok” markers unless query actually ran.

## Red flags / decision points (track explicitly)

- **RED (Tantivy in OS / determinism)**:
  - The prompt suggests Tantivy. Tantivy is std + filesystem oriented and may introduce nondeterminism via segment merges
    and floating scoring unless carefully constrained.
  - Decision for v2:
    - **Option A (recommended for determinism + OS viability):** implement a small deterministic in-memory inverted index (BM25-lite),
      and keep Tantivy (if desired) as a host-only optional backend behind a feature flag.
    - **Option B:** use Tantivy everywhere only if OS build supports it and we can prove deterministic scoring/ordering under fixed settings.
  - This decision must be documented and proven by tests.

- **RED (storage backend drift: in-memory vs on-disk)**:
  - The prompt suggests a libSQL-backed index on disk. This task remains host-first and must prioritize determinism:
    - default engine should remain a deterministic in-memory index (recommended),
    - an on-disk store is allowed only if it is proven deterministic and OS-viable (no_std constraints apply).
  - OS persistence is handled in `TASK-0154` and is gated on `/state`.

## Contract sources (single source of truth)

- **Search service contract**: `TASK-0071` (IDL shape) and this task’s API definitions once implemented
- **Search v2 UI expectations**: `TASK-0151`/`TASK-0152`

## Stop conditions (Definition of Done)

- **Proof (tests / host)**:
  - Command(s):
    - `cargo test -p search_v2_host -- --nocapture` (or equivalent crate name)
  - Required tests:
    - analyzer correctness (DE + CJK)
    - ranking determinism (usage + freshness)
    - suggest determinism
    - reindex determinism over fixtures

## Touched paths (allowlist)

- `source/services/searchd/` (replace placeholder with real service core; host-first runnable)
- `userspace/search/` (replace placeholder library)
- `userspace/libs/search-analyzers/` (new, if needed)
- `tools/nx-search/` (new)
- `tests/search_v2_host/` (new)
- `docs/search/` (added in `TASK-0154`)

## Plan (small PRs)

1. Define stable doc/query/hit types + deterministic analyzer module + tests
2. Implement deterministic index + ranking + suggest + tests
3. Add fixture-driven source adapters + `reindex` + tests
4. Add `nx-search` CLI (host-first)

## Acceptance criteria (behavioral)

- Host tests are deterministic and green.
- `searchd` provides real `suggest/query/reindex` behavior over fixtures.

Follow-up:

- Search v2.1 semantic-lite layer (hashed char n-gram vectors + tags + query expansion + hybrid rerank + explain) is tracked as `TASK-0213`/`TASK-0214`.
