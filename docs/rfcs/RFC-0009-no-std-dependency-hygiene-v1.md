# RFC-0009: no_std Dependency Hygiene v1 (OS/QEMU reproducible builds)

- Status: Complete (Phase 0 + 1 + 2)
- Owners: @runtime / @tools-team
- Created: 2026-01-07
- Last Updated: 2026-01-07 (Phase 2 complete)
- Links:
  - Tasks:
    - `tasks/TASK-0003C-dsoftbus-udp-discovery-os.md` (OS networking bring-up surfaced the failure mode)
    - `tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md` (OS/QEMU slice, proof markers)
  - ADRs (optional):
    - `docs/adr/0016-kernel-libs-architecture.md` (if we decide to formalize “OS support crates” as kernel-libs boundary)
  - Related RFCs:
    - `docs/rfcs/RFC-0006-userspace-networking-v1.md`
    - `docs/rfcs/RFC-0007-dsoftbus-os-transport-v1.md`
    - `docs/rfcs/RFC-0008-dsoftbus-noise-xk-v1.md`

## Status at a Glance

- **Phase 0 (Contract + enforcement gates)**: ✅ (contract defined; enforcement implemented via Makefile)
- **Phase 1 (Locking + RNG policy)**: ✅ (parking_lot excluded; RNG via deterministic seeds)
- **Phase 2 (CI + anti-drift hardening)**: ✅ (`just dep-gate` implemented; fails if forbidden crates appear)

Definition:

- “Complete” means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean “never changes again”.

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - The **normative rules** for what may appear in the **OS/QEMU (bare-metal) dependency graph**.
  - The **normative policy** for **randomness/RNG** usage in OS bring-up and in production custody.
  - The **normative policy** for **locking/synchronization primitives** in OS (no_std) code.
  - The **proof gates** that must be green before any task can claim “Done” for OS/QEMU networking/security milestones.
- **This RFC does NOT own**:
  - DSoftBus discovery protocol details (RFC-0007).
  - Noise handshake details (RFC-0008).
  - Kernel syscall ABI semantics (RFC-0005).
  - Implementation steps for a specific service crate (belongs in tasks).

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define **stop conditions** and **proof commands**.
- This RFC must link to the task(s) that implement and prove each enforcement gate.

## Context

We explicitly want a “clean base” where:

- `just diag-os` and OS/QEMU builds are **reproducible** on a **fresh machine** (no cache luck).
- OS/QEMU crates are **no_std + alloc** (where required) and do not accidentally pull host-only dependencies.
- Diagnostics and proof markers are **honest green**: no “works on my machine”, no fake-success markers.

During OS/QEMU networking work (DSoftBus discovery + sessions), a fresh build surfaced a class of failures where
**host-oriented crates** (or `std`-dependent feature paths) leak into the **bare-metal target graph**, causing:

- compile-time aborts (e.g. `getrandom` “target not supported”),
- “missing prelude” symptoms (e.g. `Box`, `Vec`, `Ok/Err`, `FnOnce` not resolved) from crates that are being built in an
  unexpected feature mode for no_std.

This class of failure must be prevented permanently. Otherwise, tasks cannot truthfully reach “100% Done” because the
baseline is not reproducible.

## Goals

- **G1: Reproducible OS/QEMU builds**: `cargo build --target riscv64imac-unknown-none-elf` succeeds from a clean state.
- **G2: No host-only dependency leaks**: OS/QEMU graph must not include crates that require `std` (unless explicitly allowed by this RFC).
- **G3: Deterministic bring-up by default**: bring-up flows use deterministic keys/seeds unless explicitly doing secure custody work.
- **G4: Sustainable anti-drift rules**: make it hard to reintroduce `std`/randomness/locking leaks later.

## Non-Goals

- Defining the final production key custody implementation (owned by identity/keystore RFCs/tasks).
- Optimizing locking primitives for peak performance (first we need correctness + reproducibility).
- Making every existing crate no_std immediately (this RFC defines policy + enforcement; migration is task-driven).

## Constraints / invariants (hard requirements)

- **Determinism**: proof markers and tests must not rely on timing luck or caches.
- **No fake success**: never emit `*: ready` / `SELFTEST: ... ok` unless real behavior occurred.
- **Bounded resources**: OS/QEMU code must have explicit bounds (buffers/loops/allocations).
- **Security floor**:
  - Bring-up keys must be labeled **TEST ONLY / NOT SECURE**.
  - “Randomness” must not silently degrade to a weak source without explicit labeling and proof.
- **Stubs policy**: any stub must be explicit, non-authoritative, and must not claim success.

## Proposed design

### Contract / interface (normative)

#### 1) OS/QEMU dependency hygiene contract

For crates compiled under `cfg(all(target_os = "none", target_arch = "riscv64"))` (our OS/QEMU slice):

- **Rule D1 (no_std baseline)**: Crates MUST compile with `#![no_std]` (and may use `extern crate alloc` if needed).
- **Rule D2 (no implicit std)**: Default features must not pull `std` transitively. If a dependency has `std` default,
  we must use `default-features = false` and explicitly select the no_std feature set.
- **Rule D3 (no host-only crates)**: The OS graph MUST NOT include crates whose primary contract is “host OS integration”
  (examples include platform RNG, filesystem IO, thread parking, etc.) unless explicitly permitted by an allowlist.
- **Rule D4 (explicit allowlist)**: If an exception is necessary, it must be recorded as:
  - a named allowlist entry,
  - the reason and scope,
  - and a proof gate that ensures it does not regress elsewhere.

#### 2) RNG policy contract (bring-up vs production)

- **Rule R1 (bring-up determinism)**: In bring-up milestones, cryptographic keys/seeds may be deterministic for
  reproducibility, but MUST be:
  - clearly labeled as **TEST ONLY / NOT SECURE** in code and docs,
  - parameterizable (e.g. per node/port) to unblock multi-node tests,
  - never reused as “production custody”.
- **Rule R2 (no `getrandom` in OS bring-up)**: OS/QEMU bring-up MUST NOT rely on `getrandom` or ambient randomness.
  Any entropy requirement must be satisfied explicitly (deterministic seed for bring-up, or secure custody service).
- **Rule R3 (production custody source)**: When we move to production custody, entropy must come from:
  - `keystored` / `identityd`, or
  - a kernel-provided RNG API (if we add one),
  and must be policy-gated and auditable.

#### 3) Locking/synchronization policy contract

- **Rule L1 (no `parking_lot` in OS)**: OS/QEMU code MUST NOT depend on `parking_lot` / `parking_lot_core`.
  (Rationale: these crates assume host-threading/parking mechanisms and frequently require `std`/platform hooks.)
- **Rule L2 (blessed primitives)**: OS/QEMU locking must use one of:
  - a minimal no_std lock (spin/critical-section style), or
  - a project-owned OS support crate (preferred) with a documented failure model.
- **Rule L3 (no hidden blocking)**: Locks must not introduce hidden unbounded waits in OS paths; if blocking exists,
  it must be explicit and proven (and should normally use `yield_()` loops with bounds in bring-up).

### Phases / milestones (contract-level)

- **Phase 0: Contract + enforcement gates**
  - Define the allowlist/denylist rules (D1–D4, R1–R3, L1–L3).
  - Add a deterministic proof gate that fails if forbidden crates appear in the OS graph.
- **Phase 1: Locking + RNG policy adoption**
  - Ensure all OS/QEMU crates satisfy no_std feature selection rules.
  - Remove/replace any forbidden dependencies from the OS graph.
  - Standardize bring-up RNG usage and labeling.
- **Phase 2: CI + anti-drift hardening**
  - CI checks enforce:
    - OS graph is clean (no forbidden crates),
    - `just diag-os` is reproducible from clean state,
    - marker order contracts stay stable.

## Security considerations

- **Threat model**:
  - Accidental weakening of crypto due to “fallback randomness” (e.g. weak/zero RNG).
  - Undetected introduction of host dependencies leading to “green locally, broken fresh”.
  - Confused-deputy: ambient host facilities accidentally reachable in OS mode.
- **Mitigations**:
  - Explicit RNG policy (R1–R3), no `getrandom` in OS bring-up (R2).
  - Locking policy forbidding `parking_lot` (L1) and requiring explicit bounded behavior (L3).
  - Deterministic dependency-graph proof gate (Phase 0/2).
- **Open risks**:
  - Selecting/standardizing a single OS locking primitive needs careful review (fairness, IRQ-safety, deadlock patterns).
  - Production RNG interface needs coordination with identity/keystore roadmap.

## Failure model (normative)

- If an OS build would pull a forbidden crate, the build MUST fail with a deterministic error message from the proof gate.
- If a crate requires randomness in OS bring-up, it MUST fail deterministically unless an explicit bring-up seed is provided.
- No silent fallback to host-only behavior is allowed under `target_os = "none"`.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && just diag-host
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && just diag-os
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s ./scripts/qemu-test.sh
```

### Deterministic dependency graph gate (required)

The project has a canonical command that fails if forbidden crates appear in the OS target graph:

```bash
cd /home/jenning/open-nexus-OS && just dep-gate
```

**Implementation**: `just dep-gate` iterates over all OS services, runs `cargo tree --target riscv64imac-unknown-none-elf --no-default-features --features os-lite`, and greps for forbidden crates (`parking_lot`, `parking_lot_core`, `getrandom`).

Required properties (verified):

- ✅ deterministic output
- ✅ runs on a fresh machine without network access beyond local toolchain
- ✅ fails on reintroduction of forbidden crates (`parking_lot(_core)`, `getrandom`, and any future additions)

## Alternatives considered

- **A1: Revert to a “known good” lockfile/commit**: rejected because it does not prevent recurrence and violates the
  “fix, don’t revert” sustainability requirement.
- **A2: Enable `-Z build-std` for bare-metal**: rejected as default because it expands the trusted computing base,
  increases drift risk, and moves us away from a clean no_std OS slice.
- **A3: Allow `parking_lot`/`getrandom` under OS with ad-hoc features**: rejected because it creates fragile feature
  matrices and is likely to regress on fresh machines.

## Open questions

- **Q1 (owner: @tools-team)**: What is the canonical "graph gate" tool? (`cargo tree` parsing vs a dedicated tool crate)
  - *RESOLVED*: `just diag-os` runs `cargo check --target riscv64imac... --no-default-features --features os-lite` and fails if forbidden crates appear.
- **Q2 (owner: @runtime)**: What is the blessed OS lock primitive? (spin/critical-section vs project-owned `nexus-sync`)
  - *RESOLVED*: `nexus-sync` (project-owned) is the blessed primitive. `parking_lot` is explicitly forbidden via feature-gating.
- **Q3 (owner: @runtime)**: Where does production entropy live first: `keystored` vs kernel RNG API?
  - *OPEN*: RFC-0008 Phase 2 defines `keystored` as the first production custody source.

## Implementation Notes (2026-01-07)

### Root Cause
The Makefile built OS services (`samgrd`, `dsoftbusd`, etc.) without `--no-default-features --features os-lite`, causing `std`-dependent crates (`parking_lot`, `getrandom`) to leak into the bare-metal dependency graph.

### Solution
1. **Makefile fixed**: All OS services are now built with `--no-default-features --features os-lite`.
2. **justfile fixed**: `diag-os` now includes `policyd` in the OS services list.
3. **Services without `os-lite` excluded**: `identityd`, `dist-data`, `clipboardd`, `notifd`, `resmgrd`, `searchd`, `settingsd`, `time-syncd` are not OS-ready and are excluded from the OS build.
4. **Consistency verified** (all three now aligned):
   - `Makefile`: `samgrd bundlemgrd dsoftbusd execd keystored netstackd packagefsd policyd vfsd`
   - `justfile diag-os`: same list
   - `scripts/run-qemu-rv64.sh DEFAULT_SERVICE_LIST`: same list + selftest-client
5. **Proof gates green**:
   - `just diag-os` ✅
   - `just diag-host` ✅
   - `just dep-gate` ✅ (RFC-0009 Phase 2 enforcement)
   - QEMU markers: `dsoftbusd: auth ok`, `SELFTEST: dsoftbus ping ok` ✅

## Checklist (keep current)

- [x] Scope boundaries are explicit; cross-RFC ownership is linked.
  - See "Scope boundaries (anti-drift)" section; cross-RFC links in header.
- [x] Task(s) exist for each milestone and contain stop conditions + proof.
  - TASK-0003C linked; stop conditions defined in "Proof / validation strategy".
- [x] Proof is "honest green" (markers/tests), not log-grep optimism.
  - `just diag-os` ✅, `just diag-host` ✅, `just dep-gate` ✅, QEMU markers ✅
- [x] Determinism + bounded resources are specified.
  - See "Constraints / invariants" section.
- [x] Security invariants are stated and have at least one regression proof.
  - See "Security considerations" section; `just dep-gate` is the regression proof.
- [x] If claiming stability: ABI/on-wire vectors + layout/compat tests exist.
  - N/A: This RFC defines policy, not wire formats.
- [x] Stubs (if any) are explicitly labeled and non-authoritative.
  - No stubs in this RFC; stubs policy defined in "Constraints / invariants".
