# RFC-0040: Zero-Copy VMOs v1 Plumbing — host-first, OS-gated contract seed

- Status: Done
- Owners: @runtime / @kernel-team
- Created: 2026-04-21
- Last Updated: 2026-04-23
- Links:
  - Tasks: `tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md` (execution + proof), `tasks/TASK-0290-kernel-zero-copy-closure-v1b-vmo-seals-reuse-truth.md` (production-grade closure)
  - Production gate policy: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
  - ADRs: None (as of draft)
  - Related RFCs: `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md`, `docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md`, `docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md`

## Status at a Glance

- **Phase 0 (API + ownership contract)**: ✅
- **Phase 1 (cross-process transfer + deterministic proof)**: ✅
- **Out-of-scope handoff (kernel production closure)**: delegated to `TASK-0290`

Definition:

- "Done" means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean "never changes again".

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - Contract boundaries for a userspace VMO handle abstraction used by services for zero-copy-style data exchange.
  - Ownership/lifetime semantics for handles and mappings (drop behavior, single close authority, explicit transfer semantics).
  - Host-first proof contract and OS-gated marker contract for deny-by-default, bounded behavior.
  - Rust safety discipline expectations for this track (`newtype` boundaries, `Send`/`Sync` decisions, ownership, `#[must_use]` where ignoring results can hide bugs).
- **This RFC does NOT own**:
  - Kernel architecture redesign or large ABI expansion outside the existing VMO/capability syscall family.
  - Kernel-side production-grade closure obligations (seal rights, write-map denial, lifecycle closure, reuse/copy-fallback truth); these are execution-owned by `TASK-0290`.
  - Replacing the execution SSOT with RFC checklist prose; implementation/proof commands stay in tasks.
  - App/platform-level adoption plans for every consumer service.

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define **stop conditions** and **proof commands**.
- This RFC must link to the task(s) that implement and prove each phase/milestone.
- `TASK-0031` is the SSOT execution slice for v1 plumbing and honest baseline proofs.
- `TASK-0290` is the SSOT execution slice for the kernel-side production-grade zero-copy closure.

## Context

The repo has kernel-side VMO/capability primitives and ABI exports, but still lacks a fully closed, userspace-facing zero-copy plumbing contract with deterministic cross-process proof and explicit lifecycle closure guarantees. We need a contract seed that keeps scope narrow (v1 plumbing) while preventing drift into unbounded "production closure" work.

## Goals

- Define a minimal, explicit userspace VMO contract that supports creation, mapping, and transfer across service boundaries.
- Require deterministic, behavior-first proofs (including reject paths) instead of marker-only success claims.
- Enforce Rust safety baseline for this boundary: typed handles, clear ownership, justified thread-safety, and meaningful `#[must_use]` usage.

## Non-Goals

- Claiming production-grade at `TASK-0031` bring-up stage without the closure proof set from `TASK-0290`.
- Introducing Linux/Wayland or non-RISC-V shortcuts.
- Declaring kernel-enforced sealing semantics beyond what current syscalls/contracts can honestly guarantee.

## Constraints / invariants (hard requirements)

- **Determinism**: markers/proofs are deterministic; no timing-fluke "usually ok".
- **No fake success**: never emit "ok/ready" markers unless the real behavior occurred.
- **Bounded resources**: explicit limits for transfer sizes, mapping attempts, retry loops, and handle lifetimes.
- **Security floor**: capability transfer must stay explicit and deny-by-default for invalid handles/rights/state.
- **Stubs policy**: any stub must be explicitly labeled, non-authoritative, and must not claim success.
- **Rust type discipline**: capability/VMO identifiers crossing crate boundaries use `newtype` wrappers (or documented equivalent) to avoid mixups.
- **Thread-safety discipline**: `Send`/`Sync` are only derived/implemented when invariants are stated and test-covered.
- **Ownership discipline**: mapping lifetime and handle-drop semantics are explicit and testable.
- **Result-use discipline**: annotate critical return values with `#[must_use]` where ignored outcomes can mask correctness/security failures.
- **Production-grade boundary**: production-gate closure against `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` is out of scope for this RFC and execution-owned by `TASK-0290` and follow-ups.

## Proposed design

### Contract / interface (normative)

Introduce (or finalize) a userspace `nexus-vmo` abstraction layer that:

- Wraps raw VMO capabilities/handles in typed APIs.
- Separates local mapping operations from cross-process transfer operations.
- Defines explicit error model for invalid capability, right mismatch, missing peer endpoint, mapping failure, and lifecycle misuse.
- Exposes stable v1 behavior for host tests first, then OS/QEMU marker-backed proof.

Versioning strategy:

- v1 contract is "plumbing baseline" and intentionally minimal.
- Any behavior beyond this baseline (kernel-enforced sealing, broader production hardening, perf gates) is owned by follow-up task/RFC slices rather than silently expanding this RFC.

### Phases / milestones (contract-level)

- **Phase 0**: Typed VMO API + ownership contract + host reject-path proof.
- **Phase 1**: Cross-process VMO transfer contract proven in OS/QEMU with deterministic markers.

## Out-of-scope handoff (normative stop condition)

Kernel production closure is explicitly outside RFC-0040 scope.

- Stop condition for this RFC:
  - Phase 0 and Phase 1 proofs are green and deterministic.
  - The kernel production obligations are explicitly delegated to `TASK-0290` (not silently retained in this RFC scope).
- `TASK-0290` remains the closeout owner for:
  - kernel-enforced seal/rights semantics,
  - write-map denial guarantees,
  - lifecycle closure (`vmo_destroy` path),
  - reuse/copy-fallback production truth for Gate A/Gate C closure.

## Security considerations

- **Threat model**: confused-deputy handle misuse, unauthorized capability transfer, stale-handle reuse, and unsafe fallback paths that silently copy or downgrade permissions.
- **Mitigations**: capability-gated transfer only, deny-by-default on invalid state/rights, bounded parsing and transfer sizes, explicit ownership model, negative tests for reject paths.
- **Open risks**: lifecycle closure remains incomplete until kernel-side destroy/closure path and production hardening tasks are completed and proven.

### DON'T DO (security)

- Do not accept raw untyped handle integers across public crate boundaries without validation/wrapping.
- Do not "warn and continue" on transfer/mapping authorization failures.
- Do not emit success markers for degraded/stub/deny paths.
- Do not use unbounded retry, drain, or polling loops in transfer/mapping proof paths.

## Failure model (normative)

- Invalid/missing handle or rights mismatch must fail closed with explicit error (no silent fallback).
- Cross-process transfer failures must be observable and test asserted.
- Mapping/transfer retry behavior must be bounded; no unbounded yield/drain loops.
- If OS path preconditions are missing, proofs must show deterministic deny/degraded markers, never "success" markers.

## Proof / validation strategy (required)

List the canonical proofs; tasks must implement them.

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p nexus-vmo
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

### Deterministic markers (if applicable)

- `vmo: producer sent handle`
- `vmo: consumer mapped ok`
- `vmo: sha256 ok`
- `SELFTEST: vmo share ok`

## Alternatives considered

- Copy-only IPC buffers for all payloads (rejected: does not satisfy zero-copy track goals).
- Expanding `TASK-0031` to full production-grade closure (rejected: scope drift; conflicts with task/RFC authority model).
- Kernel-first broad redesign before userspace contract seed (rejected: delays proofable host-first progress and weakens iteration loop).

## Open questions

- None for v1 plumbing scope. Kernel closure questions are owned by `TASK-0290`.

## RFC Quality Guidelines (for authors)

When writing this RFC, ensure:

- Scope boundaries are explicit; cross-RFC ownership is linked.
- Determinism + bounded resources are specified in Constraints section.
- Security invariants are stated (threat model, mitigations, DON'T DO).
- Proof strategy is concrete (not "we will test this later").
- If claiming stability: define ABI/on-wire format + versioning strategy.
- Stubs (if any) are explicitly labeled and non-authoritative.

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0**: typed userspace VMO API contract + host reject-path tests — proof: `cd /home/jenning/open-nexus-OS && cargo test -p nexus-vmo`
- [x] **Phase 1**: cross-process transfer + deterministic QEMU marker proof — proof: `cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
- [x] Out-of-scope handoff to `TASK-0290` is explicit in RFC text.
- [x] Task(s) linked with stop conditions + proof commands.
- [x] QEMU markers (if any) appear in `scripts/qemu-test.sh` and pass.
- [x] Security-relevant negative tests exist (`test_reject_*`).
