---
title: TRACK Time as a Conserved Resource (derivable time/budget rights)
status: Draft
owner: @kernel-team @runtime
created: 2026-08-18
links:
  - Reliability contract: docs/rfcs/RFC-0087-reliability-failure-model-v1.md
  - Restart derivation: docs/adr/0057-service-restart-capability-re-resolve.md
  - SMP/EDT direction: docs/adr/0052-per-hart-earliest-deadline-timer-and-affinity-respecting-steal.md
  - Lockclass right-of-way: docs/adr/0049-bkl-lockclass-and-softrt-cpu-placement.md
  - Kernel accounting truth: tasks/TASK-0286-kernel-memory-accounting-v1-rss-pressure-snapshots.md
  - Kernel pressure/OOM: tasks/TASK-0287-kernel-memory-pressure-v1-hard-limits-oom-handoff.md
---

## Purpose

Vision track for making CPU time (and, generally, runtime budgets) a **conserved,
derivable right** along the init capability tree — instead of a QoS *label* that
any service can claim. Inspired by S3K's capability-controlled temporal
partitioning (RTSS 2025; correspondence with R. Guanciale, 2026-08), adopting the
conservation principle while explicitly **rejecting the static slice mechanism**.

This track spawns **no tasks today**. RFC-0087 already encodes the three
principles this track exists to eventually make structural:

1. **Restart = derivation** (no rights drift) — contractual now, cap-backed later.
2. **Exhaustion is an event** — contractual now; a budget right gives it a
   *constructive* trigger later.
3. **The supervision path has right-of-way** — QoS/lockclass *policy* now
   (ADR-0049/0052); a conserved time right would make it *impossible* to starve,
   not just unlikely.

## Core idea (what "conserved" buys over labels)

A QoS class is an assertion; nothing prevents everything from being "high" — and
then nothing is. A derived right is conserved: whoever holds a budget can only
delegate a *subset* of it, so the sum of all time rights can never exceed the
root. `init` holds the root, delegates reservations to the display path and
app-host; app-host can hand apps only what it holds. "An app steals compositor
frame time" becomes impossible by construction, not improbable by policy — and
"who holds how much" becomes a tree query. Structurally this fits the existing
init ctrl-plane, which already mints slots and routes along a tree: a budget
right is one more entry, not a new axis.

Revocation is the recovery primitive: a runaway or wedged service *loses* its
time right through a kernel action, without its cooperation. That requires
**bounded revoke** over the derivation subtree — S3K's own data-structure
contribution, and the honest reason this track is gated (see G1).

## Gates (all RED today — do not extract tasks before these clear)

- **G1 — Bounded kernel ops / bounded revoke.** A time right's guarantee is void
  while kernel paths serialize behind the BKL with measured (not constructive)
  bounds. The ~6 ms BKL ceiling is a measurement, not a property. Revoke over a
  derivation subtree must itself be bounded, or the recovery action just
  relocates the outage. Status: RED — BKL still stands (ADR-0049 bounds it by
  policy only).
- **G2 — Accounting truth.** Conserved budgets need trusted accounting
  (TASK-0286 RSS/pressure truth, TASK-0287 enforcement). A budget the kernel
  cannot meter is a label with extra steps. Status: RED — both Draft.
- **G3 — hmv2 SMP direction decided.** The slice-table model conflicts with
  per-hart EDT + affinity-respecting steal (ADR-0052) and dynamic consumer-UI
  frame loads. Before any mechanism work, decide the open design question below
  for the hmv2 SMP implementation. Status: RED — not designed.

## Open design question (record, do not decide here)

**Static slices vs. dynamic contingent.** S3K's `(hart, [from, to))` slices give
lookup-table scheduling and state-readable isolation domains — at the price of
static allocation, which our UI load profile cannot pay. The candidate adaptation
for hmv2 is a **derivable rate/share** (budget per period, conserved under
delegation, revocable as a subtree) enforced by the EDT scheduler rather than a
slot table. Whether conservation survives contact with work-stealing and
dynamic reallocation across periods — and what reconfiguration then costs — is
the research question. Note honestly: S3K's headline benefits ("isolation domains
readable from state, no reconfiguration cost") hold in full only for the static
table we are rejecting; a dynamic variant must re-earn them or explicitly waive
them.

## Candidate work (extract only when gates clear)

- CAND-TIME-000: budget-right object model + derivation/revoke semantics (RFC).
- CAND-TIME-010: supervision-path reservation as the first consumer
  (RFC-0087 §2 upgrade: right-of-way policy → conserved right).
- CAND-TIME-020: display-path reservation (init → windowd/gpud delegation).
- CAND-TIME-030: exhaustion→event wiring from budget expiry (constructive
  timeout exception to the supervisor).

## Non-goals

- Adopting slice-table scheduling or replacing EDT.
- Any task extraction before G1–G3 clear.
- Treating this track as a prerequisite for the reliability lane — it is
  explicitly an *enabler after* detection (TASK-0049) and supervision
  (TASK-0049B), per the ordering argument recorded in RFC-0087.

## References

- S3K: capability-controlled temporal & spatial partitioning, RTSS 2025.
- Correspondence: R. Guanciale → J. Schäfer, 2026-08 (time as first-class
  capability judged worth the complexity; new capability data structure driven
  by bounded-revoke timing requirements; isolation domains identifiable from
  system state; author-side endorsement — treat as informed opinion, not
  independent evidence).
