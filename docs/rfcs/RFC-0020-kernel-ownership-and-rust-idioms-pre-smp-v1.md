<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# RFC-0020: Kernel ownership + Rust idioms pre-SMP v1 (logic-preserving)

- Status: Complete
- Owners: @kernel-team, @runtime
- Created: 2026-02-09
- Last Updated: 2026-02-10
- Links:
  - Tasks: `tasks/TASK-0011B-kernel-rust-idioms-pre-smp.md` (execution + proof)
  - Related RFCs:
    - `docs/rfcs/RFC-0001-kernel-simplification.md` (stable kernel taxonomy + layout contract; implemented by TASK-0011)
    - `docs/rfcs/RFC-0013-boot-gates-readiness-spawn-resource-v1.md` (marker/readiness discipline; determinism)
    - `docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md` (host-first + phased QEMU smoke)
  - Standards:
    - `docs/standards/RUST_STANDARDS.md` (kernel Rust policy: unsafe discipline, newtypes, ownership docs)

## Status at a Glance

- **Phase 0 (Ownership model documented)**: ✅
- **Phase 1 (Kernel handle newtypes)**: ✅
- **Phase 2 (Explicit Send/Sync boundaries)**: ✅
- **Phase 3 (Kernel error envelope + `#[must_use]`)**: ✅
- **Phase 4 (Capability type-safety wrappers)**: ✅
- **Phase 5 (IPC/cap transfer semantics — internal prep, no ABI change)**: ✅

Definition:

- “Complete” means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean “never changes again”.

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - The **kernel ownership model** documentation contract (what must be explicit, where it lives).
  - A **zero-cost newtype contract** for kernel identifiers/handles to prevent type confusion (pre-SMP correctness).
  - A **pre-SMP concurrency boundary contract**: which kernel structures are explicitly `!Send/!Sync` today, and how we make that intent mechanically obvious.
  - A **kernel error envelope contract** (within the kernel crate) that makes error propagation explicit and hard to ignore (`#[must_use]`), without changing syscall ABI/errno semantics.
  - A **minimal capability type-safety contract** (phantom-typed wrappers) that prevents category confusion at compile-time while preserving existing runtime checks and ABI.
  - An **IPC/cap transfer semantics prep contract** that makes ownership transfer intent explicit in internal APIs while keeping the external syscall ABI and behavior unchanged (copy remains the syscall-level behavior in v1).
  - The rule that the above must be **logic-preserving**, **ABI-stable**, and **marker-stable**.
- **This RFC does NOT own**:
  - SMP implementation itself (owned by `tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md`).
  - Syscall ABI changes (numbers/layouts/errno semantics).
  - Introducing locks or changing scheduling policy/semantics (only make ownership + boundaries explicit).
  - Broad rewrites of subsystem APIs that are not directly justified by phases 0–5.

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define **stop conditions** and **proof commands**.
- This RFC links to the task that implements and proves each phase/milestone.

## Context

After TASK-0011 stabilized the kernel’s physical layout and documentation headers, the next high-leverage step before SMP is to make “Rust-specific correctness” more explicit:

- Ownership and lifetimes in the kernel should be clear from types + docs, not tribal knowledge.
- Handle confusion bugs (mixing `Pid`, capability slots, address-space handles, etc.) become more likely as code grows and becomes concurrent.
- SMP bring-up will force concurrency boundaries; we should mark those boundaries intentionally now, without changing runtime behavior.
- SMP also benefits from a scheduler that can be split cleanly: the pre-SMP types should make it obvious which state must become per-CPU (e.g. runqueues) and which state remains globally coordinated.

This RFC defines a minimal, reviewable contract for that preparation.

## Goals

- Make kernel ownership boundaries **explicit and auditable**.
- Prevent common pre-SMP correctness bugs via **compile-time types** (newtypes for handles).
- Make concurrency boundaries **explicit** without introducing locks or behavior changes.
- Make scheduler-related ownership and thread-boundary assumptions explicit so TASK-0012 can introduce per-CPU runqueues without untangling implicit sharing.
- Keep changes **mechanical** and **logic-preserving**.

## Non-Goals

- Implement SMP, IPIs, per-CPU runqueues (TASK-0012).
- Change syscall return conventions (still `-errno` in `a0`).
- Introduce new kernel dependencies or feature wiring.
- Change scheduler behavior/policy (only clarify ownership/types).

## Constraints / invariants (hard requirements)

- **Logic-preserving**: no runtime behavior change; QEMU output marker strings stay identical.
- **ABI stability**: syscall numbers, struct layouts, and errno semantics remain unchanged.
- **Determinism**: proof runs remain deterministic; no new unbounded loops/waits introduced.
- **No fake success**: do not change readiness/ok marker semantics to “paper over” issues.
- **Unsafe discipline**:
  - No new `unsafe` unless unavoidable and accompanied by a written safety argument.
  - No `unsafe impl Send/Sync` without a written safety argument.

## Proposed design

### Contract / interface (normative)

#### 1) Ownership model documentation (normative)

The kernel must have a single place that answers “who owns what?” at subsystem granularity.

- **Document location (normative)**: `docs/architecture/01-neuron-kernel.md`
- **Minimum required content**:
  - global kernel state shape (what is initialized once, what is mutated during syscalls)
  - ownership of core subsystems (`Scheduler`, `TaskTable`, `IpcRouter`, `AddressSpaceManager`)
  - lifetime model (what is `'static`, what is stack-borrowed per syscall/trap)
  - explicit statement: **no shared mutable state without synchronization** (and what synchronization primitives exist today)
  - scheduler-specific ownership:
    - pre-SMP: `Scheduler` is mutated on one core only; state is not designed for cross-thread mutation
    - identify which scheduler state is intended to become per-CPU in TASK-0012 (e.g. runqueues) versus what is conceptually global (e.g. PID allocation / global task table)

This is documentation-only but is treated as part of the contract for subsequent SMP work.

#### 2) Kernel handle newtypes (normative)

All kernel identifiers/handles that are currently plain integers and are meaningfully distinct MUST be wrapped in `#[repr(transparent)]` newtypes, with explicit conversions.

Minimum required set (v1):

- `Pid` (task identifier)
- `CapSlot` (capability table index)
- `AsHandle` (address-space handle / opaque identifier)

Contract:

- Newtypes MUST be:
  - `Copy`, `Eq`, `Ord`, `Hash`, `Debug`
  - `#[repr(transparent)]` over the underlying integer (zero-cost)
  - constructed only via `from_raw`-style functions (kernel-internal visibility)
  - converted back via `as_raw` (kernel-internal visibility)
- Newtypes MUST NOT change ABI surfaces:
  - syscall boundary still encodes/decodes raw integers as before
  - marker strings unchanged

Canonical location:

- `source/kernel/neuron/src/types.rs` (or a small, stable types module that is imported explicitly).

#### 3) Explicit Send/Sync boundaries (normative)

Before SMP, the kernel is effectively single-core, but we want future SMP work to be forced to “touch the right places”.

Contract:

- Types that MUST NOT cross thread boundaries in v1 (e.g. scheduler structures that assume single-core mutation) should be made explicitly `!Send` and `!Sync` by construction.
- Prefer **safe, stable** patterns:
  - `PhantomData<*mut ()>` or similar marker fields to make the type `!Send/!Sync` without `unsafe`.
  - Negative impls (`impl !Send for T {}` / `impl !Sync for T {}`) are acceptable only if they do not introduce toolchain/edition constraints for the kernel build; otherwise default to the PhantomData marker.
- Types that are immutable-after-init should remain `Send`/`Sync` by default (auto traits); avoid `unsafe impl Send/Sync` unless unavoidable.

#### 4) Kernel error envelope + `#[must_use]` (normative)

The kernel must make error propagation explicit and hard to ignore, without changing syscall ABI.

Contract:

- All newly introduced or migrated kernel-internal error enums/structs used in `Result<T, E>` MUST be annotated `#[must_use]`.
- The kernel MUST have a small “syscall error envelope” type that:
  - preserves the existing `-errno` mapping at the syscall boundary,
  - carries enough internal structure to avoid ad-hoc `i64`/`isize` error plumbing in the implementation,
  - remains internal to the kernel crate (no ABI impact).
- “Mechanical” means:
  - refactors change types and plumbing but do not change which errno is returned for existing failure cases,
  - QEMU markers remain unchanged.

#### 5) Capability type-safety wrappers (normative)

Capabilities are central to correctness. We want to eliminate category confusion without changing runtime semantics.

Contract:

- Introduce minimal phantom-typed wrappers for capability identifiers/handles where they reduce confusion (e.g. `Cap<T>` or `CapId<T>` where \(T\) is a zero-sized marker type).
- The wrappers MUST be zero-cost and must not change table layout or syscall encoding.
- Existing runtime rights checks and validation MUST remain in place; the typing is additive safety, not a replacement for checks.

#### 6) IPC/cap transfer semantics — internal prep (normative)

We want internal APIs that make ownership intent explicit, but v1 must not change syscall ABI or observable behavior.

Contract:

- Syscall-level cap transfer remains “copy” behavior in v1 (no new flags, no ABI change).
- Internally, it is permitted to:
  - introduce explicit internal enums/markers that describe intended transfer mode (e.g. `Copy` vs “logically moved”) as long as the syscall boundary still behaves as copy,
  - restructure internal code so that a future ABI extension can be implemented locally (e.g. concentrating transfer logic behind one function).
- Any “move semantics” in v1 are therefore **representational/architectural only** (clarity + future-proofing), not a user-visible behavior change.

### Phases / milestones (contract-level)

- **Phase 0 (Ownership model documented)**:
  - Contract: ownership section exists and is accurate.
  - Proof: docs review + no code/proof regression expected.
- **Phase 1 (Kernel handle newtypes)**:
  - Contract: newtypes exist; mechanical call-site updates; no ABI change.
  - Proof: task proof gates (below).
- **Phase 2 (Explicit Send/Sync boundaries)**:
  - Contract: concurrency boundaries are explicit (marker fields / negative impls) with rationale.
  - Proof: task proof gates (below).
- **Phase 3 (Kernel error envelope + `#[must_use]`)**:
  - Contract: kernel-internal error envelope exists; `#[must_use]` applied per contract; errno mapping unchanged.
  - Proof: task proof gates (below).
- **Phase 4 (Capability type-safety wrappers)**:
  - Contract: minimal phantom wrappers exist; call sites updated mechanically; runtime checks unchanged.
  - Proof: task proof gates (below).
- **Phase 5 (IPC/cap transfer semantics — internal prep, no ABI change)**:
  - Contract: internal transfer logic centralized/explicit; syscall behavior unchanged.
  - Proof: task proof gates (below).

## Security considerations

- **Threat model**:
  - Future SMP work introduces data races if ownership/concurrency boundaries are implicit.
  - Handle confusion causes capability misuse or memory safety errors as code grows.
- **Mitigations**:
  - Newtypes eliminate entire classes of “wrong integer” bugs at compile time.
  - Explicit `!Send/!Sync` boundaries force future SMP work to design proper sharing/synchronization.
  - Ownership docs reduce incorrect assumptions about who may mutate which structure.
- **Open risks**:
  - Over-eager concurrency annotations could block legitimate refactors; mitigate by keeping Phase 2 minimal and well-justified.

## Failure model (normative)

- If any change alters:
  - QEMU marker strings or their ordering,
  - syscall ABI (numbers/layouts/errno),
  - or introduces new behavioral differences,
  then it is out of scope for v1 and must be split into a separate task/RFC.

## Proof / validation strategy (required)

Proofs are owned by TASK-0011B. Canonical commands:

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test --workspace
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh
```

### Proof (OS compile slice)

```bash
cd /home/jenning/open-nexus-OS && just diag-os
```

## Alternatives considered

- **Do SMP first, clean up later**:
  - Rejected: SMP multiplies the cost of unclear ownership/handle confusion and increases regression risk.
- **Defer error envelope / typed caps / transfer prep**:
  - Rejected: these are still compatible with “mechanical, logic-preserving” work and directly reduce SMP migration risk (fewer ambiguous integers/paths in scheduler/IPC code).

## Open questions

- Should `types.rs` remain at crate root (ubiquitous dependency) or move under `core/`? (Related: RFC-0001 open question.)
- Should we standardize on negative impls vs PhantomData markers for `!Send/!Sync` in the kernel toolchain slice?
- What is the minimal `AsHandle`/`Asid` naming that stays honest about hardware vs logical handle?
- For Phase 5: what is the smallest internal API surface that centralizes transfer logic without touching syscall ABI?

---

## Implementation Checklist

- [x] **Phase 0**: Ownership model documented — proof: docs diff + review
- [x] **Phase 1**: Kernel handle newtypes — proof: `cargo test --workspace` + `just diag-os` + QEMU marker contract
- [x] **Phase 2**: Explicit Send/Sync boundaries — proof: `cargo test --workspace` + `just diag-os` + QEMU marker contract
- [x] **Phase 3**: Kernel error envelope + `#[must_use]` — proof: `cargo test --workspace` + `just diag-os` + QEMU marker contract
- [x] **Phase 4**: Capability type-safety wrappers — proof: `cargo test --workspace` + `just diag-os` + QEMU marker contract
- [x] **Phase 5**: IPC/cap transfer semantics (internal prep, no ABI change) — proof: `cargo test --workspace` + `just diag-os` + QEMU marker contract
- [x] Task linked with stop conditions + proof commands (`tasks/TASK-0011B-kernel-rust-idioms-pre-smp.md`)
