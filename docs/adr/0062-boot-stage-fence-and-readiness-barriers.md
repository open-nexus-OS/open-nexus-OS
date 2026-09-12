<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0062: init synchronizes boot ONLY through `@ready` and one kernel stage fence — never through yields, resume order or time caps

- Status: Accepted
- Date: 2026-09-09
- Links:
  - Tasks: `tasks/TASK-0324-display-handoff-deterministic-by-construction.md` (execution + proof, P2/P3/P5)
  - RFCs: `docs/rfcs/RFC-0093-display-handoff-and-boot-stage-contract.md` (the contract this decision fixes), `docs/rfcs/RFC-0069-init-declarative-service-manifest-slot-discipline-boot-stages.md` (§4 boot stages), `docs/rfcs/RFC-0013-boot-gates-readiness-spawn-resource-v1.md` (init markers A1–A4)
  - Related ADRs: `docs/adr/0052-per-hart-earliest-deadline-timer-and-affinity-respecting-steal.md` (kernel timeline fence), `docs/adr/0057-service-restart-capability-re-resolve.md` (restart + `STATUS_STALE`), `docs/adr/0041-never-black-boot-splash-atomic-desktop-reveal.md` (never-black reveal)

## Context

init today orders bring-up with `yield_()` calls, a hand-ordered driver resume list, time-capped
waits and `init: up <svc>` printed at resume time. On SMP none of these is a barrier: a resumed
task may not run for the whole boot (`touchd` never printed `payload ready` in proof boots), a
route can be answered before its target serves, and a reveal can be decided by a timer. The
kernel already provides a timeline fence (`fence_create`/`fence_signal`/`fence_wait`, ADR-0052)
and the control channel already carries verbs. The question is what init may synchronize on.

## Decision

- **Readiness is a message.** A service is ready when it sends `@ready` on its control channel
  (from `nexus_service_entry::ready`, the one place that also prints `<svc>: ready`); init
  records it per `ServiceId` and emits `init: up <svc>` only then. No other print of `": ready"`
  and no other emission of `init: up` may exist (`check-init-sync.sh`).
- **Stages are one kernel timeline fence per boot** with monotone values
  `Platform=1 < DisplayReady=2 < SessionStart=3 < ShellVisible=4`, signalled by init from the
  ready table (and windowd's `@stage` for the two display stages). Every child receives the
  fence capability at its declared slot with **WAIT rights only** and waits for its declared
  stage's prerequisite before `entry()`. If the kernel's capability transfer cannot restrict
  rights on transfer today, the kernel change is in scope of TASK-0324 P5 with its own proof
  (`KSELFTEST: fence transfer ok`); a child that could signal a stage is a security defect.
- **Forbidden as synchronization in init**: `yield_()`, resume order, time caps, marker
  presence. Dependencies between services are expressed only as parked routes (RFC-0093 §1)
  and the fence.
- Out of scope: supervision and restart policy (ADR-0057 stays), the compositor pipeline, the
  recovery graph's content (only its barrier sets derive from `boot_graph::includes`).

## Consequences

- **Positive**: boot order becomes a property of declarations (`ServiceSpec.stage`, routes),
  not of code order; "resumed" implies "ran to ready" or the boot fails loudly; headless,
  recovery and full graphs use the same verbs; the display chain can join the declarative arm.
- **Negative / accepted cost**: one control-channel verb per service (fleet-wide one-line
  change); init holds a ready table and a parked-route ring (bounded); marker order shifts
  (`init: up` after `<svc>: ready`) and proof manifests are updated in the same package; a
  possible kernel rights-mask change for the fence capability.
- **Delivered (TASK-0324 P5-a, 2026-09-12)**: the kernel floor is no longer hypothetical.
  `Rights::WAIT` exists beside `Rights::MANAGE`; `fence_create` mints `MANAGE | WAIT`,
  `fence_signal` requires `MANAGE`, and `fence_wait` accepts `WAIT` or `MANAGE` — so the
  WAIT-only copy a child receives can block for a stage but never release one for the fleet.
  Before this the check was on the capability KIND alone: every fence holder could signal.
  Proven in the boot ladder by `KSELFTEST: fence transfer ok` (create -> derive a WAIT-only
  copy -> wait succeeds -> signal refused). The liveness witness ships with it:
  `KSELFTEST: liveness snapshot FAIL quiet-stall ...`, latched once per boot when no task has
  been dispatched for 2 s, every online hart is idle and at least one task is blocked.
- **Delivered (TASK-0324 P5-b, 2026-09-12)**: the ladder itself. `Stage` (1-4) and
  `ServiceSpec.stage` are declared in the topology; init creates ONE fence per boot and pins it
  into every child — embedded, volume-spawned AND respawned — at spawn time with `Rights::WAIT`
  alone, so the cap is in place before the task is ever resumed. The responder is the only place
  a stage advances, and it prints `stage: <label>` AT THE SIGNAL SITE (the two markers that used
  to be printed at fixed points in init's code path are gone). ⭐ A barrier gates on DECLARED
  truth, not on a maintained list: a member must run under the target, must expose a server (a
  pure client or producer announces nothing to wait for — measured: exactly the specs with
  `exposes_server: false` are the ones that never print `init: up`), and must belong to the tier.
  Getting this wrong is silent: the first implementation gated the platform on the proof harness
  and the boot simply never reached a stage.
- **Delivered (TASK-0324 P5-c, 2026-09-12)**: the barrier actually binds, and the old
  synchronization is gone in the same package. `nexus_service_entry::os::bootstrap` waits for its
  service's declared stage prerequisite on the fence BEFORE `entry()` — the single funnel every
  OS service enters through, so this is the only startup synchronization a service performs. A
  process the topology does not declare (a spawned app child) holds no fence and waits for
  nothing. The fence decides the ORDER; a 2 s liveness deadline decides only how long a broken
  boot may stay SILENT — on it the service names itself once (`FAIL stage wait svc=… stage=…`)
  and keeps waiting, so the barrier still holds but the wedge has a name. Deleted: the four
  `yield_()` barriers in init (18 bounded IPC-retry yields are backoff, not synchronization, and
  stay) and the hand-sorted driver resume list. ⭐ The proof that the order was never the real
  contract: the drivers now resume in channel order — hidrawd FIRST, the exact case the old
  comment warned would produce a black screen — and the display chain still comes up
  (`windowd: ready` → `present ok` → `systemui: first frame visible`). `check-init-sync.sh`
  gates all three shapes and is self-tested against injected violations.
- **Follow-ups**: TASK-0324 P2 (`@ready`), P3 (parked routes), P5 (fence + liveness witness),
  P8 (docs: `09-nexus-init.md`, `06-boot-and-bringup.md`, RFC-0069 §4 implemented, RFC-0013).

## Alternatives considered

- Keep `yield_()`/resume order and add more time caps (rejected: correctness by timer, proven
  flaky under SMP and host load).
- A per-service semaphore or per-pair handshake (rejected: N mechanisms; one monotone fence
  covers the stage graph and already exists in the kernel).
- Treat `init: up` as ready (rejected by RFC-0013 A1; this ADR makes the marker honest instead).
- Let any child signal the fence (rejected: a compromised or buggy child could release later
  stages; WAIT-only rights are the boundary).
