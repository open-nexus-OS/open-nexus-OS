---
schema: 1
id: sub-80
title: "Sub-80: every task below 0080 done"
status: done
started: 2026-09-03
done: 2026-09-21
goal: "Every open task below 0080 done at its end state or closed by decision with the actual solution recorded; the network family on hold."
sections:
  - key: P1
    title: "Phase 1: below 0054, without network"
  - key: REC
    title: "Reconciliation against the code"
  - key: P2
    title: "Phase 2: 0054 to 0079"
steps:
  - key: P1.1
    section: P1
    story: "a verified system volume carries the services and apps, not the boot image"
    state: done
    done: 2026-09-04
    tasks: [TASK-0321]
  - key: P1.2
    section: P1
    story: "bundle-set deltas with a resumable stage journal"
    state: done
    done: 2026-09-05
    tasks: [TASK-0035]
  - key: P1.3
    section: P1
    story: "policy profile v2: argument matchers, learn and enforce"
    state: done
    done: 2026-09-05
    tasks: [TASK-0028]
  - key: P1.4
    section: P1
    story: "statefs quotas, netstackd identity and the egress seam, one deny taxonomy"
    state: done
    done: 2026-09-07
    tasks: [TASK-0043]
  - key: P1.5
    section: P1
    story: "inbound default-deny: services are exposed only through the ingress gateway"
    state: done
    done: 2026-09-08
    tasks: [TASK-0052]
  - key: R1
    section: REC
    story: "eight ledgers closed against the code with their actual solution"
    state: done
    done: 2026-09-09
    tasks: [TASK-0011B, TASK-0037, TASK-0041, TASK-0054B, TASK-0054D, TASK-0076B, TASK-0050B, TASK-0079]
  - key: R2
    section: REC
    story: "the display handoff is deterministic by construction: build truth, one wiring, readiness barriers"
    state: done
    done: 2026-09-15
    tasks: [TASK-0324]
  - key: P2.1
    section: P2
    story: "one trap per side for an IPC call, a direct handoff"
    state: done
    done: 2026-09-18
    tasks: [TASK-0054C]
  - key: P2.2
    section: P2
    story: "pkg:/ reads above 8 KiB as a pass-through"
    state: done
    done: 2026-09-18
    tasks: [TASK-0033]
  - key: P2.3
    section: P2
    story: "gpud decides GL when it is first needed"
    state: done
    done: 2026-09-19
    tasks: [TASK-0326]
  - key: P2.4
    section: P2
    story: "DSL ergonomics and a runtime that frees by size class"
    state: done
    done: 2026-09-21
    tasks: [TASK-0077B, TASK-0077C]
  - key: P2.5
    section: P2
    story: "modals, tiling, the clipboard and the screenshot"
    state: dropped
    tasks: [TASK-0074, TASK-0066, TASK-0067, TASK-0067B, TASK-0068]
    gate: "moved to hardware-network B3 on 2026-09-21 and done there by 2026-10-09"
---

# Milestone: Sub-80 (2026-09-03 → 2026-09-21)

The defer bucket of the UI fast lane, rebuilt by the operator on 2026-09-03: Phase 1 = every open
task below 0054 without network scope, Phase 2 = 0054–0079. Prerequisites were pulled into the
lane (0321 before 0035), never bypassed; the network family (0024, 0030, 0038, 0040) stayed on
hold for a joint discussion. Each task was reviewed against the code and its philosophy docs,
its ledger rewritten to the end state, built, proven green and pushed before the next was
planned.

The last five tasks ran as Block 3 of the hardware and network fast track once the board had a
picture and USB input (operator, 2026-09-29); the mission "every task below 0080 done" closed
there on 2026-10-09.

Reconstructed on 2026-11-02 from `tasks/IMPLEMENTATION-ORDER.md` as of `b929e66d` and `9e2626f7`
(RFC-0115).
