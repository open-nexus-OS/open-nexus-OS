---
title: TASK-0150 IME v2 Part 2b (OS/QEMU): candidate strip in ime-ui + CJK OSK layouts + selftests
status: Done (2026-07-22)
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

## Result (2026-07-22)

Landed per RFC-0075 Phase 3 with these shape deltas vs the plan above:

- The strip lives INSIDE the OSK band (ime-ui top row), not as a separate
  caret-anchored popup — the caret rect stays recorded for the future
  floating popup; the OSK band is the v1 anchor.
- imed hosts `ime_core::Engine`: composition is focus-INDEPENDENT (the
  deterministic probes exercise the real engine without a field), delivery
  stays focus-gated, PASSWORD fields bypass the engine entirely.
- `OP_SET_LAYOUT=8` (new, additive): inputd relays `input.keymap` on the
  main endpoint; the OSK globe cycles de→us→jp→kr→zh over the
  capability-gated osk endpoint (`svc.ime.layout`). The osk reply echoes
  the step's commit to the INJECTING sender only (probe observability).
- Strip data path: imed `OP_PREEDIT`/`OP_CANDIDATES` → windowd →
  `OP_SURFACE_IME_STATE=24` → ime-ui `ImeStripEvent::Preedit/Cands`;
  candidate taps ride `svc.ime.select`. KR OSK rows show 2-set jamo labels;
  jp/zh ride the us rows (romaji/pinyin). OSK shows in EVERY profile
  (profile = layout, not keyboard presence; HID-presence hiding = follow-up).
- Proofs: `SELFTEST: ime v2 cjk jp ok` (layout jp, `nn`+Enter echoes ん) and
  `SELFTEST: ime v2 candidates ok` (`nihao`+space, select(0) commits 你好)
  green in `ci-os-smp1`; INTERACTIVE: composer focus → OSK → globe → jp →
  romaji preedit in the strip → candidates (kanji + reading) → tap →
  `apphost: text commit applied` + strip cleared (visible boot 2026-07-22).
- KNOWN GAP (recorded): the UI font has NO CJK glyph coverage — strip and
  fields render `?` for kana/hangul/han. The byte path is proven end-to-end
  (probes + markers); glyph coverage is a FONT task, not IME logic.
- ja/ko/zh locale catalogs added for all six `@t()` apps (with de parity).

## Addendum 8b (2026-07-22): data-driven layouts + env axes

User insert after Phase 8: 180 languages must never mean 180 `if` trees.

- `userspace/keymaps::osk_rows(LayoutId, row) -> &[OskKey{label,key,action}]`
  is the layout-data SSOT (KR shows jamo labels over 2-set Latin keys;
  jp/zh share the us rows). Golden: `keymaps_contract.rs`.
- `svc.ime.rows(layout, row) -> List<OskKey>` (app-host answers natively
  from the SSOT); ime-ui renders four `List(...)` templates — the OskPage
  layout branches are GONE (the existing collection mechanism carries it;
  a new KeyRow primitive was evaluated and REJECTED as accidental
  complexity — no compile-time child generation exists or is needed).
- `KeymapEvent::Changed(tag)` (app-host, region-push driven) reloads the
  rows; `svc.ime.cycle(current)` cycles the SYSTEM layout (order = platform
  data) — imed persists `input.keymap` via a new settingsd route
  (init-wired slots 8/9/10; cycle guard: the inputd relay of the same tag
  never re-writes).
- `device.locale` / `device.keymap` are env axes (DEVICE_FIELDS rows 7/8,
  `FixtureEnv` runtime-varying String fields) for the rare STRUCTURAL
  per-region arms; `OP_SURFACE_REGION` gained an optional trailing keymap
  field (old frames decode with an empty tag) and windowd a third watch
  subscription (`input.keymap`).
