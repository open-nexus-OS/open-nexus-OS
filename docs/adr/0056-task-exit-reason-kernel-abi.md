# ADR-0056: The kernel records WHY a task exited and exposes it through wait — exit code alone is not failure truth

- Status: Accepted
- Date: 2026-08-18
- Links:
  - Tasks: `tasks/TASK-0049-fault-exhaustion-truth-proof-reanimation.md` (execution + proof)
  - RFCs: `docs/rfcs/RFC-0087-reliability-failure-model-v1.md` (reason taxonomy),
    `docs/rfcs/RFC-0081-process-reaper-nonblocking-reclaim.md` (wait_nohang path),
    `docs/rfcs/RFC-0011-logd-journal-crash-v1.md` (crash envelope consumes `reason`)
  - Related ADRs: `docs/adr/0054-map-errors-keep-their-identity-across-the-abi.md`
    (same principle: identity survives the ABI)

## Context

Today a userspace fault ends in `exit_current_and_release(tasks, -22)`
(`source/kernel/neuron/src/core/trap/handler.rs`): a page fault, an illegal
instruction, a voluntary `exit(-22)`, and a policy kill are indistinguishable at
the observer. Supervision (restart? crash loop? clean stop?), crash retention
(is this a crash at all?), and honest post-mortems all require the cause, and
today they would all have to guess from the exit code. The trap ring
(`core/trap/fault.rs`) already holds the cause — it just never crosses the ABI.

## Decision

- The kernel records an **exit reason** in the TCB at the moment of death:
  `clean` (exit 0) · `error` (voluntary exit ≠ 0) · `fault{cause}` (trap-kill,
  cause from the existing trap ring — reused, not duplicated) · `kill{by}`
  (killed by an authority). A `panic` marker is set by the userspace runtime
  before its abort exit and carried as a sub-flag of `error` — the kernel does
  not parse panic payloads.
- `sys_wait` and `sys_wait_nohang` return the reason alongside `(pid, status)`;
  `nexus-abi` exposes it as a typed enum. Per ADR-0054, no wildcard collapsing:
  every kernel-side reason maps to exactly one ABI variant.
- The reason is **kernel truth**: userspace cannot set or overwrite it (except
  the pre-declared panic sub-flag, which can only *add* panic to a voluntary
  exit, never mask a fault).
- Out of scope: register/memory capture for uncontrolled crashes (RFC-0031
  limitation stands), signal delivery semantics, core dumps.

## Consequences

- **Positive**: crash-loop policy counts real crashes (a `clean` exit never
  trips it); crash evidence gains a trustworthy `reason` field; the
  `exit(-22)` ambiguity disappears; execd's crash reporting stops inferring.
- **Negative / accepted cost**: touches two approval zones (`source/kernel/**`,
  `source/libs/nexus-abi`); wait-ABI change ripples through execd and the
  selftest markers (one coordinated task, TASK-0049).
- **Follow-ups**: TASK-0049 (execution), RFC-0011 envelope gains `reason`
  (compatible extension — the envelope reserved extensibility).

## Alternatives considered

- **Heuristic in userspace** ("exit ≠ 0 = crash"): rejected — supervision and
  retention built on a guess is the interim solution this lane forbids, and the
  rebuild would land in the same approval zones anyway.
- **Full fault frames to userspace** (registers, stack): rejected for v1 —
  disproportionate kernel surface; RFC-0031 keeps in-process capture.
- **Reason via logd side channel instead of wait ABI**: rejected — evidence must
  not be the source of truth for supervision decisions (RAM ring, droppable).
