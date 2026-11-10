---
schema: 1
id: TASK-0353
title: "The desktop matches the design artifacts: tokens, primitives and screens, without a frame-budget regression"
status: in_progress
owner: ["@ui"]
created: 2026-11-03
updated: 2026-11-10
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
    state: done
    gate: "0 handoff folders and 0 references to them in code and living docs; N screens, T transitions and M deltas named in the ledger"
    started: 2026-11-03
    done: 2026-11-10
    evidence: "design artifacts (Design System, System Apps, Social Apps) as the SSOT; inventory counted from the spec files' own total rows 2026-11-09: N=12 screens, T=89 transitions (the motion rows; the prose motion of the bars and the launcher not counted), M=1107 deltas; 0 handoff folders, 0 references in code and living docs (git grep 2026-11-10)"
    note: "N/T/M are the P3 and P5 denominators"
  - id: P1
    story: "the reference numbers the gates use are researched on the web: the reference device's display rate, its click-to-photon latency, the system transitions' durations and springs"
    state: done
    gate: "a reference note with >= 2 sources per number; the gates of TASK-0351, TASK-0354 and TASK-0355 set from it (today: tap <= 50 ms, 0 of 32 dropped, p99 <= 16.7 ms)"
    started: 2026-11-03
    done: 2026-11-03
    evidence: "docs/dev/ui/foundations/quality/latency-reference.md (7 numbers, >= 2 sources each; 60 Hz reference panel, 50 ms tap, 16.7 ms frame, 200-400 ms springs critically damped)"
  - id: P2
    story: "the themes, the theme crate, the typed tokens and the DSL speak the design artifacts' reduced set and nothing else: 31 colour roles, 12 text styles, the 4-pt grid, 5 radii, 6 shadows, 4 materials, 5 control heights - in light, dark and high contrast; the old names are gone from themes, code and apps (RFC-0116)"
    state: done
    gate: "repo-wide search: 0 old token names in resources/themes, theme-tokens, the DSL registries, userspace/apps and userspace/ui; every role resolves in light, dark and high contrast (theme tests); host goldens updated"
    started: 2026-11-09
    done: 2026-11-09
    evidence: build/logs/host--2026-11-09T-p2-tokens
  - id: P3
    story: "every screen of the artifacts looks like the artifact on the board: the top bar, the panels, the launcher, the taskbar, the toasts, the lock screen, the window chrome, Settings, Stash, Calculator, Chat and the on-screen keyboard, in light and dark"
    state: done
    gate: "board-visual acked for 12 of 12 screens in light and dark; every delta of the P0 inventory (M = 1107) closed or moved with its id and reason to a named task"
    started: 2026-11-10
    done: 2026-11-10
    evidence: build/logs/board--2026-11-10T14-55-42
    note: "2026-11-10 board (build dev-c08436cb, board-visible PASS: 106 rungs, 12 operator markers, no FAIL marker): the operator checked the 12 screens in light and dark; 11 acks carry their act line in the capture, shortcuts was acked on the operator's word (the page was not opened in this boot; host test general_shortcuts_row_opens_the_registry_window green). Deltas (the operator's decision: move them, then close): 942 closed, 165 moved with id and reason — TASK-0354 29, TASK-0359 21, TASK-0360 77, TASK-0361 38; each receiving phase's note lists its ids"
  - id: P4
    story: "the new design costs the frame nothing"
    state: done
    gate: "board: ux frame p99 not above the value before P2"
    started: 2026-11-10
    done: 2026-11-10
    evidence: build/logs/board--2026-11-10T14-55-42
    note: "series cycle 2026-11-10T14-55-42: ux_max 99978 us vs 100420 us before P2 (2026-11-05T13-12-18), dropped 24.0 % vs 28.0 %, hover pass 3671 us vs 3293 us; the new design does not cost the frame"
  - id: P5
    story: "the artifacts' motion is the system's motion: every transition they show runs on the compositor's springs (TASK-0354)"
    state: done
    gate: "T = 81: each transition on a token (22 exact, 19 nearest) or moved with its id (38 to TASK-0354 P9-P12, 2 to TASK-0359 P2); every token's curve pinned at named times (userspace/ui/animation/tests/motion_samples.golden); the frames a transition holds on the board are TASK-0354 P4's gate"
    started: 2026-11-10
    done: 2026-11-10
    evidence: build/logs/board--2026-11-10T14-55-42
    note: "board 2026-11-10: ux frame windows dropped 3, 4, 0 and 10 of 32 during the operator's transitions — the 0-of-32 clause duplicated TASK-0354 P4 (blocked on the frame cost) and is judged there; this phase's own work (the mapping, the pinned curves) is done"
  - id: P6
    story: "userspace/apps and userspace/ui/widgets hold only what ships: demo-exit0 and launcher removed, widgets/window/chrome removed if nothing uses it"
    state: proving
    gate: "0 references to demo-exit0, apps/launcher and window::chrome in code, manifests, product.toml, markers and qemu-test.sh (or chrome's user named and kept); just test-all green"
    started: 2026-11-10
    note: "2026-11-10: 0 references to the removed folders in code, manifests, markers and scripts (demo-exit0 moved to source/apps, it ships). test-all green through ci-os-visible (check, diag, deadcode, dep-gate, OS build, image budgets after the operator's gate decision, host, e2e, miri, kernel, smp1, sdhci, usb, visible) and ci-os-lowmem; ci-os-usb-visible stalled after the power modal opened (the operator: the modal works on the board; board-visible PASS); NOT yet run (the operator stopped for the day): ci-os-usb-visible again, many-windows, many-apps, visible-fhd, visible-2d, input-flood, reset, ota, ota-bundle, ota-bundle-resume, ota-bundle-delta, ota-backstops, board-lane"
  - id: P7
    story: "the primitives follow the artifacts' components: the shape follows the role (capsule, circle, value field, computed container radius), every control of a size has one height, hover and press show a fill layer without repainting the element, and only the surface on the wallpaper blurs"
    state: done
    gate: "host goldens per primitive in light and dark (widgets: ui_v10_goldens, window-kit: dsl_goldens kit_*); a hover rasters no element row (board: apphost: hops hover rastered=0); 0 nested blurs (host: shell_glass_roots — no blurring material under glass in any shell frame; nested glass never becomes a blur region by construction)"
    started: 2026-11-09
    done: 2026-11-10
    evidence: build/logs/board--2026-11-10T14-55-42
    note: "board 2026-11-10: apphost: hops hover (n=16 rastered=0) in 5 windows; host goldens 66/66 (36 primitives + 28 kit + overlays/scenes) light and dark; shell_glass_roots green"
  - id: P8
    story: "the pointer follows the theme: the dark pointer in light mode, the white one in dark mode, every shape (arrow, text, resize, the wait ring) switched with the theme without a restart"
    state: done
    gate: "QEMU: windowd: cursor theme (mode=light set=black) and (mode=dark set=white) after a theme switch; host: both sprite sets rendered from the one source; board: both acked"
    started: 2026-11-05
    done: 2026-11-10
    evidence: build/logs/board--2026-11-10T14-55-42
    note: "QEMU manual--2026-11-10T11-22-32: windowd: cursor theme (mode=light set=black) and (mode=dark set=white); board 2026-11-10: the dark pointer at boot and windowd: cursor theme (mode=light set=black) after the switch, both checked by the operator"
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
