<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# RFC-0108: The UX latency contract — "instant" and "smooth" as numbers every lane judges

- Status: In Progress
- Owners: @ui @runtime
- Created: 2026-10-20
- Last Updated: 2026-10-20
- Links:
  - Tasks: `tasks/TASK-0334-ux-latency-contract-budgets-measurement-lines-gates.md` (execution + proof)
  - SSOT: `source/libs/nexus-ux-budgets/src/lib.rs` (the budgets, the verdict rule)
  - Related RFCs: `docs/rfcs/RFC-0096-ipc-performance-contract-v2-call-reply-recv-fastpath.md`
    (the IPC budget), `docs/rfcs/RFC-0101-…` (the GPU driver's frame), `docs/rfcs/RFC-0100-…`
    (layer 6: the launch under 200 ms warm)
  - Related tracks: `tasks/TRACK-TIME-AS-RESOURCE.md` (time as a conserved right — the display
    path's reservation is this contract's guarantee, later), `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
    (the consumer-floor release)

## Status at a Glance

- **Phase 0 (the contract: budgets SSOT, the line format, three lines on the board ladder)**: ✅ 2026-10-20 (board cycle 49, TASK-0334: every line red by name — the first honest reading)
- **Phase 1 (the input carries its own stamp from the device; the tap's feedback judged)**: ⬜
  (with the non-parking compositor's input contract)
- **Phase 2 (a UI alarm: the shell's status pill yellow/red from the same numbers via metricsd)**: ⬜
- **Phase 3 (the budgets green on the board with glass: the consumer floor)**: ⬜ (packages:
  the non-parking compositor, the frame cost with glass, the launch without a copy)

"Complete" means every budget line is `ok` on the board lane with the operator's series.

## Scope boundaries (anti-drift)

- **This RFC owns**: the budgets (one SSOT crate), the measurement-line format and its verdict
  words, which lines each lane requires, how an emulated lane's timing is treated.
- **This RFC does NOT own**: the mechanisms that meet the budgets (the compositor's delivery
  contract, gpud's frame cost, the launch path), the input wire format, metricsd's schema.

### Relationship to tasks (single execution truth)

- TASK-0334 proves Phase 0; the packages named in TASK-0333 ("next, in this order") prove
  Phase 3 against these lines.

## Context

Cycle 47 on the desk board (2026-10-20) was green on every gate and felt like a slideshow,
then froze twice. The numbers existed — `gpud: gpu frame us (… avg=69677 … wait 66394)`,
`apphost: start t_us` to `WINDOWD: surface presented` 6.7 s for the settings app — but no line
judged them, so no run alarmed. The operator's demand: the board must feel like the reference
class (instant open, 60 Hz smooth with glass), and no run may forget that goal. A gate that
measures correctness alone cannot carry it; a budget with a verdict can.

## Goals

- One SSOT of what "instant" and "smooth" mean, read by the services, never re-typed.
- One line shape with one verdict rule, so a reader (or a script) judges any run the same way.
- The budgets on the board ladder from the first package on, red named by the allow-list
  with its tracking reference — never hidden.

## Non-Goals

- Meeting the budgets in this RFC (the packages do).
- Hard real-time guarantees (the display path's reservation is `TRACK-TIME-AS-RESOURCE`'s).

## Constraints / invariants (hard requirements)

- **No fake success**: `ok` only when the measured value is inside the budget; the verdict is
  the host-tested `nexus_ux_budgets::judge`, never a service's own comparison.
- **Determinism**: a budget is a number, the same on every lane; what differs per lane is only
  which `FAIL` lines the allow-list names (an emulated lane's timing is not the contract).
- **Bounded**: windows of fixed size (32 frames, 16 inputs); one line per window.
- **No identity on the line**: a measurement names a slot or a window, never a user's input.

## Proposed design

### The budgets (normative; the SSOT is the crate)

| Budget | ok | warn | FAIL | Why this number |
|---|---|---|---|---|
| A frame | ≤ 16 667 µs | 1 dropped frame in a 32-frame window | ≥ 4 dropped | a 60 Hz panel's period; one drop is a visible hitch, four a slideshow |
| An input's wait in the compositor (to the frame's submit) | ≤ 50 ms | > 50 ms | > 100 ms | the reference class shows a pointer move in 2–3 frames; past 100 ms a user feels lag |
| A tap's visible feedback | ≤ 100 ms | — | > 100 ms | the response-time bound users read as "immediate" (Phase 1) |
| A launch to its first frame | ≤ 300 ms | > 300 ms | > 1 000 ms | the plan's warm restart under 200 ms (6e) plus margin; past a second a user doubts the tap |

### The line (normative)

```
<service>: ux <name> <ok|warn|FAIL> (<measure>=<value> … budget_<unit>=<budget>)
```

- `<service>` is the emitter (`gpud`, `windowd`, `apphost`); `<name>` the budget (`frame`,
  `input`, `launch`, later `tap`); the verdict word is the crate's (`FAIL` in capitals so the
  no-fake-green scan finds it); the fields name the measure, the window and the budget.
- Phase 0 lines: `gpud: ux frame … (dropped= of=32 max_us= budget_us=16667)`,
  `windowd: ux input … (to_submit_us= of=16 budget_us=50000)`,
  `apphost: ux launch … (first_frame_ms= budget_ms=300)`.
- The ladders require the line's PREFIX (`gpud: ux frame `) — presence is the rung; the verdict
  is read by the no-fake-green scan: a `FAIL` is red unless the lane's allow-list names it with
  its tracking reference (`config/fail-marker-allow*.txt`).
- An emulated lane (QEMU, TCG) never meets a timing budget; its `FAIL` lines are allow-listed
  as such. The board's are allow-listed only while a package tracks them.

### Proof gates

- Host: the verdict rule's truth table and `test_reject_*` (`nexus-ux-budgets`), the input
  window (`windowd::ux_input`).
- QEMU `headless`/`visible`: `apphost: ux launch ` present (the greeter's first frame).
- Board `board-visible`: all three lines present; the verdicts read in the measurement folder
  of each package.

## Open questions

- Whether the frame window should also carry the p99 over a longer horizon (a histogram in
  gpud) — decided when the frame cost package lands.
- Whether the launch budget splits warm/cold by a flag from execd (a shared image present or
  not) — decided in 6c.
