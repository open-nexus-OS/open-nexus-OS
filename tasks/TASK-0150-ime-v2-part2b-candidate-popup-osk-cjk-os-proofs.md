---
title: TASK-0150 IME v2 Part 2b (OS-gated): candidate popup UI + OSK JP/KR/ZH layouts + selftests/postflight + docs
status: Draft
owner: @ui
created: 2025-12-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - IME v2 Part 1 OS wiring: tasks/TASK-0147-ime-text-v2-part1b-osk-focus-a11y-os-proofs.md
  - IME v2 Part 2a engines/dict: tasks/TASK-0149-ime-v2-part2-cjk-engines-userdict.md
  - Existing IME+OSK umbrella: tasks/TASK-0096-ui-v15c-ime-candidate-ui-osk.md
  - Text primitives + caret anchoring: tasks/TASK-0094-ui-v15a-text-primitives-uax-bidi-hittest.md
  - TextField core (caret/selection): tasks/TASK-0095-ui-v15b-selection-caret-textfield-core.md
  - Quotas (dict bounds): tasks/TASK-0133-statefs-quotas-v1-accounting-enforcement.md
  - Policy caps (input): tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

With CJK engines present (Part 2a), we need the UI integration:

- a candidate popup anchored at caret,
- selection/paging via keyboard and OSK,
- OSK layouts for JP/KR/ZH,
- OS selftests + postflight + docs.

This task is OS/QEMU-facing and must remain bounded and deterministic.

## Goal

Deliver:

1. Candidate popup overlay (SystemUI):
   - anchored to caret rect
   - shows preedit (seg underline) + candidate list + paging
   - selection:
     - arrows / tab navigation
     - enter commits selected candidate
     - mouse/touch selects and commits
   - A11y:
     - listbox/listitem roles, polite announcements (“3 candidates”)
   - markers:
     - `ime:popup open`
     - `ime:popup select idx=<n>`
     - `ime:popup commit`
2. OSK layouts extension:
   - JP kana layout (full keyboard layout; phone-like 12-key is a follow-up)
   - KR 2-set layout (3-set can be an explicit stub behind an option)
   - ZH uses latin layout (pinyin); tone digits as long-press (v1 can be stubbed explicitly)
   - markers:
     - `osk: layout jp`
     - `osk: layout kr`
     - `osk: layout zh`
3. Policy + focus guards:
   - commits only delivered to focused editable
   - option/dict changes gated by caps (`input.ime.options`, `input.ime.dict.write`)
   - async composition events:
     - candidate selection and preedit updates are delivered asynchronously (event queue) per focused window/session
     - bounded queue length; deterministic draining order (no reentrancy into UI)
4. Proof:
   - host tests for overlay selection logic are optional, but OS markers are required
   - OS selftests for JP/KR/ZH + dict learn/persist (dict persist gated on `/state`)
   - postflight delegates to canonical proofs (host tests + `scripts/qemu-test.sh`)
5. Docs:
   - `docs/input/ime-cjk.md`
   - `docs/text/textshape.md` (link to `TASK-0148`)
   - update `docs/systemui/osk.md` with CJK layouts

## Non-Goals

- Kernel changes.
- Multi-VM input sharing.
- Full production candidate UX (fuzzy search, cloud dicts, etc.).

## Constraints / invariants (hard requirements)

- Deterministic candidate ordering and selection behavior.
- Bounded list sizes and bounded UI work per frame.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success markers: popup/osk markers only when UI actually opened/handled events.
 - Async composition is deterministic:
   - a session has a stable ID,
   - events are processed FIFO per session,
   - focus change clears/isolates sessions deterministically.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - Command(s):
    - `cargo test -p ime_v2_part2_host -- --nocapture`

- **Proof (QEMU)**:
  - Command(s):
    - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=195s ./scripts/qemu-test.sh`
  - Required markers (to be added to `scripts/qemu-test.sh` expected list):
    - `text: bidi on` (or equivalent from `TASK-0094`/`TASK-0148` when enabled)
    - `imed: ready`
    - `SELFTEST: ime v2 jp ok`
    - `SELFTEST: ime v2 kr ok`
    - `SELFTEST: ime v2 zh ok`
    - `SELFTEST: ime v2 dict ok` (only when `/state` persistence is available; otherwise must be explicitly skipped with a `stub/placeholder` marker)

## Touched paths (allowlist)

- `userspace/systemui/overlays/ime_popup/` (new)
- `userspace/systemui/overlays/osk/` (extend)
- `source/apps/selftest-client/`
- `tools/postflight-ime-v2-part2.sh`
- `docs/input/` + `docs/systemui/` + `docs/text/`
- `scripts/qemu-test.sh` (marker contract update)

## Plan (small PRs)

1. Candidate popup overlay + a11y + markers
2. OSK CJK layouts + long-press minimal hooks (explicit stubs OK)
3. Selftests + marker contract + docs + postflight

## Acceptance criteria (behavioral)

- In QEMU, JP/KR/ZH flows reach commit via candidate popup and are proven via selftest markers.
- User dict learn/persist is proven when `/state` is available; otherwise it is explicitly marked as `stub/placeholder` (never “ok”).
