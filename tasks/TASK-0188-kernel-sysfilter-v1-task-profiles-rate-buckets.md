---
title: TASK-0188 Kernel sysfilter v1 (NEURON): per-task syscall allowlist + rate buckets + profile IDs (true enforcement)
status: Draft
owner: @runtime
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Kernel IPC/cap model: docs/rfcs/RFC-0005-kernel-ipc-capability-model.md
  - Userland syscall guardrails (not a boundary): tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md
  - Kernel ABI/syscalls work (related): tasks/TASK-0042-smp-v2-affinity-qos-budgets-kernel-abi.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

`TASK-0019` provides “seccomp-like” guardrails in userland, but explicitly is **not** a security boundary.
This task introduces **true enforcement** in the NEURON kernel syscall entry path:

- default-deny syscall allowlists,
- per-task profile IDs attached at spawn,
- token-bucket rate limits on selected syscalls,
- stable deny semantics (`EPERM`) and rate-limited kernel markers.

This is kernel work by design.

Related planning:

- If we want policy-authored sysfilter profiles compiled into a compact, OS-friendly artifact (rather than hand-coded tables),
  that snapshot output is tracked in `TASK-0229` (must not introduce a parallel policy authority).

## Goal

Deliver:

1. Kernel sysfilter hook:
   - add `source/kernel/neuron/src/security/sysfilter.rs`
   - wire it into syscall entry before dispatch
2. Per-task profile identity:
   - attach a `TaskProfileId` (small integer) to each task at spawn
   - profile selection is controlled by the spawner (`execd` / kernel spawn ABI); default profile is deny-by-default minimal for kernel selftests only
3. Allowlist model:
   - start with core syscall set:
     - `ipc_send`, `ipc_recv_v1`, `ipc_recv_v2`, `yield`, `nsec`, `map`, `spawn`, `cap_*`
   - enforce granular cap syscalls (e.g. `cap_transfer`, `cap_grant`) separately
4. Rate buckets (token bucket):
   - per-task, per-syscall bucket for `send/recv/spawn` (initial set)
   - deterministic refill based on monotonic time source
5. Violation behavior:
   - on deny: return `EPERM`, increment per-task counter
   - emit rate-limited marker (do not spam UART)
   - markers:
     - `neuron: sysfilter on`
     - `neuron: deny pid=<..> sys=<name>`
     - `neuron: rate pid=<..> sys=<name> dropped=<n>`

## Non-Goals

- Designing the full sandbox policy language (userspace profiles handle that).
- Perfect rate shaping (token bucket is sufficient for v1).

## Constraints / invariants (hard requirements)

- **Kernel behavior change is explicit**: this task must be isolated and proven; no “text-only refactor”.
- Determinism: stable counters and stable marker strings; rate refill uses monotonic time only.
- No fake success: markers only reflect real denies/rate drops.

## Red flags / decision points (track explicitly)

- **RED (ABI/spawn contract)**:
  - Kernel must know the task’s `TaskProfileId`. If the current spawn ABI cannot carry it safely, this task requires an ABI extension.
  - Any ABI change must be versioned and documented; do not silently reinterpret existing fields.

- **RED (self-hosting risk)**:
  - Default-deny can brick the system if profiles are wrong. Provide a safe “bootstrap profile” for init/execd bring-up.

## Security considerations

### Threat model

- **Syscall abuse/DoS**: tight loops calling syscalls to starve the system (availability)
- **Privilege escalation via missing allowlist**: unintended syscall availability for a profile
- **Non-deterministic enforcement**: timing-based tests or refill logic causing flaky or bypassable enforcement

### Security invariants (MUST hold)

- **Default-deny**: profiles are deny-by-default; explicit allowlist required
- **Deterministic enforcement**: rate buckets refill deterministically using monotonic time
- **Stable denial semantics**: denied syscalls return `EPERM` deterministically
- **Bounded logging**: deny/rate markers are rate-limited; no UART flood

### DON'T DO (explicit prohibitions)

- DON'T ship a “debug allow all” profile in production images
- DON'T make enforcement timing-dependent in tests/proofs
- DON'T log secrets or high-volume per-deny details in production

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p neuron_kernel_tests -- --nocapture` (or equivalent) proves:
    - allowlisted syscall succeeds
    - denied syscall returns `EPERM`
    - rate bucket drops deterministically under injected time

- **Proof (QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=210s ./scripts/qemu-test.sh`
  - Required markers:
    - `neuron: sysfilter on`
    - `SELFTEST: sysfilter deny ok`
    - `SELFTEST: sysfilter rate ok`

## Touched paths (allowlist)

- `source/kernel/neuron/src/security/sysfilter.rs` (new)
- syscall entry path (`source/kernel/neuron/src/syscall/*`)
- task model (`source/kernel/neuron/src/task.rs`)
- spawn ABI path (kernel + execd integration point)
- `source/apps/selftest-client/`
- `scripts/qemu-test.sh`
- `docs/security/sysfilter.md`

## Plan (small PRs)

1. Add sysfilter module + deny-by-default for a minimal test profile (kernel selftest only)
2. Add TaskProfileId plumbing at spawn + stable kernel markers
3. Add token bucket rate limiting + deterministic tests
4. Update docs and marker contract

## Acceptance criteria (behavioral)

- In QEMU, at least one denied syscall and one rate-drop case is proven by selftest markers without breaking boot.
