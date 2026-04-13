---
title: TASK-0286 Kernel memory accounting v1: per-task RSS counters + pressure snapshots + trusted query ABI
status: Draft
owner: @runtime @kernel-team
created: 2026-04-13
depends-on:
  - TASK-0228
  - TASK-0269
follow-up-tasks:
  - TASK-0287
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Boot/resource gates: docs/rfcs/RFC-0013-boot-gates-readiness-spawn-resource-v1.md
  - OOM watchdog v1 (cooperative baseline): tasks/TASK-0228-oomd-v1-deterministic-watchdog-cooperative-memstat-samgr-kill.md
  - Security introspection guardrails: tasks/TASK-0230-nx-sec-v1-cli-security-introspection-deny-tests-offline.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

`TASK-0228` is intentionally cooperative because there is no stable kernel RSS/accounting ABI today.
That is good enough for bring-up, but it is not production-grade:

- policy cannot honestly enforce RSS-style limits,
- diagnostics cannot report true memory ownership,
- and OOM handling remains partially blind.

We need a small, explicit kernel accounting floor that gives true counters without turning the kernel
into a procfs-style monitoring subsystem.

## Goal

Provide a kernel-owned memory accounting contract that reports bounded, deterministic per-task memory
state to trusted readers:

- resident bytes,
- mapped bytes,
- page-fault / reclaim-relevant counters that are actually available,
- and global pressure snapshots suitable for OOM policy and diagnostics.

## Non-Goals

- Full cgroups or hierarchical accounting.
- Rich procfs/debugfs surfaces.
- Wall-clock sampling loops.
- Solving OOM enforcement by itself (that is `TASK-0287`).

## Constraints / invariants (hard requirements)

- **No fake precision**: if a counter is approximate, document the exact approximation.
- **Bounded kernel work**: no unbounded global page-table walks for ordinary reads.
- **Trusted readers only**: this ABI is for canonical system authorities, not ambient user queries.
- **Deterministic outputs**: same workload -> same counters/markers within defined tolerances.
- **Stable semantics**: do not overload existing fields or invent ad-hoc text reports.

## Red flags / decision points (track explicitly)

- **RED (shared mapping attribution)**:
  - pick one explicit charging rule for shared VMOs/mappings; do not let different readers infer different totals.
- **YELLOW (counter surface)**:
  - keep v1 minimal; every extra counter adds maintenance and proof burden.
- **GREEN (scope)**:
  - per-task + global snapshot is enough for production-grade closure v1; no general-purpose procfs required.

## Security considerations

### Threat model
- Untrusted tasks learning too much about other tasks' memory state.
- Spoofed userspace memory reports overriding kernel truth.
- Expensive query paths becoming a DoS surface.

### Security invariants (MUST hold)
- Kernel accounting is the source of truth when exposed through this ABI.
- Only trusted/system-authorized readers may query per-task accounting.
- Query cost is bounded and does not require scanning unrelated tasks on every request.

### DON'T DO (explicit prohibitions)
- DON'T expose unrestricted per-task memory queries to arbitrary apps.
- DON'T claim counters are exact RSS if shared mappings are only approximately charged.
- DON'T add debug-only bypasses that change accounting semantics in release builds.

## Contract sources (single source of truth)

- QEMU marker contract: `scripts/qemu-test.sh`
- Kernel task/resource truth: `TASK-0269`
- Cooperative OOM baseline and limitations: `TASK-0228`

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - kernel/unit tests prove:
    - map/unmap/fault accounting updates deterministically,
    - shared-mapping charging follows one documented rule,
    - unauthorized readers are rejected.
- **Proof (OS/QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=180s ./scripts/qemu-test.sh`
  - required markers:
    - `neuron: memacct on`
    - `KSELFTEST: memacct rss ok`
    - `KSELFTEST: memacct query deny ok`
    - `SELFTEST: oomd rss query ok`

## Touched paths (allowlist)

- `source/kernel/neuron/src/mm/`
- `source/kernel/neuron/src/task/`
- `source/kernel/neuron/src/syscall/`
- `source/libs/nexus-abi/`
- `source/services/oomd/`
- `source/apps/selftest-client/`
- `docs/architecture/01-neuron-kernel.md`
- `docs/reliability/`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. Define the minimal accounting model and ABI shape.
2. Wire map/unmap/fault-side counter maintenance with host tests.
3. Add trusted query path and reject-path tests.
4. Integrate `oomd`/diagnostics reader and QEMU selftests.

## Acceptance criteria (behavioral)

- Trusted system code can read real kernel memory counters for a task.
- Unauthorized queries fail deterministically.
- The repo no longer has to pretend cooperative memstat is kernel RSS.

## Evidence (to paste into PR)

- QEMU: marker excerpt showing memory-accounting enablement and deny/selftest markers.
- Tests: exact kernel/unit test summary for accounting and reject paths.
