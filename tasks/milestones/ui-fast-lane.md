---
schema: 1
id: ui-fast-lane
title: "UI fast lane"
status: done
started: 2026-04-21
done: 2026-07-22
goal: "A real desktop in QEMU: login, launcher, pointer and keyboard, text, scroll, app windows and settings, the SystemUI written in the DSL — about 40 tasks instead of the 98 below 0122."
sections:
  - key: S1
    title: "Foundations below 0054"
  - key: S2
    title: "Visible UI and the input spine"
  - key: S3
    title: "UI content: text, layout, gestures, animation, windows, lifecycle"
  - key: S4
    title: "Shell infrastructure"
  - key: S5
    title: "The DSL foundation"
  - key: S6
    title: "SystemUI in the DSL"
steps:
  - key: S1.1
    section: S1
    story: "the supply chain, zero-copy VMOs, packagefs v2 and sandboxing carry the UI stack"
    state: done
    done: 2026-07-17
    tasks: [TASK-0029, TASK-0031, TASK-0032, TASK-0039]
  - key: S1.2
    section: S1
    story: "one CLI, configd and policy as code serve the UI services"
    state: done
    done: 2026-07-17
    tasks: [TASK-0045, TASK-0046, TASK-0047]
  - key: S2.1
    section: S2
    story: "a CPU renderer, the compositor and a visible first frame in QEMU"
    state: done
    done: 2026-07-17
    tasks: [TASK-0054, TASK-0055, TASK-0055B, TASK-0055C]
  - key: S2.2
    section: S2
    story: "double-buffered surfaces, cursor, hover, focus and click"
    state: done
    done: 2026-07-17
    tasks: [TASK-0056, TASK-0056B]
  - key: S2.3
    section: S2
    story: "live keyboard and pointer through hidrawd, touchd and inputd"
    state: done
    done: 2026-07-17
    tasks: [TASK-0252, TASK-0253, TASK-0056C]
  - key: S3.1
    section: S3
    story: "text shaping, layout, clip and scroll"
    state: done
    done: 2026-05-20
    tasks: [TASK-0057, TASK-0058, TASK-0059]
  - key: S3.2
    section: S3
    story: "typing lands in apps through imed and the on-screen keyboard"
    state: done
    done: 2026-07-22
    tasks: [TASK-0146, TASK-0147]
  - key: S3.3
    section: S3
    story: "gestures, animation, a virtualized list, window management, the app lifecycle, the greeter"
    state: done
    done: 2026-07-19
    tasks: [TASK-0061, TASK-0062, TASK-0063, TASK-0064, TASK-0065, TASK-0065B]
  - key: S4.1
    section: S4
    story: "window move and resize, settingsd and its panel, the design system"
    state: done
    done: 2026-07-19
    tasks: [TASK-0070, TASK-0072, TASK-0073]
  - key: S5.1
    section: S5
    story: "the DSL from lexer to interpreter, state, navigation and services"
    state: done
    done: 2026-07-19
    tasks: [TASK-0075, TASK-0076, TASK-0077, TASK-0078, TASK-0078B]
  - key: S5.2
    section: S5
    story: "the SystemUI bootstrap shell in the DSL, host and OS"
    state: done
    done: 2026-07-19
    tasks: [TASK-0080B, TASK-0080C]
  - key: S6.1
    section: S6
    story: "launcher, control center and settings as DSL pages, wired in the OS"
    state: done
    done: 2026-07-19
    tasks: [TASK-0119, TASK-0120, TASK-0121]
  - key: S6.2
    section: S6
    story: "notification wiring, the app platform and the integration kit"
    state: dropped
    tasks: [TASK-0122, TASK-0122B, TASK-0122C]
    gate: "not needed for the lane's goal; parked as drafts"
---

# Milestone: UI fast lane (2026-04-21 → 2026-07-22)

Instead of building every task from 0024 to 0118 in order, only the tasks the UI chain needed were
built, and everything else went to a defer bucket for Sub-80. The lane closed when typing landed
in apps and the SystemUI ran from DSL pages. Dates of the steps are the last done date of their
tasks, mostly reconstructed from git (2026-07-17/19 are the reconciliation commits).

The UX bar the lane met (the "Orbital-level" gate): a visible greeter or dev session, live pointer
and keyboard, cursor, hover, focus, click and scroll, a launcher, apps in windows with focus,
close and move, SVG icon sources, settings, no global input leaks, no marker-only claims. Every
UI capability had to appear on the shared visible proof surface of the real QEMU desktop.

Reconstructed on 2026-11-02 from `tasks/IMPLEMENTATION-ORDER.md` as of `b929e66d` (RFC-0115).
