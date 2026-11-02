---
name: plan-package
description: The planning ritual of RFC-0115 for a new task, package or milestone step — a goal with a number, a named consumer, an estimate from our own history, phases as user stories with measurable gates — written through `nx ledger`, not prose. Use before starting any task, when cutting phases, and whenever a cost or scope objection comes up.
---

# Plan a package (RFC-0115)

The two expensive mistakes this prevents: **a week of work behind a gate without a number** (the
GPU driver ran a week before a wrong flag was found — its gate was "a picture", not "frame time
below the stock system") and **a mechanism without a consumer** (a week of memory work unused
because apps had hard-coded sizes).

## The ritual (in order, each step is a command)

1. **Orient.** `nx ledger brief`, then `nx ledger show <milestone> <TASK>`. Read the ledger body
   only for the decisions you need.
2. **Measure first.** Name the baseline with its instrument and where it was taken. No
   measurement → phase P0 is the measurement.
3. **Goal with a number.**
   `nx ledger goal TASK-N "<metric>" "<baseline>" "<target>" "<instrument>"`
4. **Name the consumer.** `nx ledger set TASK-N consumer "<who uses it>"`. If the consumer does
   not exist yet, it is part of this package or of a step in the same milestone — otherwise the
   mechanism is not planned.
5. **Estimate from history, never from the textbook.** Reference class = `subsystem` + `kind` +
   `novelty` (`nx ledger set TASK-N kind service`, `… novelty variant`). Look up comparable done
   tasks (`nx ledger list --kind task --subsystem <s> --status done`, their `active_days`) or ask
   Section 9 for P50/P85 with the sample size. `nx ledger set TASK-N estimate <days>`.
   **A cost objection ("this takes months") without a reference class from our data is not an
   objection** — this project's GPU driver took a week; production-grade is often an hour more.
6. **Phases as user stories.** One sentence of behaviour per phase, and a gate that names the
   value it must reach:
   `nx ledger phase TASK-N P1 --story "…" --gate "<marker or command> <value>"`
7. **Three lenses** (`architecture-review`) for anything structural; its three lines go into the
   ledger body.
8. **Place it.** `nx ledger step <milestone> <KEY> --tasks TASK-N`, and this week's sprint:
   `nx ledger plan TASK-N P1,P2 --p50 … --p85 … --sample-size …`.

## While building

- `nx ledger phase TASK-N P1 --state building` when you start (it starts the task),
  `--state proving` when built and waiting for its lane or board cycle,
  `--state done --evidence build/logs/<run>` when the gate is green.
- A finding that changes the plan becomes a new phase or a new task now, not a paragraph.
- Progress reports at the end of a run: `nx ledger table` — counted, never estimated.

## Closing

Every phase `done`/`dropped`; `nx ledger set TASK-N active_days <n>`; if the estimate missed,
`nx ledger set TASK-N variance_note "<why>"` — the next estimate learns from it; then the docs
sweep (CHANGELOG, docs, the RFC's status) and `nx ledger set TASK-N status done`.

## Ownership

A task `in_progress` belongs to the building session. Drafts, future tasks, milestones and
sprints may be changed in parallel by the planning conversation (Section 9); re-read
`nx ledger show` before writing to a planning document you did not create in this session.
