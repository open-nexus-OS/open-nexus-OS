---
title: TASK-0287 Kernel memory pressure v1: watermarks + hard limits + canonical OOM handoff
status: Draft
owner: @runtime @kernel-team @reliability
created: 2026-04-13
depends-on:
  - TASK-0228
  - TASK-0269
  - TASK-0286
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - OOM watchdog v1 (cooperative baseline): tasks/TASK-0228-oomd-v1-deterministic-watchdog-cooperative-memstat-samgr-kill.md
  - Ability/app kill reason propagation: tasks/TASK-0235-ability-v1_1b-os-appmgrd-extension-samgr-hooks-fgbg-policies-selftests.md
  - Boot/resource gates: docs/rfcs/RFC-0013-boot-gates-readiness-spawn-resource-v1.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

After `TASK-0286`, the system can know real memory state, but production-grade runtime still needs
an enforcement contract:

- pressure states must be surfaced before the system falls off a cliff,
- hard limits must fail deterministically instead of depending on luck,
- and OOM action must still respect the single kill-authority model.

This task closes that gap without turning the kernel into a policy engine.

## Goal

Introduce a kernel pressure-and-enforcement floor that:

- tracks global pressure watermarks,
- enforces hard memory ceilings / reserve protection deterministically,
- and hands OOM action to canonical userland authorities with stable reasons.

## Non-Goals

- Full kernel OOM killer heuristics zoo.
- Linux-style cgroups / memory controller parity.
- Best-effort reclaim magic that hides failure.
- Policy duplication outside canonical services.

## Constraints / invariants (hard requirements)

- **Single authority for kill execution**: kernel may signal/enforce limits, but orderly kill flows remain canonical.
- **Deterministic failure**: over-limit allocation paths fail with stable reasons, not timing races.
- **Protected reserves**: critical services and system reserves must not be starved by ordinary workloads.
- **Bounded reaction**: no unbounded reclaim loops or endless retry paths.

## Red flags / decision points (track explicitly)

- **RED (victim semantics)**:
  - v1 must choose whether the kernel only blocks/flags or also nominates a victim; do not blur the contract.
- **YELLOW (critical-service exemptions)**:
  - exemptions must be explicit and minimal, not an "allow all system services" escape hatch.
- **GREEN (architecture)**:
  - policy decides who is protected/preferred; kernel enforces limits and truth.

## Security considerations

### Threat model
- Malicious tasks attempting memory exhaustion to starve trusted services.
- Unbounded reclaim/kill loops becoming a kernel DoS vector.
- Unauthorized tasks gaining protected-memory treatment.

### Security invariants (MUST hold)
- Global reserve and pressure thresholds are kernel-enforced.
- Critical-service protection is explicit and auditable.
- OOM reason codes are stable enough for audit and lifecycle propagation.

### DON'T DO (explicit prohibitions)
- DON'T add a second userspace-independent task-kill authority.
- DON'T hide allocation failure behind infinite retry/yield loops.
- DON'T make protection classes ambient or implicit.

## Contract sources (single source of truth)

- Memory truth ABI: `TASK-0286`
- Cooperative OOM baseline: `TASK-0228`
- Lifecycle/kill reason propagation: `TASK-0235`

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - kernel/unit tests prove:
    - watermark transitions are deterministic,
    - hard-limit violations return the documented failure,
    - protected reserves are preserved under pressure fixtures.
- **Proof (OS/QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=210s ./scripts/qemu-test.sh`
  - required markers:
    - `neuron: pressure high`
    - `neuron: oom handoff ready`
    - `KSELFTEST: mem pressure limit ok`
    - `SELFTEST: oomd kernel pressure ok`

## Touched paths (allowlist)

- `source/kernel/neuron/src/mm/`
- `source/kernel/neuron/src/task/`
- `source/kernel/neuron/src/syscall/`
- `source/libs/nexus-abi/`
- `source/services/oomd/`
- `source/services/execd/`
- `source/services/appmgrd/`
- `source/apps/selftest-client/`
- `docs/reliability/`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. Add watermark / reserve model and host fixtures.
2. Add hard-limit enforcement and stable error semantics.
3. Wire kernel -> `oomd` / lifecycle handoff with canonical reasons.
4. Prove reserve protection and OOM handoff in QEMU.

## Acceptance criteria (behavioral)

- Memory pressure becomes a first-class, deterministic kernel signal.
- Ordinary tasks cannot exhaust the system without hitting a stable enforced boundary.
- OOM handling is no longer dependent on cooperative-only accounting.

## Evidence (to paste into PR)

- QEMU: pressure + OOM handoff marker excerpt.
- Tests: host/kernel summaries for watermarks, hard limits, and reserve protection.
