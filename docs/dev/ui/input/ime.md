<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# IME

Contract: `docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md`.
Execution: TASK-0146 (host core) → TASK-0147 (OS wiring + OSK) →
TASK-0149/0150 (CJK + candidates) → TASK-0203/0204 (personalization).

## Architecture (RFC-0075)

```
hidrawd → inputd ──(keymap resolve, exists)──► imed ──► windowd ──► focused surface → app-host
                     KeyOutput::{Text,Dead,      composition        OP_SURFACE_TEXT     insert into
                     Action} while text-focused  (ime-core)         push                focused field
```

- **imed** is the IME authority (TRACK-AUTHORITY-NAMING); `source/services/ime`
  is deprecated and gets deleted in TASK-0147.
- **ime-core** (`userspace/ime-core`): no_std, alloc-free composition —
  dead-key/compose state machine (DE `´` `` ` `` `^`), bounded preedit,
  deterministic `ImeOutcome`.
- **Engines (TASK-0149)**: ONE `ImeEngine` trait (feed/select/page_next/
  reset → bounded `EngineOutcome` snapshots: preedit ≤ 64 B, candidates ≤
  8 × 32 B/page); `Engine` enum-dispatch hosts Latin (the composer), **JP**
  (romaji→kana longest-match, っ sokuon, ん rules, const kana→kanji lexicon
  — the reading is always the last candidate), **KR** (2-set jamo, Unicode
  syllable algebra, compound medials/finals, jamo-splitting backspace) and
  **ZH** (pinyin exact-buffer lookup, paging). `EngineId::for_layout`
  follows `input.keymap` (unknown → Latin, fail-open to plain typing).
  Const tables are correctness-proof sized; real lexica ride bundle assets
  in a later slice.
- **User dictionary (TASK-0149 API, storage = TASK-0203/0204)**: bounded
  in-memory `train`/`lookup`/`forget` (≤ 1024 entries/lang), frequency
  ranking with insertion-order tie-breaks, lowest-freq-oldest-first
  eviction — training is a separate call so imed's password gate cannot be
  bypassed.
- **Keymaps** stay in `userspace/keymaps` (TASK-0252); dead keys are marked
  `KeyOutput::Dead(char)` and only the composer interprets them.
- **OSK + candidate strip** live in the `ime-ui` DSL overlay app — never in
  windowd (compositor stays UI-free).

## Wire

- `nexus-wire/src/imed.rs` (MAGIC `'I','E'`): `OP_SET_FOCUS` (windowd→imed),
  `OP_KEY` (inputd/OSK→imed), `OP_COMMIT`/`OP_PREEDIT`/`OP_CANDIDATES`
  (imed→windowd push), `OP_CANDIDATE_SELECT`.
- `nexus-display-proto/src/surface_text.rs`: `OP_SURFACE_TEXT` (windowd→app),
  `OP_SURFACE_TEXT_FOCUS` (app→windowd, caret rect = OSK/popup anchor).

## Security invariants

- imed accepts `OP_KEY` only from inputd (`sender_service_id`); OSK keys
  only via imed's DEDICATED `imed-osk` endpoint — possession of the route
  cap is the authorization (`nexus.permission.IME`, `ime` bundle-type
  ceiling; RFC-0075 Phase 2).
- Password fields (`field_kind=password`): no preedit push, no candidates,
  no personalization learning — enforced in imed, fail-closed.
- Typed text NEVER appears in logs or markers; selftests use fixed fixtures.

## Markers

`imed: ready` · `SELFTEST: ime v2 latin us ok` · `SELFTEST: ime v2 deadkeys de
ok` · `SELFTEST: ime v2 osk ok` · `imed: reject foreign key source` ·
`SELFTEST: ime v2 cjk jp ok` · `SELFTEST: ime v2 candidates ok` ·
`SELFTEST: ime ranking persist ok` (see the RFC's marker ladder).
