# RFC-0001: Kernel Simplification (Logic-Preserving)

- Status: Draft
- Owners: Kernel + Runtime Team
- Created: 2025-10-24

## Context
The NEURON kernel is functionally progressing but navigation and comprehension costs are high. This RFC proposes 10 logic-preserving measures to reduce complexity, improve agent/developer orientation, and surface test scope—without changing runtime behavior.

## Goals
- Faster orientation (clear responsibilities, fewer tokens to grok intent)
- Explicit invariants, dependencies, and test scope per module
- Logic-preserving refactors only; zero behavior changes

## Non-Goals
- Performance tuning, algorithmic changes, or broad API redesigns
- Immediate subcrate split; can be a later follow-up

## Relationship to upcoming kernel work (why now)

SMP bring-up and scheduling changes (see performance/power tasks) are high-debug-cost. This RFC is
intentionally **logic-preserving** and is meant to be executed as a preparatory “debugging window”
before behavioral kernel work lands. The execution checklist lives in a task file (below).

## Tracking (single source of truth)

- **Implementation checklist / execution**: `tasks/TASK-0011-kernel-simplification-phase-a.md`
- **Follow-up behavioral kernel work** (separate tasks, not part of this RFC):
  - SMP bring-up: `tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md`
  - QoS + timer coalescing: `tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md`

This RFC provides the rationale and the logic-preserving measures. The task is the authoritative
checklist, scope, and proof definition.

## Measures (1–10)
1) Module layering and naming
   - Adopt a clear responsibility taxonomy: arch, memory, process, comm, sched, core, utils, test.
   - Keep current file locations for now; only rename modules if strictly beneficial and trivial.
   - Enforce `pub(crate)` by default; open surface areas deliberately.

2) Standardized module headers (kernel-specific)
   - CONTEXT, OWNERS, STATUS, API_STABILITY, TEST_COVERAGE
   - KERNEL_INVARIANTS (W^X, user/kernel boundary, capability rights), PUBLIC API, DEPENDENCIES, ADR

3) Test documentation uplift
   - In all kernel tests: add TEST_SCOPE, TEST_SCENARIOS, KERNEL_TEST_DEPENDENCIES
   - Ensure counts in TEST_COVERAGE reflect reality or say "No tests"

4) Feature-flag grouping (naming only)
   - Group features by purpose (development/testing/boot)
   - No semantic changes to feature wiring; just nomenclature and docs

5) Error category unification (type aliases only)
   - Introduce umbrella error naming in docs (Memory, Process, Syscall, Hardware, IPC)
   - Do not change existing error enums yet; mapping documented for future alignment

6) Constants centralization (docs-first)
   - Document canonical constants (PAGE_SIZE, MAX_SYSCALL, USER_STACK_TOP, etc.)
   - Keep code as-is; centralization can be a later mechanical change

7) Architecture notes per subsystem
   - Short rationale per module: design, constraints, expected complexity
   - Links to ADRs where relevant

8) Performance characteristics (qualitative)
   - Document expected complexity (e.g., O(1) syscall dispatch), not microbenchmarks

9) Security invariants visibility
   - Explicitly list invariants enforced by each subsystem (W^X, rights checks, isolation)

10) Debug/diagnostics index
   - Enumerate debug features and their flags (debug_uart, stack guards, PT verify, trap symbols)
   - No default changes

## Migration plan

- Phase A (Text-only): **see TASK-0011** (authoritative checklist)
- Phase B (Optional, mechanical): visibility tightening, constant aliases, minor renames
- Phase C (Optional, higher cost): physical reorg or subcrates

## Risks
- Build churn: None in Phase A (text-only)
- Mislabeling status/coverage: mitigated via review checklist

## Validation
- All modules carry standardized headers
- Tests list scope/scenarios; coverage counts are accurate
- ADR references exist and resolve

## Open questions
- Do we want a prelude module for common kernel types?
- When to introduce subcrates for parallel build speed?

<!-- Userspace VFS Proof status moved to docs/status; RFC stays scope-limited. -->
