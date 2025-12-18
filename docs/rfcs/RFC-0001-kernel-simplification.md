# RFC-0001: Kernel Simplification (Logic-Preserving)

- Status: Draft
- Owners: Kernel + Runtime Team
- Created: 2025-10-24

Context
The NEURON kernel is functionally progressing but navigation and comprehension costs are high. This RFC proposes 10 logic-preserving measures to reduce complexity, improve agent/developer orientation, and surface test scope—without changing runtime behavior.

Goals
- Faster orientation (clear responsibilities, fewer tokens to grok intent)
- Explicit invariants, dependencies, and test scope per module
- Logic-preserving refactors only; zero behavior changes

Non-Goals
- Performance tuning, algorithmic changes, or broad API redesigns
- Immediate subcrate split; can be a later follow-up

Measures (1–10)
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

Migration Plan
- Phase A (Text-only): Headers, docs, RFC, cross-links, CODEOWNERS verification
- Phase B (Optional, mechanical): Visibility tightening, constant aliases, minor renames
- Phase C (Optional, higher cost): Physical reorg or subcrates

Risks
- Build churn: None in Phase A (text-only)
- Mislabeling status/coverage: mitigated via review checklist

Validation
- All modules carry standardized headers
- Tests list scope/scenarios; coverage counts are accurate
- ADR references exist and resolve

Open Questions
- Do we want a prelude module for common kernel types?
- When to introduce subcrates for parallel build speed?

<!-- Userspace VFS Proof status moved to docs/status; RFC stays scope-limited. -->
