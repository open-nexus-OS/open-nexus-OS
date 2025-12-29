---
title: TASK-0097 UI v15d: spellcheck service (spellerd) + suggestions + underlines + deterministic dictionaries
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Text primitives: tasks/TASK-0094-ui-v15a-text-primitives-uax-bidi-hittest.md
  - TextField core: tasks/TASK-0095-ui-v15b-selection-caret-textfield-core.md
  - Policy as Code (no network): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Config broker (langs): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
---

## Context

Spellcheck is a productivity feature and also a “text pipeline stress test”.
We keep it deterministic, local-only, and bounded:

- dictionary packs shipped in `/system/dict/`,
- fast tokenization using UAX#29 primitives,
- suggestion ranking via Levenshtein + simple frequencies (deterministic).

## Goal

Deliver:

1. `spellerd` service:
   - IDL `speller.capnp`:
     - `check(lang,text)` returns miss indices
     - `suggest(lang,token,limit)` returns alternatives
   - dictionary packs (wordlist + optional frequency stub)
   - markers:
     - `spellerd: ready`
     - `spell: miss n=..`
     - `spell: fix "<from>"->"<to>"`
2. Integration with `textedit_core`/TextField:
   - underline misspellings
   - context menu suggestions and apply replacement
3. Host tests for miss detection and deterministic suggestions.

## Non-Goals

- Kernel changes.
- Network dictionary fetch.
- Full affix morphology engines.

## Constraints / invariants

- Deterministic tokenization and suggestion ranking.
- Bounded CPU and memory:
  - cap token length,
  - cap max suggestions,
  - cap dictionary size per language (for v15).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v15d_host/`:

- misspelling detection for fixture sentences
- suggestion list for known token matches goldens
- applying suggestion updates underline spans deterministically (model-level)

### Proof (OS/QEMU) — gated

UART markers:

- `spellerd: ready`
- `SELFTEST: ui v15 spell ok`

## Touched paths (allowlist)

- `source/services/spellerd/` (new)
- `userspace/ui/textedit_core/` and/or TextField integration points
- `tests/ui_v15d_host/`
- `docs/ui/spellcheck.md` (new)

## Plan (small PRs)

1. spellerd core + dictionaries + markers
2. TextField integration (underlines + suggestions) + markers
3. host tests + docs
