---
title: TASK-0149 IME v2 Part 2a (host-first): JP/KR/ZH engines in ime-core + bounded user-dict API
status: Draft
owner: @ui
created: 2025-12-26
updated: 2026-07-21 (rewritten against repo reality; engines live in ime-core, persistence moved to TASK-0204 on statefsd)
depends-on:
  - TASK-0146
  - TASK-0147
follow-up-tasks:
  - TASK-0150 (candidate popup + CJK OSK layouts)
  - TASK-0203 / TASK-0204 (adaptive ranking + persistence)
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Contract: docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md
  - Host foundation: tasks/TASK-0146-ime-text-v2-part1a-imed-keymaps-host.md
  - OS wiring: tasks/TASK-0147-ime-text-v2-part1b-osk-focus-a11y-os-proofs.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

With Latin typing proven end-to-end (TASK-0147), Part 2a adds the CJK engines —
**host-only, deterministic, bounded**. Changes vs the 2025-12 draft:

- Engines live as modules in `userspace/ime-core` (one crate, one
  `ImeEngine` trait), not four separate `ime-jp/kr/zh/dict` crates — the
  module-per-language split inside one crate keeps the workspace lean and the
  600-LOC ratchet honest.
- Persistence is **not here**: this task defines the in-memory bounded
  user-dict API; storage semantics land in TASK-0203/0204 (statefsd, not the
  nonexistent securefsd).
- Engine selection follows the `input.keymap` setting (`jp|kr|zh`, TASK-0298);
  the keymap tables for jp/kr/zh already exist in `userspace/keymaps`.

## Goal

1. `ImeEngine` trait in ime-core (key event in → preedit/candidates/commit out;
   deterministic snapshots, bounded candidates ≤ 8×32 B).
2. **JP**: romaji→kana composition + tiny const kana→kanji lexicon (candidate
   generator; deterministic ordering).
3. **KR**: 2-set hangul jamo composition incl. backspace jamo-splitting.
4. **ZH**: pinyin→han candidates from a bounded const table (simplified first;
   traditional variant column where the table has it).
5. Bounded user-dict API (`lookup`/`train`/`forget`, per-lang caps,
   deterministic eviction order) — in-memory trait, storage-agnostic.

## Non-Goals

- No persistence (TASK-0204), no adaptive ranking (TASK-0203).
- No candidate popup UI / OSK layouts (TASK-0150); no QEMU markers here.
- No large lexica or language models — const tables sized for correctness
  proofs and everyday phrases, not corpus coverage.
- No bidi/shaping (TASK-0148 stays Deferred).

## Constraints / invariants (hard requirements)

- no_std, alloc-free, zero deps; const tables only (no file I/O in engines).
- Deterministic: same key sequence → same candidate list, always; stable
  tie-breakers (table order, then codepoint).
- Bounded: preedit ≤ 64 B, candidates ≤ 8 per page, user-dict ≤ 1024
  entries/lang in-memory.
- No `unwrap`/`expect`; engines never panic on arbitrary key sequences
  (fuzz-style host test over random `KeyOutput` streams).

## Security considerations

- Engines process untrusted key sequences: all state transitions bounded;
  the random-stream test proves no panic/overflow.
- `field_kind=password` never reaches engines with learning enabled
  (enforced in imed, re-asserted here by API design: `train` is a separate
  call the caller must gate).

## Contract sources (single source of truth)

- **Engine semantics**: RFC-0075 (composition contract section extended by
  this task with the engine trait; still one RFC — same behavior family).
- **Conversion goldens**: `userspace/ime-core/tests/` fixtures.

## Stop conditions (Definition of Done)

- **Proof (host)**: `cargo test -p ime-core` —
  - JP: `nihongo` → にほんご → 日本語 candidate first; っ/ん edge cases.
  - KR: 한 from ㅎ+ㅏ+ㄴ; backspace splits to 하.
  - ZH: `nihao` → 你好 candidate first; paging beyond 8 candidates.
  - user-dict: train/lookup/forget/eviction determinism.
  - random-stream no-panic test.
- **Gates**: `just check` + `just test-host` green; no QEMU claims.

## Touched paths (allowlist)

- `userspace/ime-core/` (engine modules + tests)
- `docs/dev/ui/input/ime.md` (engine section), `CHANGELOG.md`

## Plan (small PRs)

1. `ImeEngine` trait + engine selection plumbing + JP engine + goldens.
2. KR engine + goldens.
3. ZH engine + goldens + user-dict API + eviction tests + fuzz-style test.

## Acceptance criteria (behavioral)

- All conversion goldens green and deterministic across runs.
- imed can host any engine behind the trait without imed-side changes
  (proven by a host test swapping engines on one session).
