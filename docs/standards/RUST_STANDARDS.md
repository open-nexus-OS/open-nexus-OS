<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Rust Standards (Kernel + OS + Userspace)

**Status**: Active  
**Owners**: @kernel-team, @runtime  
**Created**: 2026-01-09  
**Purpose**: Define Rust best practices and layering rules so the codebase stays correct, auditable, and easy to evolve.

---

## Why this exists

Open Nexus OS is built to be **host-first, QEMU-last** and to stay **change-friendly**: foundations must be strict enough
to prevent drift, but flexible enough that adding features later does not require “fixes underneath”.

This document codifies:

- **Where `std` is preferred** and where `no_std` is required.
- **Lint + warning policy**, especially for the kernel.
- **Unsafe policy** and how to write auditable low-level Rust.
- A **layered code philosophy** (kernel vs libraries vs services vs tests).

---

## 1) `std` vs `no_std` (best practice)

### Rule 1.1 — Host-first domain code prefers `std`

- **Userspace domain libraries** in `userspace/` are expected to be **host-testable** and therefore typically use `std`.
- Benefits: better tooling, richer tests (property tests, Miri where applicable), easier refactors.

### Rule 1.2 — OS/QEMU paths require `no_std` (+ `alloc` when needed)

- **Kernel** and **OS/QEMU services** must compile in bare-metal mode: `no_std` and (only if required) `alloc`.
- Dependency hygiene rules are owned by **RFC‑0009**.

### Rule 1.3 — Feature gating is mandatory

- OS services must build with: `--no-default-features --features os-lite`.
- Any crate that supports both host and OS must clearly separate host (`std`) and OS (`os-lite`) paths.

---

## 2) Layered code philosophy (what belongs where)

### 2.1 Kernel (`source/kernel/neuron/`)

**Goal**: Minimal, deterministic, capability-driven kernel. No policy, no crypto, no protocol parsing.

- **Correctness first**: the kernel must be warning-clean in OS builds (`deny(warnings)` is intentional).
- **Determinism**: proofs are marker-driven; avoid timing-luck behavior.
- **Concurrency model**: follow the Servo-inspired ownership/message-passing guidance in `docs/architecture/16-rust-concurrency-model.md`.
- **No “business logic”**: kernel owns scheduling/vm/ipc/capability mechanics only.

### 2.2 Core libraries (contract crates)

- Prefer **small, composable crates** with explicit error types and bounded inputs.
- Default to `#![forbid(unsafe_code)]` unless the crate is explicitly a low-level backend.

### 2.3 OS services (`source/services/*d`)

**Goal**: Thin adapters over userspace libraries.

- No `unwrap`/`expect` on untrusted inputs.
- Markers must be “honest green”: `*: ready` only after real readiness; `SELFTEST: ... ok` only after real behavior.
- Heavy logic should move into `userspace/` crates that have host tests.

### 2.4 Userspace libraries (`userspace/`)

- Host-first: tests and determinism come first.
- OS backends should be explicit and must not “fake” OS support (return `Unsupported` deterministically unless implemented).

### 2.5 Tests

- Prefer host tests for behavior and negative cases.
- QEMU tests are bounded smoke checks with deterministic marker ordering (`scripts/qemu-test.sh`).

---

## 3) Kernel lint + warning policy (Rust-conform and change-friendly)

### Rule 3.1 — `deny(warnings)` in kernel OS builds is a feature, not a nuisance

We use warning-clean builds as a **drift detector**. Warnings in the kernel tend to indicate:

- dead code that hides incomplete plumbing,
- accidental feature-path changes,
- or incomplete refactors.

### Rule 3.2 — `dead_code` handling in kernel

`dead_code` is valuable, but there are legitimate bring-up phases where kernel-internal APIs are staged.

**Best practice order**:

1. **Prefer real usage** when it reflects actual invariants (best).
2. If usage would be artificial or would pull more code into the kernel, use a **targeted suppression** with a removal clause.

**Allowed form** (tight scope only):

- `#[allow(dead_code)]` on the **smallest item** (function/const), never the whole module, plus:
  - a short “why”, and
  - **REMOVE_WHEN(...)** clause referencing the owning task/landing point.

**Not allowed**:

- blanket `#![allow(dead_code)]` on kernel modules (except narrowly-scoped bring-up stubs where the module is explicitly
  tagged as such and scheduled for removal).

### Decision for the current `Asid` / `AsHandle` case

For `Asid::{from_raw, raw, KERNEL}` and `AsHandle::{from_raw, raw}`:

- **We choose targeted suppression + removal clause**, because “using them” right now would be artificial and risks
  changing kernel plumbing semantics prematurely.
- Removal trigger should be the address-space plumbing work described in `tasks/TASK-0011B-kernel-rust-idioms-pre-smp.md`
  and/or the AddressSpaceManager syscall wiring.

---

## 4) Unsafe policy (kernel and low-level backends)

### Rule 4.1 — Unsafe is permitted only where necessary, and must be auditable

- Prefer safe Rust. Use `unsafe` for:
  - MMIO/CSR reads/writes,
  - context switching,
  - page-table manipulation,
  - trap entry/exit glue.

### Rule 4.2 — Keep unsafe blocks small and document invariants

- “Small unsafe, big safe”:
  - Do the minimal raw pointer operation in `unsafe`,
  - immediately convert into safe types/structures.

### Rule 4.3 — No `unsafe impl Send/Sync` without a written safety argument

- Prefer deriving `Send/Sync` automatically where possible.
- If you must add `unsafe impl Send/Sync`, include a comment describing:
  - what data is shared,
  - what invariants make it safe,
  - and how it is enforced (types, ownership, or explicit synchronization).

---

## 5) Error handling and panics

- In kernel and OS services: prefer explicit error propagation.
- Avoid `unwrap`/`expect` on untrusted inputs.
- Panics are reserved for truly unreachable kernel invariants and should be rare; prefer “fail closed” behavior in
  security-sensitive paths.

---

## 6) References (project-local)

- Servo-inspired concurrency model: `docs/architecture/16-rust-concurrency-model.md`
- Host-first/QEMU-last testing and marker contract: `docs/architecture/02-selftest-and-ci.md`, `scripts/qemu-test.sh`
- no_std dependency hygiene contract: `docs/rfcs/RFC-0009-no-std-dependency-hygiene-v1.md`
