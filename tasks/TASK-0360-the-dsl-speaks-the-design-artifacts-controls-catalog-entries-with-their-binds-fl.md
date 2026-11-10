---
schema: 1
id: TASK-0360
title: "The DSL speaks the design artifacts' controls: catalog entries with their binds, floating surfaces anchored to a trigger, the triggers and modifiers the artifacts use"
status: draft
owner: ["@ui"]
created: 2026-11-10
updated: 2026-11-10
subsystem: ui
kind: ui
novelty: first
estimate: 5
goal:
  metric: "design deltas open because the DSL lacks the control, the anchored surface, the trigger or the modifier the artifacts use"
  baseline: "77 counted 2026-11-10 (moved from TASK-0353 P3)"
  target: "0 open; the moved ids closed"
  instrument: "the TASK-0353 audit tables re-run against the spec files"
consumer: hardware-network/LU.1
phases:
  - id: P0
    story: "an RFC names the append-only additions the artifacts need — catalog entries and their bind kinds, a floating surface anchored to its trigger and its dismiss, the hover, secondary-click and submit triggers, the environment modifiers, record literals and string builtins — and settles the token and tier questions TASK-0353 handed over"
    state: planned
    gate: "the RFC accepted; every moved delta id (TASK-0353 P3) mapped to one of its sections"
    note: "moved from TASK-0353 (2026-11-10, the operator's \"move them, then close\"; ids of the design spec and the P5 transition inventory): primitives-core CTL-4; primitives-window IC-1; primitives-window AB-9; primitives-window WS-7 WM-3 WM-17 IC-4 AW-4; shell-panels PN-GLOW; launcher-lock E2 L13 L25 L37 L45 L49; chat H11 H16; chat H6; settings-stash A10; settings-stash A13; settings-stash S45"
  - id: P1
    story: "the value controls are DSL widgets with binds: RadioGroup, Segment, Stepper, Rating, TextArea with multiline editing, PinField, a Slider with drag and min/max/step, a Select with options and a picked value, AppIcon, SkeletonText; Button icon and loading, a button-look Toggle with an icon, ProgressBar indeterminate"
    state: planned
    gate: "host: a bind round-trip test per entry, goldens light and dark, widgets.md entries compile (docs gate)"
    note: "moved from TASK-0353 (2026-11-10, the operator's \"move them, then close\"; ids of the design spec and the P5 transition inventory): primitives-core RAD-7 SEG-7 STP-8 RAT-6 TA-10 PIN-2 ICN-8; primitives-core SLD-9 SEL-9; primitives-core BTN-16 BTN-21 TGL-10; primitives-core TA-9; settings-stash S51; settings-stash F1"
  - id: P2
    story: "the navigation and container controls are DSL widgets: Sidebar and SplitView, TabBar, SubHeader, Accordion, Pagination, TreeView, PageControl with a paged view, the grouped List, GridTile, Table, Form with Section and LabeledContent"
    state: planned
    gate: "host: a test per entry, goldens light and dark, widgets.md entries compile"
    note: "moved from TASK-0353 (2026-11-10, the operator's \"move them, then close\"; ids of the design spec and the P5 transition inventory): primitives-window LI-13 SB-10 TB-10 SH-5 AC-8 PG-9 TV-8 PC-1 GR-2 TA-1 FS-1"
  - id: P3
    story: "a floating surface anchors to its trigger and goes on an outside press or Esc — menus, popovers, the select panel, suggestions, the split button, a context menu on a secondary click, a tooltip after a hover delay; a widget's parts take their own taps (crumbs, clear, reveal, cancel, tokens, dismiss); Enter submits a field"
    state: planned
    gate: "host: a test per surface and trigger; QEMU: a lane opens and dismisses an anchored menu"
    note: "moved from TASK-0353 (2026-11-10, the operator's \"move them, then close\"; ids of the design spec and the P5 transition inventory): primitives-core BTN-18 SEL-7 SB-8; primitives-window PO-1 TT-1 MN-12 BC-6 LI-10; shell-panels PN-FOCUS; launcher-lock C13; launcher-lock H1; launcher-lock L35; osk 19; settings-stash S8; settings-stash S23; settings-stash S38; settings-stash F14; settings-stash F29 F30 F31"
  - id: P4
    story: "the apps use them: Settings' search mode and sub-pages, Stash's view segment and crumbs, the window kit's rail, Chat's counts"
    state: planned
    gate: "the moved delta ids closed when the apps' audits are re-run"
    note: "moved from TASK-0353 (2026-11-10, the operator's \"move them, then close\"; ids of the design spec and the P5 transition inventory): primitives-window MN-11 AW-8; calculator C34 C36; calculator C35; chat H4; settings-stash S9 S37; settings-stash S24 S30"
---

<!-- `nx ledger new task` copies everything below this line; the frontmatter above shows the
     planning fields (RFC-0115). Write state with `nx ledger`, never by hand. -->

## The ask

Who asked, in their words, and the outcome they want.

## Measured before the cut

What the code and the system do today — numbers, log paths, the measurement folder. A plan
that names no measurement is a guess.

## Three lenses

- **Scope** — in: … · out: … (with where each "out" goes).
- **Invariant** — what must hold, and the `test_reject_*` that proves it.
- **Contract** — the RFC/ADR this implements or seeds.

## Not doing here

The scope firewall: what is deliberately left out, and which task or milestone step takes it.

## Definition of Done

- Every phase `done` with its evidence; the goal's target reached on the instrument.
- Host tests, the QEMU lanes and the board cycle the phases name are green.
- Security-relevant work has its `test_reject_*` and hardening markers
  (`docs/standards/SECURITY_STANDARDS.md`).
- Docs swept: CHANGELOG, affected docs, the RFC's status; `active_days` and, when the estimate
  missed, a `variance_note`.
