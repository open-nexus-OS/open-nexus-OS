---
schema: 1
id: TASK-0352
title: "The memory model used: no fixed process tables, an owner's footprint includes its surfaces, limits per app"
status: draft
owner: ["@kernel-team", "@runtime"]
created: 2026-11-03
subsystem: kernel
components: [execd, abilitymgr, memd, gpud]
kind: service
novelty: variant
estimate: 2
goal:
  metric: "apps alive at once; apps whose surfaces are charged to them"
  baseline: "32 (execd tracked children, abilitymgr instances, memd banded processes); 8 GPU charge clients, a ninth app's surfaces go uncharged (TASK-0348 'Not doing here')"
  target: "a 33rd app launches; every app's surfaces charged; an app over its limit refused by name while the others run"
  instrument: "execd/abilitymgr launch lines · memd footprint per app (Part::Surfaces)"
consumer: TASK-0351/P4
depends_on: [TASK-0348]
links:
  - label: "Scope from"
    target: "TASK-0348 'Not doing here' (the old number TASK-0349 there means this task)"
  - label: Contract
    target: "RFC-0100 (memory object model v2)"
phases:
  - id: P0
    story: "every table that bounds the number of apps is listed with its size, and the 33rd app's refusal is logged"
    state: planned
    gate: "QEMU: the refusal of app 33 logged; the list of fixed tables in the ledger"
  - id: P1
    story: "execd, abilitymgr and memd grow with the apps"
    state: planned
    gate: "QEMU: 40 app instances launched, 0 refusals"
  - id: P2
    story: "every app's surfaces count in its footprint"
    state: planned
    gate: "QEMU: app 9..40 each with Part::Surfaces > 0 in memd's footprint"
  - id: P3
    story: "an app past its limit is refused by name and the others keep running"
    state: planned
    gate: "host test_reject_* for the limit; QEMU: the app over its limit gets ENOMEM or a recorded kill, 0 other apps affected"
  - id: P4
    story: "the same on the board"
    state: planned
    gate: "board: a thirty-third app launches; a ninth app's surfaces are charged"
---

## The ask

## Measured before the cut

## Three lenses

## Not doing here

## Definition of Done
