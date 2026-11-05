---
schema: 1
id: TASK-0353
title: "The desktop matches the design artifacts: tokens, primitives and screens, without a frame-budget regression"
status: in_progress
owner: ["@ui"]
created: 2026-11-03
updated: 2026-11-05
subsystem: ui
components: [desktop-shell, app-host, windowd, window-kit, widgets]
kind: ui
novelty: variant
estimate: 3.5
started: 2026-11-03
dates_source: recorded
goal:
  metric: "screens of the artifacts that match on the board; design deltas left (tokens, sizes, radii, spacing, motion); frames dropped per transition; dead folders left in userspace/apps and userspace/ui/widgets"
  baseline: "0 screens matched; deltas counted in P0; reference numbers set, not researched; demo-exit0 and launcher still in userspace/apps, widgets/window/chrome without a known user since TASK-0337"
  target: "every artifact screen matched (fixed sizes and radii exact, padding/gap on the 4px grid, as TASK-0312/0313); every artifact transition on a motion token; 0 deltas left; 0 of 32 frames dropped per transition; frame p99 not above today's; every reference number with >= 2 sources; 0 references left to the removed folders"
  instrument: "board-visual ack per screen · host goldens · ux frame p99 line · reference note with sources · repo-wide reference search · just test-all"
consumer: hardware-network/LU.1
depends_on: [TASK-0354, TASK-0355]
links:
  - label: Source
    target: "the design artifacts from the operator (2026-11-03), captured into the repo in P0"
  - label: Pattern
    target: "TASK-0312, TASK-0313 (pixel-fidelity contract of the earlier handoffs)"
  - label: Budgets
    target: "RFC-0108 (the numbers P1 confirms or replaces)"
phases:
  - id: P0
    story: "the artifacts are the design contract: the SSOT outside the repo, the repository's handoff copies removed, every screen and every transition listed and each delta to today's build counted"
    state: building
    gate: "0 handoff folders and 0 references to them in code and living docs; N screens, T transitions and M deltas named in the ledger"
    started: 2026-11-03
  - id: P1
    story: "the reference numbers the gates use are researched on the web: the reference device's display rate, its click-to-photon latency, the system transitions' durations and springs"
    state: done
    gate: "a reference note with >= 2 sources per number; the gates of TASK-0351, TASK-0354 and TASK-0355 set from it (today: tap <= 50 ms, 0 of 32 dropped, p99 <= 16.7 ms)"
    started: 2026-11-03
    done: 2026-11-03
    evidence: "docs/dev/ui/foundations/quality/latency-reference.md (7 numbers, >= 2 sources each; 60 Hz reference panel, 50 ms tap, 16.7 ms frame, 200-400 ms springs critically damped)"
  - id: P2
    story: "tokens and primitives carry the new design (colour, type, radii, spacing)"
    state: planned
    gate: "host goldens updated; token deltas 0 of M"
  - id: P3
    story: "every screen of the artifacts looks like the artifact on the board"
    state: planned
    gate: "board-visual acked for N of N screens; deltas 0 of M"
  - id: P4
    story: "the new design costs the frame nothing"
    state: planned
    gate: "board: ux frame p99 not above the value before P2"
  - id: P5
    story: "the artifacts' motion is the system's motion: every transition they show runs on the compositor's springs (TASK-0354)"
    state: planned
    gate: "T of T transitions mapped to a token, host goldens sampled at named times; board: 0 of 32 frames dropped per transition"
  - id: P6
    story: "userspace/apps and userspace/ui/widgets hold only what ships: demo-exit0 and launcher removed, widgets/window/chrome removed if nothing uses it"
    state: planned
    gate: "0 references to demo-exit0, apps/launcher and window::chrome in code, manifests, product.toml, markers and qemu-test.sh (or chrome's user named and kept); just test-all green"
---

## The ask

Operator, 2026-11-03: the desktop looks like the design artifacts — tokens, primitives, every
screen and every transition — on the board, without the frame budget giving way; and the gates'
numbers come from research, not from habit. P6 (the dead folders) stays here by the operator's
choice (2026-11-03).

## Measured before the cut

- P1 done 2026-11-03: `docs/dev/ui/foundations/quality/latency-reference.md` — the reference
  laptop's panel is 60 Hz fixed, so 16.7 ms p99 and 0 of 32 dropped stay the frame gate; tap to
  first changed frame 50 ms (perception threshold about 60 ms); transitions 200–400 ms springs,
  critically damped by default, micro-motion under 200 ms.
- P0 counts the deltas against today's build (TASK-0312/0313 proved pixel fidelity to the earlier
  handoffs; the artifacts replace them as the contract).

## Three lenses (architecture-review, 2026-11-03)

- **Scope** — the design tokens and primitives (`userspace/ui/widgets`, `userspace/apps/window-kit`,
  the shell's DSL), the screens the artifacts show, their transitions mapped to motion tokens
  (TASK-0354 supplies the compositor motion; this task supplies the mapping), the goldens; P6 removes
  `userspace/apps/demo-exit0`, `userspace/apps/launcher` and `widgets/window/chrome` if nothing uses
  them. Out: windowd (window UI is a widget, never windowd — ADR-0076, RFC-0067); the motion
  mechanics (TASK-0354); the top bar's and launcher's latency (TASK-0355); the GPU's pixel cost
  (TASK-0351).
- **Invariant** — pixel fidelity as TASK-0312/0313 define it (fixed sizes and radii exact, padding
  and gap on the 4 px grid); the frame budget does not give way (P4: `gpud: ux frame` p99 not above
  the value before P2); no screen is acked without the board's picture; a removed folder leaves 0
  references (manifests, `product.toml`, markers, `qemu-test.sh`). Negative test: the goldens
  (a token delta fails its golden); P6: the reference search is the gate, `just test-all` green.
- **Contract** — RFC-0108 (the budgets, with the numbers of `latency-reference.md`); the design
  handoff pattern of TASK-0312/0313; RFC-0109/ADR-0076 for what a screen may declare as layers; no
  wire or ABI change here — anything the artifacts need beyond the layer model goes to TASK-0354.

## Not doing here

- The motion mechanism (TASK-0354), the latency cuts (TASK-0355), the GPU work (TASK-0351), a new
  widget framework (the kit stays).

## Definition of Done

Every phase done or dropped; `goal.result` with N of N screens acked, 0 of M deltas, the frame
p99 before and after; the reference note linked from RFC-0108; `nx ledger stale` empty.
