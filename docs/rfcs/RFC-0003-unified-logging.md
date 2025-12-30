# RFC-0003: Unified Logging Infrastructure

- Status: Complete (Phase 0: deterministic bring-up markers + guards; later phases deferred)
- Authors: Runtime Team
- Last Updated: 2025-12-30

## Status at a Glance

- **Phase 0 (Deterministic bring-up logging + guards)**: Complete ✅
- **Phase 1+ (Kernel parity / routing / buffers)**: Deferred (tracked in tasks; out of scope for this RFC)

Definition:

- This RFC is “Complete” when the Phase 0 contract is true in the repo: deterministic markers exist,
  no-fake-success discipline is enforced, and the logging guard floor prevents provenance-unsafe
  slices from crashing bring-up paths.
- Any later “full logging control plane” work is intentionally deferred and must be tracked as tasks
  (and potentially a follow-up RFC) to avoid drift.

## Role in the RFC stack (how RFC-0003 fits with RFC-0004/0005)

RFC‑0003 is **supporting infrastructure**, not a blocker for RFC‑0004/0005:

- **RFC‑0004 depends on RFC‑0003 (minimal)** for provenance-safe, deterministic logging so guard
  violations can be surfaced without dereferencing untrusted pointers.
- **RFC‑0005 depends on RFC‑0003 (minimal)** for honest QEMU markers and actionable diagnostics
  during IPC/policy/routing bring-up.

Practical rule:

- We implement only what we need for **deterministic markers + guard visibility**, and defer the
  “full logging control plane” (buffers/routing) unless it unblocks security or correctness.

### Scope boundary (anti-drift) + TASK-0002 note

RFC‑0003 is intentionally **supporting infrastructure**.

- **RFC‑0003 owns**: the logging facade/sinks and the “no fake success / deterministic marker” discipline.
- **RFC‑0003 does NOT own**:
  - process architecture / init/service topology (RFC‑0002),
  - loader safety invariants (RFC‑0004),
  - IPC/capability contracts (RFC‑0005),
  - hardware bring-up, persistence, OOM, or driver/accelerator planning (tracked in `tasks/`).

TASK‑0002 (Userspace VFS Proof) only depends on RFC‑0003 for deterministic markers and provenance-safe logging/guards.

### Relationship to tasks (execution truth)

- Tasks (`tasks/TASK-*.md`) define **stop conditions + proof**.
- This RFC defines the logging contract/constraints; tasks implement and prove it.

“Done enough for now” criteria:

- Core services and init-lite can emit deterministic markers without duplicated helpers.
- Logging guards remain strict (reject non-provenance-safe pointers deterministically).

## Summary

Neuron currently mixes several ad-hoc logging styles (raw UART loops in the
kernel, `debug_putc` wrappers in userspace, temporary panic prints). This RFC
proposes a unified `nexus-log` facade that provides consistent severity/target
semantics across domains, predictable fallbacks during bring-up, and room for
future routing (buffers, tracing).

## Motivation

- **Consistency** – today each component formats its own strings, which makes
  enable/disable switches and tooling brittle.
- **Determinism** – panic paths often fall back to bespoke UART loops; we need a
  shared fallback story that avoids touching allocators or `fmt` when the world
  is on fire.
- **Observability control** – logs grow noisy during bring-up; we need
  compile-time defaults and runtime knobs to focus on the domain under
  investigation.
- **Long-term tooling** – structured logging (key/value, ring buffers) depends
  on a single choke-point that can evolve without auditing every caller.

## Goals

1. Provide a single `no_std` crate (`nexus-log`) with:
   - `Level` abstraction (Error/Warn/Info/Debug/Trace)
   - Domain/target tagging (`[LEVEL target] payload`)
   - Minimal runtime configuration (global max level, later target masks)
2. Separate sinks for kernel and userspace while keeping the API identical.
3. Guarantee a panic-safe path (raw UART) that does not allocate or touch
   `core::fmt` if the caller does not request it.
4. Allow callers to compose lines without relying on trait objects; still expose
   an escape hatch for formatted arguments when safe.
5. Document phased adoption so we can incrementally port existing sites without
   derailing current debugging efforts.

### Non-goals (for this RFC iteration)

- Structured/JSON logging.
- Asynchronous draining or per-core ring buffers (tracked as future work).
- Automatically rewriting existing macros (e.g. `log_info!`) in one sweep.

## Checklist (complete)

- [x] Deterministic marker discipline is documented (“no fake success”).
- [x] Minimal logging facade exists and is usable from bring-up paths without duplicating helpers.
- [x] Guard expectations are explicit and aligned with loader provenance constraints (RFC‑0004).
- [x] Later “full logging control plane” work is explicitly deferred to tasks (no drift).
