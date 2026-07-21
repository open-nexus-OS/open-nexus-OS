---
title: TASK-0150 IME v2 Part 2b (OS/QEMU): candidate strip in ime-ui + CJK OSK layouts + selftests
status: Draft
owner: @ui
created: 2025-12-26
updated: 2026-07-21 (rewritten against repo reality; candidate UI lives in the ime-ui overlay app, not windowd)
depends-on:
  - TASK-0147
  - TASK-0149
follow-up-tasks:
  - TASK-0203 / TASK-0204 (adaptive ranking + persistence)
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Contract: docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md
  - OSK app baseline: tasks/TASK-0147-ime-text-v2-part1b-osk-focus-a11y-os-proofs.md
  - CJK engines: tasks/TASK-0149-ime-v2-part2-cjk-engines-userdict.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

With CJK engines host-proven (TASK-0149), this task wires the visible half:
the candidate strip and CJK OSK layouts — both inside the **ime-ui overlay
app** from TASK-0147. windowd only composites and routes; the caret rect
carried by `OP_SURFACE_TEXT_FOCUS` is the popup anchor. The imed wire ops
(`OP_PREEDIT`/`OP_CANDIDATES`/`OP_CANDIDATE_SELECT`) already exist from
TASK-0146 — this task is their first real consumer.

## Goal

1. **Candidate strip** in ime-ui: anchored near the focus caret rect,
   shows preedit underline text + up to 8 candidates + paging; selection via
   number keys, arrows+Enter, Tab, tap; Escape cancels composition.
2. **CJK OSK layouts** in ime-ui: JP kana, KR 2-set, ZH latin (pinyin);
   layout follows `input.keymap`.
3. imed: engine wiring so hw + OSK keys drive preedit/candidate pushes for
   the active CJK engine; `OP_CANDIDATE_SELECT` commits.
4. Selftests: deterministic injected sequences prove conversion + selection
   end-to-end at the app side.

## Non-Goals

- No adaptive/personalized ranking (TASK-0203/0204) — table order only.
- No a11y listbox roles yet (a11y track).
- No windowd-side popup drawing or anchoring logic beyond passing the caret rect.

## Constraints / invariants (hard requirements)

- Bounded pushes: candidates ≤ 8×32 B per frame, paging by page index —
  never a full-lexicon dump over IPC.
- Fixed buffers in imed for candidate frames; no per-key allocation.
- Overlay positioning: on-screen clamping so the strip never leaves the
  visible area (atlas over-read trap: clamp to surface dims).
- Markers honest; marker changes ride qemu-test.sh + markers.txt + docs together.

## Security considerations

- Candidate selection (`OP_CANDIDATE_SELECT`) accepted only from windowd
  (relayed UI) — same sender gate as `OP_SET_FOCUS`; `test_reject_*` present.
- Password fields: no candidate strip, no preedit push (invariant from
  RFC-0075, re-proven here with a negative selftest path host-side).
- No typed text in logs/markers; selftest fixtures fixed.

## Contract sources (single source of truth)

- **QEMU marker contract**: `scripts/qemu-test.sh` + `tools/nx/chains/markers.txt`
- **Wire contract**: nexus-wire imed goldens (unchanged; consumer-only task)

## Stop conditions (Definition of Done)

- **Proof (QEMU)**:
  - `SELFTEST: ime v2 cjk jp ok` — injected romaji fixture → expected kanji
    commit observed app-side
  - `SELFTEST: ime v2 candidates ok` — candidate push → select → commit round-trip
- **Proof (interactive)**: `just start` — switch keymap to jp in Settings,
  type romaji in a TextField, pick a candidate from the strip (touch + keys).
- **Gates**: `just check`, `just test-all` green; RFC-0075 checklist updated.

## Touched paths (allowlist)

- `userspace/apps/ime-ui/` (candidate strip + CJK layouts)
- `source/services/imed/` (engine wiring, candidate frame emit)
- `source/apps/selftest-client/`
- `scripts/qemu-test.sh`, `tools/nx/chains/markers.txt` — **approval zone**
- `docs/dev/ui/input/ime.md`, `CHANGELOG.md`

## Plan (small PRs)

1. imed engine wiring + candidate pushes + host tests.
2. ime-ui candidate strip (anchor, paging, selection) + JP OSK layout.
3. KR/ZH OSK layouts + selftests + markers + docs.

## Acceptance criteria (behavioral)

- JP/KR/ZH typing works end-to-end with visible candidates and commit by
  tap, number key, or Enter — deterministic under the selftest fixtures.
- Candidate strip follows the caret between fields and never renders for
  password fields.
