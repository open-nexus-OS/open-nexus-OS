---
title: TASK-0151 Search v2 UI (host-first): command-palette overlay + chip model + keyboard nav + a11y + deterministic tests
status: Draft
owner: @ui
created: 2025-12-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Search backend (searchd v9a): tasks/TASK-0071-ui-v9a-searchd-command-palette.md
  - Search v2 backend (real engine): tasks/TASK-0153-search-v2-backend-host-index-ranking-analyzers-sources.md
  - Shortcuts/overlays baseline: tasks/TASK-0070-ui-v8b-wm-resize-move-shortcuts-settings-overlays.md
  - Recents substrate: tasks/TASK-0082-ui-v11b-thumbnailer-recents.md
  - A11y baseline: tasks/TASK-0114-ui-v20a-a11yd-tree-actions-focusnav.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

`TASK-0071` introduces `searchd` and an initial command palette UI. This task extracts a **Search v2 UI surface**
that is deterministic, accessible, and testable host-first:

- command-palette overlay UX (omnibox + suggest + results + zero-query),
- result “chip” model and rendering,
- keyboard selection/activation model,
- a11y announcements and roles.

Deep-link execution and OS/QEMU markers are handled in `TASK-0152`.

Backend note:

- Current `source/services/searchd` + `userspace/search` are placeholders. Search v2 UI requires a real backend;
  that backend work is tracked in `TASK-0153`/`TASK-0154`.

## Goal

Deliver:

1. SystemUI overlay: Search command palette
   - omnibox with debounce (deterministic)
   - IME-aware text input:
     - queries run on **committed** text only (never on preedit)
     - preedit (if present) is rendered as a faint inline hint but does not affect query results
   - suggestions while typing (`searchd.suggest`)
   - results view (`searchd.query`)
   - zero-query “springboard” when empty input
   - filter pills (All/Apps/Files/Settings/Recents)
   - markers (throttled):
     - `searchui: open`
     - `searchui: close`
     - `searchui: suggest n=<n>`
     - `searchui: results n=<n>`
     - `searchui: zeroquery n=<n>`
2. Result chip model
   - stable mapping from `searchd` hit → `Chip { id, kind, title, subtitle, uri, ... }`
   - deterministic shortcut hints for top ranks (`Ctrl+1..9`)
3. Keyboard & selection model
   - arrows/page/home/end selection
   - enter activates selected
   - `Ctrl+1..9` activates ranked chip
   - selection stability across suggest→results transitions (best-effort; deterministic rules)
   - markers:
     - `searchui: select idx=<n> kind=<kind>`
     - `searchui: activate id=<id>`
4. Accessibility & i18n (host-first)
   - roles: searchbox/listbox/option
   - SR announcements for result counts and selection changes (polite; throttled)
   - string resources in EN/DE at least (no network, deterministic)
5. Deterministic host tests
   - suggest→results flow
   - keyboard nav + shortcuts activation (router is mocked)
   - zero-query ordering and counts (fixtures)
   - a11y announcement fixtures
   - IME semantics fixture:
     - preedit-only input produces zero queries
     - committing text triggers suggest/query deterministically

## Non-Goals

- Kernel changes.
- Deep-link execution on OS (see `TASK-0152`).
- Persistent search index on disk (still out of scope; `searchd` remains in-memory).
- Real filesystem indexing (still stubbed; file results come from providers/fixtures).

## Constraints / invariants (hard requirements)

- Determinism: stable ordering, stable tie-breakers, stable debounce behavior (virtual clock in tests).
- Bounded UI work: cap results shown and cache sizes in the overlay.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success markers: only emit `searchui:*` when the corresponding UI action really happened.

## Red flags / decision points (track explicitly)

- **YELLOW (overlap with TASK-0071 UI)**:
  - `TASK-0071` already includes “command palette UI”. This task defines the v2 UX contract and tests.
  - Implementation should reuse/extend the same overlay rather than creating two palettes.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - Command(s):
    - `cargo test -p search_v2_ui_host -- --nocapture`
  - Required coverage:
    - suggest→results mapping and stable ranking display
    - keyboard navigation and activation hooks
    - zero-query springboard ordering
    - a11y announcement fixtures

## Touched paths (allowlist)

- `userspace/systemui/overlays/search_palette/` (new or refactor existing palette code into this path)
- `tests/search_v2_ui_host/` (new)
- `docs/search/ui.md` (added in `TASK-0152` if OS-facing behavior is included there)

## Plan (small PRs)

1. Overlay UX + chip model + deterministic mapping rules
2. Keyboard selection/activation model + a11y hooks
3. Host tests and fixtures

## Acceptance criteria (behavioral)

- Host tests are deterministic and green.
- Palette UI emits markers for open/close/suggest/results/zeroquery/selection/activation (throttled).

Follow-up:

- Search v2.1 palette semantic chips + hybrid explain surfacing is tracked as `TASK-0214` (backend work in `TASK-0213`).
