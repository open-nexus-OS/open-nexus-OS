# RFC-0040: Zero-Copy VMOs v1 Plumbing — host-first, OS-gated contract seed

- Status: In Progress
- Owners: @runtime / @kernel-team
- Created: 2026-04-21
- Last Updated: 2026-04-21
- Links:
  - Tasks: `tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md` (execution + proof), `tasks/TASK-0290-kernel-zero-copy-closure-v1b-vmo-seals-reuse-truth.md` (production-grade closure)
  - Production gate policy: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
  - ADRs: None (as of draft)
  - Related RFCs: `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md`, `docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md`, `docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md`

## Status at a Glance

- **Phase 0 (API + ownership contract)**: ⬜
- **Phase 1 (cross-process transfer + deterministic proof)**: ⬜
- **Phase 2 (production-grade closure gates)**: ⬜

Definition:

- "Complete" means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean "never changes again".

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - Contract boundaries for a userspace VMO handle abstraction used by services for zero-copy-style data exchange.
  - Ownership/lifetime semantics for handles and mappings (drop behavior, single close authority, explicit transfer semantics).
  - Host-first proof contract and OS-gated marker contract for deny-by-default, bounded behavior.
  - Rust safety discipline expectations for this track (`newtype` boundaries, `Send`/`Sync` decisions, ownership, `#[must_use]` where ignoring results can hide bugs).
  - The requirement that this track reaches production-grade closure for Kernel Core/Runtime and Storage-facing zero-copy claims before this RFC can be marked complete.
- **This RFC does NOT own**:
  - Kernel architecture redesign or large ABI expansion outside the existing VMO/capability syscall family.
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
- Require explicit production-grade closure gates (via `TASK-0290` and linked follow-ups) before declaring this contract complete.

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
- **Production-grade gating**: completion requires closure against the relevant production gates in `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Kernel Core/Runtime and Storage zero-copy integrity expectations), proven through `TASK-0290` and linked follow-ups.

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
- **Phase 2**: Production-grade zero-copy closure proven (`TASK-0290`) with kernel-enforced sealing/rights and deterministic closure markers.

## Production-grade requirement (normative)

This RFC is a contract seed that starts host-first, but it is **not complete** until production-grade closure is proven.

- Production-grade claims for this track must align with `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`, especially:
  - Gate A (Kernel Core & Runtime): deterministic capability/map/rights behavior with no fake-success claims.
  - Gate C (Storage, PackageFS & Content): honest zero-copy behavior where bulk transfer semantics are explicit and bounded.
- `TASK-0031` establishes the plumbing/honesty floor and must not claim closure early.
- `TASK-0290` provides the kernel-side closeout obligations (seal rights, write-map denial, reuse/copy-fallback truth).
- RFC status can move to **Complete** only after those production-grade obligations are proven green.

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

- `selftest: vmo-share basic ok`
- `selftest: vmo-share reject-invalid-cap ok`
- `selftest: vmo-share lifecycle-bounded ok`

## Alternatives considered

- Copy-only IPC buffers for all payloads (rejected: does not satisfy zero-copy track goals).
- Expanding `TASK-0031` to full production-grade closure (rejected: scope drift; conflicts with task/RFC authority model).
- Kernel-first broad redesign before userspace contract seed (rejected: delays proofable host-first progress and weakens iteration loop).

## Open questions

- Should kernel-side `vmo_destroy` closure be completed in the same milestone as cross-process proof, or remain a strict gate inside `TASK-0290` closure? (owner: runtime+kernel; decision by first `TASK-0031` implementation cut completion)

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

- [ ] **Phase 0**: typed userspace VMO API contract + host reject-path tests — proof: `cd /home/jenning/open-nexus-OS && cargo test -p nexus-vmo`
- [ ] **Phase 1**: cross-process transfer + deterministic QEMU marker proof — proof: `cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
- [ ] **Phase 2**: production-grade closure gates proven through `TASK-0290` (kernel seal/right enforcement + closure markers) — proof: `cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=210s ./scripts/qemu-test.sh`
- [ ] Task(s) linked with stop conditions + proof commands.
- [ ] QEMU markers (if any) appear in `scripts/qemu-test.sh` and pass.
- [ ] Security-relevant negative tests exist (`test_reject_*`).
