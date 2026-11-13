---
schema: 1
id: TASK-0372
title: "A control transfer that fails on the wire is sent again: xhcid retries a transaction error before it reports a failure"
status: draft
owner: ["@runtime"]
created: 2026-11-12
updated: 2026-11-12
subsystem: hardware
goal:
  metric: "board boots with xhcid: FAIL (step=control cc=4)"
  baseline: "2 of 15 board cycles (board--2026-11-11T10-41-29, board--2026-11-12T15-31-27); the device worked both times"
  target: "0 of 10 board cycles; a request that fails three times still reports its code"
  instrument: "FAIL gate of scripts/board-test.sh over ten cycles; host test of the control pipe"
phases:
  - id: P1
    story: "a control request whose transfer fails on the wire (transaction error) is sent again after EP0's recovery, up to three times; only a request that keeps failing, or fails for another reason, is reported"
    state: planned
    gate: "host: a_transaction_error_is_retried_then_succeeds, test_reject_retry_beyond_three, a stall is reported at once; board: 0 xhcid: FAIL (step=control cc=4) in ten cycles"
    note: "found 2026-11-12 on the board cycle of TASK-0362 P1: pipe.rs recovers EP0 (Reset Endpoint, Set TR Dequeue) and then reports the code — the request is not sent again. The line makes the board judge red although keyboard and mouse work."
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
