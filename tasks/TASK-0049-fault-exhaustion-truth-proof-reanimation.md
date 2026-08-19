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

- **RED → RESOLVED 2026-08-19 (root-cause round, bounded)**: the retired chain's
  root cause was already found and fixed by 0080D R1 — `spawn_inner` starts every
  task Suspended (grants-before-resume hardening) and execd never resumed its
  children; the fix is live at `source/services/execd/src/os_lite.rs:902-908`
  ("#102 ROOT CAUSE FIX" comment). The #102-family kernel doubt (sender-wake for
  blocking-recv children) has a green standing regression gate since 2026-07-07
  (`SELFTEST: exec child blocking recv wake ok` via `recv-wake-probe`).
  **Boot-verified 2026-08-19** (headless ladder, exit 0): `child: hello-elf`
  appears in UART BEFORE the parent's `execd: elf load ok` — exec'd children
  execute. Consequences: (a) reanimation STAYS in this task, no split;
  (b) restoration is a bounded revert-shaped patch — the retirement commit
  `af0c7a8d` ("fix: selftest bug, now green", 2026-06-30) deleted exactly
  185 lines in `phases/exec.rs`, 42 marker lines in `markers/exec.toml`, and
  16 UART gates in `scripts/qemu-test.sh` (list preserved in that commit);
  all six selftest helpers the chain used still exist (`wait_for_pid`,
  `emit_line_with_pid_status`, `locate_minidump_for_crash`,
  `statefs_has_crash_dump`, `grant_statefs_caps_to_child`,
  `logd_query_contains_since_paged`); (c) reanimation can land as the FIRST
  PR of this task (independent of the exit-reason ABI work) — note the
  current `execd: elf load ok` / `SELFTEST: e2e exec-elf ok` emits are
  parent-side and unconditional after 256 yields, i.e. they do not prove
  child execution; `child: hello-elf` must return as a gated marker.
- **Reanimation findings 2026-08-19 (second round, during restore)**:
  (d) the restored chain's selftest-side statefs cap grant into the
  demo.minidump child raced the child's exit — it was only ever reliable
  while the #102 bug kept children suspended. Fixed the end-state way:
  execd grants the statefs route (child slots 7/8, payload SSOT
  `userspace/apps/demo-exit0/build.rs`) BEFORE `task_resume`
  (`grant_minidump_statefs_route`, same grants-before-resume discipline as
  the app-host grants); the selftest-side helper was deleted.
  (e) **ADR-0054 gap found in the kernel**: `core/trap/errno.rs:44` maps
  EVERY `SysError::Transfer(_)` to EPERM — InvalidChild (reaped task),
  Capability(NoSpace) and slot-occupied all collapse to `CapabilityDenied`
  in userspace, which cost one full instrumented boot to differentiate.
  Fold the identity-preserving errno split into this task's kernel PR
  (same file family as the exit-reason work; keep ADR-0054 discipline).
  (f) **execd had no statefsd route at all** (`init: route statefsd
  NOT_FOUND`): neither declared in `REQUIRED_ROUTES` nor wired in the init
  execd arm — which also means execd's own crash-dump writer
  (`write_dump_to_statefs` → `KernelClient::new_for("statefsd")`) could
  never have worked on this topology. Fixed declaratively: route entry
  `(Execd, Statefsd)` + a SharedResponse clone pair in the execd wiring arm
  (appended as a NAMED route behind the positional slot-order contracts —
  never insert transfers before the windowd 8/9 / bundle 10 / probe 11–14
  blocks). Boot proof: `init: execd route->statefsd ok`,
  `execd: minidump statefs route granted`, child exits 42, full chain green
  (headless run 2026-08-19T11-43-28, 16/16 markers).
  (g) The headless/smp1 harness arm never gated the chain (pre-dating the
  retirement) — that was the second half of the masking. Fixed: the 14
  chain gates are appended for `headless|smp1` (profiles that run the full
  service ladder); network/display profiles are deliberately excluded
  (they may stop before the exec phase).
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

1. ✅ **Root-cause round (bounded)** — DONE 2026-08-19; findings recorded in
   the resolved RED above (root cause already fixed by 0080D R1; two follow-on
   root causes found and fixed during restore).
2. **Kernel + ABI exit reason** (after approval): TCB field, trap/exit/kill call
   sites, wait delivery, `nexus-abi` enum + tests. Include the errno identity
   split for `SysError::Transfer(_)` (finding (e)).
3. **execd plumbing**: reason in reap loop, envelope field, markers.
4. **Exhaustion events**: gpud + statefsd transitions, selftest knobs, markers.
5. ✅ **Proof reanimation** — DONE 2026-08-19 (PR-1, delivered before step 2 —
   independent of the ABI work): chain restored verbatim from af0c7a8d^,
   16/16 markers boot-proven, hard-gated on headless|smp1 (`just test-all`
   green incl. the smp1 deterministic gate). Mechanics: execd grants the
   minidump child's statefs route BEFORE resume
   (`source/services/execd/src/child_grants.rs`), execd↔statefsd named route
   added declaratively (REQUIRED_ROUTES + execd wiring arm →
   `provision_execd_named_routes` in `route_provision.rs`). Structure-ratchet
   splits shipped alongside: `phases/exec.rs` → `probes/soaks.rs` (ADR-0048
   detectors), execd grant family → `child_grants.rs`.
