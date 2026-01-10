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
  - **Scope rule (keep RFCs “100% done”)**:
    - Each RFC should be scoped so it can realistically reach **Status: Complete** as soon as its
      corresponding task slice(s) are done and proven.
    - If a follow-on task needs new behavior beyond the existing RFC scope, create a **new RFC**
      (a new “contract seed”) instead of extending an old RFC into a multi-phase backlog.
    - When we intentionally defer a capability (e.g. “real subnet discovery”), the current RFC must
      state that it is **out of scope** and that a **new RFC** will define the next contract when scheduled.
  - If a contract changes, update the RFC *and* link to the task/PR that proves it.

- **ADRs (`docs/adr/*.md`) are narrow decision records**
  - Use ADRs for “one decision, one rationale” when a change is too granular or too cross-cutting to
    live inside a single RFC without causing churn.

### Contradictions rule

- If a task and an RFC disagree on **architecture/contract**, treat the **RFC as authoritative** and update the task.
- If they disagree on **progress/plan/proof signals**, treat the **task as authoritative** and update the RFC only if the *contract* changed.

### “Contract seed” rule for follow-on tasks

- Follow-on tasks MUST NOT silently expand old RFC scopes.
- If a follow-on task requires new contracts, add a new RFC (or ADR if it’s a narrow decision),
  link it from the new task, and keep the previous RFC marked **Complete**.

## RFC template (required structure)

Use the template when creating new RFCs:

- `docs/rfcs/RFC-TEMPLATE.md`

Hard requirements (agents should keep these current):

- **Status at a Glance** section near the top (phase-level progress).
- **Checklist** section at the end (checkboxes), focused on contract + proof gates, not "implementation chores".

## Security-relevant RFCs

RFCs touching crypto, auth, identity, capabilities, or sensitive data MUST include:

1. **Threat model**: What attacks are relevant?
2. **Security invariants**: What MUST always hold?
3. **DON'T DO list**: Explicit prohibitions
4. **Proof strategy**: How security is verified (negative tests, hardening markers)

See `docs/standards/SECURITY_STANDARDS.md` for detailed guidelines.

**Security RFCs in this repo:**


- RFC-0005: Kernel IPC & Capability Model (capability-based security)
- RFC-0008: DSoftBus Noise XK v1 (authentication + identity binding)
- RFC-0009: no_std Dependency Hygiene v1 (build security)

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
- RFC-0006: Userspace Networking v1 (virtio-net + smoltcp + sockets facade)
  - docs/rfcs/RFC-0006-userspace-networking-v1.md
- RFC-0007: DSoftBus OS Transport v1 (UDP discovery + TCP sessions over sockets facade)
  - docs/rfcs/RFC-0007-dsoftbus-os-transport-v1.md
- RFC-0008: DSoftBus Noise XK v1 (no_std handshake + identity binding)
  - docs/rfcs/RFC-0008-dsoftbus-noise-xk-v1.md
