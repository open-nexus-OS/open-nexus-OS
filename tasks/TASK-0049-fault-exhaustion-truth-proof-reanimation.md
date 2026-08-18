---
title: TASK-0049 Reliability v1a: fault & exhaustion truth — exit reasons in the kernel ABI + first exhaustion events + crash-proof reanimation
status: Draft
owner: @reliability
created: 2025-12-23
updated: 2026-08-18
depends-on:
  - TASK-0006 # logd (Done — crash envelope sink)
  - TASK-0018 # crashdumps v1 (Done — but OS proof retired; reanimated HERE)
  - TASK-0026 # statefs journal v2 (Done — evidence substrate for 0049C)
follow-up-tasks:
  - TASK-0049B # supervision consumes the exit reason
  - TASK-0049C # persistent evidence journal
  - TASK-0051B # crash evidence at rest (former scope of this ledger)
links:
  - Reliability contract: docs/rfcs/RFC-0087-reliability-failure-model-v1.md
  - Exit-reason decision: docs/adr/0056-task-exit-reason-kernel-abi.md
  - Crash envelope: docs/rfcs/RFC-0011-logd-journal-crash-v1.md
  - Crashdumps v1: docs/rfcs/RFC-0031-crashdumps-v1-minidump-host-symbolize.md
  - Reaper mechanics: docs/rfcs/RFC-0081-process-reaper-nonblocking-reclaim.md
  - Open point #102: tasks/TRACK-OPEN-POINTS-2026-07.md
  - Testing contract: scripts/qemu-test.sh
---

## Rewrite 2026-08-18 (was: "Crashdump v2b: crashd ingestion + retention + correlation")

The original 2025-12-23 scope is re-cut against repo reality:

- **`crashd` ingestion/retention/redaction moves to TASK-0051B.** Building a
  collector was premature while (a) the on-device crash *producer* is unproven —
  the execd exec/crash/minidump selftest chain was retired during the RFC-0068
  exec migration (`source/apps/selftest-client/src/os_lite/phases/exec.rs:102-107`,
  only FAIL markers remain gated), and (b) "correlate with recent logs" targeted a
  16 KiB RAM ring that survives neither 128 log lines nor a reboot (0049C fixes).
- **The `userspace/runtime/nexus-crash/` crate proposal is dropped** —
  `userspace/crash` (NMD1, TASK-0018) already owns in-process capture.
- **This ledger now owns Phase 0 of RFC-0087: detection.** You can only recover
  what you can detect; today every fault is `exit(-22)` and two documented
  degradations are silent.

## Context

Three detection holes, all verified in code:

1. **Exit-code ambiguity**: the kernel kills a faulting task via
   `exit_current_and_release(tasks, -22)` (`core/trap/handler.rs`) — a page
   fault, an illegal instruction, a voluntary `exit(-22)` and a policy kill are
   indistinguishable to `wait`. Supervision (0049B) and honest crash retention
   (0051B) both need the real reason (ADR-0056).
2. **Silent degradation**: VMO-arena exhaustion silently drops gpud GL→2D;
   statefsd silently stays RAM-backed when the virtio upgrade window is missed
   (`source/services/statefsd/src/os_lite.rs` pristine-window upgrade). RFC-0087
   makes exhaustion an event; this task wires the two proven cases.
3. **Dark proof**: TASK-0018 is Done but its OS markers (`SELFTEST: minidump ok`,
   `SELFTEST: crash report ok`, `execd: minidump written`) are gated nowhere;
   only FAIL variants exist in `proof-manifest/markers/exec.toml`.

## Goal

1. **Exit reason in kernel + ABI** (ADR-0056): `clean | error | fault{cause} |
   kill{by}` recorded in the TCB at death, panic sub-flag set by the service
   runtime; delivered via `sys_wait`/`sys_wait_nohang` and typed in `nexus-abi`.
   Cause decoding reuses `core/trap/fault.rs` (no duplication).
2. **execd consumes the reason**: `reap_ready_children()` reports
   `execd: exit pid=<pid> reason=<reason> code=<code>`; the RFC-0011 crash
   envelope gains a `reason` field (`crash_fields.rs`).
3. **Exhaustion events v1** (RFC-0087 vocabulary, userspace-side, small):
   - `gpud: degrade gl->2d (reason=arena)` + `event=exhaust.v1` to logd,
   - `statefsd: degrade ram-backed (upgrade missed)` + event,
   both emitted exactly once per boot at the real transition.
4. **Proof reanimation** (closes #102): root-cause the retired chain
   ("execd-spawned children LOAD but no longer execute"), restore the
   exec/crash/minidump ladder incl. the three TASK-0018 negative rejects, and
   re-gate the ok-markers in `proof-manifest/markers/exec.toml` +
   `scripts/qemu-test.sh`.

## Non-Goals

- crashd / retention / GC / redaction (TASK-0051B).
- Persistent evidence journal (TASK-0049C).
- Restart/backoff policy (TASK-0049B).
- Kernel memory watermarks / OOM enforcement (TASK-0286/0287 — this task only
  defines where their events land).
- Register/memory capture for uncontrolled crashes (RFC-0031 limitation stands).

## Constraints / invariants (hard requirements)

- **Approval zones**: `source/kernel/**` and `source/libs/nexus-abi` — explicit
  user approval before touching; ADR-0056 is the design authority.
- Reason is kernel truth: userspace can never mask a fault; the panic sub-flag
  can only *add* to a voluntary exit (ADR-0056).
- ADR-0054 discipline: every kernel reason maps to exactly one ABI variant, no
  wildcard arms.
- Exhaustion events are bounded (fixed fields, once per boot per resource) and
  emitted at the real transition — never speculatively.
- No `unwrap/expect` on untrusted input; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (unknown size — retired chain root cause)**: "children LOAD but no longer
  execute" is undocumented. Bounded debugging first (hypotheses + time cap); if
  the cause reaches deep into the RFC-0068 exec migration, split the reanimation
  into its own ledger rather than inflating this one. Record findings here.
- **YELLOW (wait-ABI shape)**: whether reason rides in the status word (packed)
  or a widened return struct is decided in ADR-0056 execution; whichever is
  chosen, `nexus-abi` presents a typed enum and both wait paths agree.
- **YELLOW (marker stability)**: `execd: crash report …` is an existing marker
  consumed by tests; extend (`reason=`) without breaking existing greps, and
  update `scripts/qemu-test.sh` + proof-manifest in the same PR (marker contract
  rule).

## Contract sources (single source of truth)

- Reason taxonomy: RFC-0087 §1; encoding: ADR-0056.
- Crash envelope: RFC-0011 (extensible fields — `reason` is additive).
- Marker contract: `scripts/qemu-test.sh` + `proof-manifest/markers/exec.toml`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- `cargo test -p nexus-abi` — exit-reason enum roundtrip, no-wildcard mapping.
- `cargo test -p execd` — reap path maps reason → envelope field; `test_reject_*`
  for forged/malformed reason bytes on the wire.
- Kernel host-proof where applicable (fault→reason table pure-function tests).

### Proof (OS/QEMU) — required

UART markers (gated in `scripts/qemu-test.sh` + proof-manifest):

- `execd: exit pid=<pid> reason=fault code=<code>` (induced fault child)
- `execd: exit pid=<pid> reason=clean code=0` (exit0 child)
- `SELFTEST: exit reason ok`
- `SELFTEST: crash report ok` (reanimated)
- `SELFTEST: minidump ok` + `execd: minidump written <path>` (reanimated)
- three negative rejects from TASK-0018 (forged metadata / no-artifact /
  mismatched build_id) re-gated
- `gpud: degrade gl->2d (reason=arena)` — proven via forced-arena-exhaustion
  selftest knob (deterministic), and
- `statefsd: degrade ram-backed (upgrade missed)` — proven via withheld-grant
  selftest knob; both also as `event=exhaust.v1` records queryable from logd.

## Touched paths (allowlist)

- `source/kernel/neuron/src/core/trap/handler.rs`, `core/trap/fault.rs`,
  `syscall/api/sched_task.rs` (approval zone; exit-reason record + wait delivery)
- `source/libs/nexus-abi/` (approval zone; typed reason enum)
- `source/services/execd/src/` (reap path, crash_fields, markers)
- `source/drivers/gpud/src/` (degrade event at the GL→2D transition)
- `source/services/statefsd/src/` (degrade event at missed upgrade)
- `source/apps/selftest-client/` (reanimated exec phase + new proofs)
- `source/apps/selftest-client/proof-manifest/markers/exec.toml`
- `scripts/qemu-test.sh` (marker list)
- `docs/reliability/` (failure vocabulary section), `docs/services/lifecycle.md`
  (correction — the described OS supervisor does not exist yet; point to RFC-0087)

## Plan (small PRs)

1. **Root-cause round (bounded)**: why do execd-spawned children load but not
   execute? Fix or split; record here.
2. **Kernel + ABI exit reason** (after approval): TCB field, trap/exit/kill call
   sites, wait delivery, `nexus-abi` enum + tests.
3. **execd plumbing**: reason in reap loop, envelope field, markers.
4. **Exhaustion events**: gpud + statefsd transitions, selftest knobs, markers.
5. **Proof reanimation**: restore ladder, re-gate ok-markers + negative rejects,
   `just test-all`.
