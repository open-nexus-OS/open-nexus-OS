# RFC Process

1. Title your RFC `RFC-XXXX-short-title.md` with an incrementing number.
2. Describe the problem statement and constraints up front.
3. Outline the proposed design, alternatives, and validation strategy.
4. Document risks, mitigations, and test coverage expectations.
5. Submit a pull request and request subsystem maintainer reviews.

## Authority model (prevent drift)

We keep three document types with clear roles:

- **Tasks (`tasks/TASK-*.md`) are the execution truth**
  - They define **concrete work**, **stop conditions**, and **proof** (QEMU markers and/or `cargo test`).
  - They are updated as reality changes (new blockers, corrected scope, revised proof signals).
  - They must remain honest: no “fake success” markers; determinism rules apply.

- **RFCs (`docs/rfcs/RFC-*.md`) are design seeds / contracts**
  - They define **architecture decisions**, **interfaces/contracts**, and **what “stable” means** (if applicable).
  - They must not grow into a backlog tracker; link to tasks for implementation and evidence.
  - If a contract changes, update the RFC *and* link to the task/PR that proves it.

- **ADRs (`docs/adr/*.md`) are narrow decision records**
  - Use ADRs for “one decision, one rationale” when a change is too granular or too cross-cutting to
    live inside a single RFC without causing churn.

### Contradictions rule

- If a task and an RFC disagree on **architecture/contract**, treat the **RFC as authoritative** and update the task.
- If they disagree on **progress/plan/proof signals**, treat the **task as authoritative** and update the RFC only if the *contract* changed.

## RFC template (required structure)

Use the template when creating new RFCs:

- `docs/rfcs/RFC-TEMPLATE.md`

Hard requirements (agents should keep these current):

- **Status at a Glance** section near the top (phase-level progress).
- **Checklist** section at the end (checkboxes), focused on contract + proof gates, not “implementation chores”.

## Index

- RFC-0001: Kernel Simplification (Logic-Preserving)
  - docs/rfcs/RFC-0001-kernel-simplification.md
- RFC-0002: Process-Per-Service Architecture
  - docs/rfcs/RFC-0002-process-per-service-architecture.md
- RFC-0003: Unified Logging Infrastructure
  - docs/rfcs/RFC-0003-unified-logging.md
- RFC-0004: Loader Safety & Shared-Page Guards
  - docs/rfcs/RFC-0004-safe-loader-guards.md
- RFC-0005: Kernel IPC & Capability Model
  - docs/rfcs/RFC-0005-kernel-ipc-capability-model.md
