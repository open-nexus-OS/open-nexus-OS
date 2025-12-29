---
title: TASK-0149 IME v2 Part 2a (host-first): JP/KR/ZH engines + user dictionary persistence (OS-gated)
status: Draft
owner: @ui
created: 2025-12-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - IME v2 Part 1: tasks/TASK-0146-ime-text-v2-part1a-imed-keymaps-host.md
  - IME v2 Part 1 OS wiring: tasks/TASK-0147-ime-text-v2-part1b-osk-focus-a11y-os-proofs.md
  - Text stack integration (bidi/breaks/shaping): tasks/TASK-0148-textshape-v1-deterministic-bidi-breaks-shaping-contract.md
  - Candidate UI + OS proofs (Part 2b): tasks/TASK-0150-ime-v2-part2b-candidate-popup-osk-cjk-os-proofs.md
  - Persistence substrate: tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Quotas (per-lang bounds): tasks/TASK-0133-statefs-quotas-v1-accounting-enforcement.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Part 2 expands IME beyond Latin/US/DE:

- JP: romaji→kana and a tiny deterministic kana→kanji candidate generator
- KR: hangul jamo composition (2-set baseline; 3-set optional)
- ZH: pinyin→han candidates (simp/trad options)
- user dictionary persistence and learning (bounded)

This is primarily **host-first** (deterministic engines + tests). Persistence is **OS-gated** on `/state`.

Naming note:

- The repo currently contains a placeholder service crate `source/services/ime` (prints `ime: ready` and uppercases text).
- IME v2 tasks introduced `imed` as the intended daemon name. This task assumes we standardize on **one** daemon
  (prefer `imed`) and either retire or rename `ime` to avoid duplicate “IME” services.

## Goal

Deliver:

1. Common IME engine interface (no unsafe):
   - `userspace/libs/ime-core` defines `ImeEngine` trait, `EngineAction`, bounded buffers, deterministic candidate ordering.
2. Engines:
   - `userspace/libs/ime-jp`:
     - romaji→kana table (deterministic)
     - tiny bundled lexicon for kana→kanji candidates (deterministic)
     - options: `jp.mode=romaji|kana`
   - `userspace/libs/ime-kr`:
     - hangul composition (2-set default), correct backspace splitting
     - option: `kr.layout=2set|3set` (3-set can be stubbed explicitly)
   - `userspace/libs/ime-zh`:
     - pinyin tokenizer (tone-less + tone numbers)
     - bundled simp/trad lexicon with deterministic ranking + pagination
     - option: `zh.variant=simp|trad`
3. `imed` wiring:
   - engine selection by language
   - expose deterministic snapshots: current preedit segments + candidate list + selected index
4. User dictionary + learning:
   - `userspace/libs/ime-dict` stores per-lang user phrases
   - bounded size (e.g., 1 MiB per language) and deterministic eviction policy
   - “learn on commit”: committing a candidate bumps frequency deterministically
   - persistence path (OS-gated): `state:/ime/user/<lang>/user.dict.jsonl`

5. Predictive dictionary API (engine-facing, deterministic):
   - provide a small shared API used by JP/ZH engines:
     - `lookup(lang, input, limit) -> candidates`
     - `train(lang, input, cand)`
   - default backend (v2): deterministic JSONL/trie tables (host-first), aligned with `ime-dict` storage
   - optional host-only backend:
     - a SQLite/libSQL-backed implementation is allowed **only** if it is kept host-first and does not
       introduce an OS dependency that cannot run in no_std builds

## Non-Goals

- Kernel changes.
- Production-grade dictionaries / large data sets.
- Full-blown language models.
- Perfect segmentation correctness (tiny deterministic lexicon is OK for v2).

## Constraints / invariants (hard requirements)

- Deterministic candidate ranking and pagination.
- Bounded memory and bounded candidate lists.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Stubs are explicit: if an engine mode is not implemented, return `Unsupported/Placeholder` and never claim success markers.

## Red flags / decision points (track explicitly)

- **RED (persistence requires `/state`)**:
  - user dictionary persistence is OS-gated on `TASK-0009`.
  - host tests must still cover serialization and round-trip behavior deterministically.

- **YELLOW (service naming drift)**:
  - decide whether `imed` replaces `ime`, or whether `ime` is renamed to `imed`.
  - tasks and markers must be consistent (`imed: ready` vs `ime: ready`).

- **RED (SQLite/libSQL viability in OS)**:
  - OS userland is `no_std` in many components; a SQLite/libSQL dependency is likely not viable.
  - Do not make JP/ZH engines depend on SQLite in OS. If we want SQLite, keep it host-only behind a feature flag and provide a pure-Rust deterministic backend for OS.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - Command(s):
    - `cargo test -p ime_v2_part2_host -- --nocapture` (or equivalent crate name)
  - Required coverage:
    - JP: romaji→kana + candidate generation + selection commit
    - KR: hangul composition + backspace splitting
    - ZH: pinyin→candidates (simp/trad option) + pagination stability
    - dict: learn → top candidate, serialize/deserialize round-trip

- **Proof (OS/QEMU) — gated**
  - Only after `/state` is available and `TASK-0150` is implemented:
    - `SELFTEST: ime v2 dict ok`

## Touched paths (allowlist)

- `userspace/libs/ime-core/` (new)
- `userspace/libs/ime-jp/` (new)
- `userspace/libs/ime-kr/` (new)
- `userspace/libs/ime-zh/` (new)
- `userspace/libs/ime-dict/` (new)
- `source/services/imed/` (wire engines)
- `docs/input/ime-cjk.md` (added in Part 2b)

## Plan (small PRs)

1. `ime-core` trait + `ime-dict` (host serialization + bounded eviction) + tests
2. Implement JP/KR/ZH engines with fixtures + tests
3. Wire engines into `imed` with deterministic snapshot outputs

## Acceptance criteria (behavioral)

- Host tests deterministically validate JP/KR/ZH engines + user dict learning/round-trip.
- No OS success markers are claimed until Part 2b wires UI + OS selftests.

Follow-up:

- IME v2.1 (adaptive ranking, context bigrams, forget semantics, deterministic export/import, SecureFS-backed personalization store) is tracked as `TASK-0203`/`TASK-0204`.
