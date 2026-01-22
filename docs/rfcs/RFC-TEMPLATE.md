# RFC-XXXX: <Title>

- Status: Draft / In Progress / Complete
- Owners: @kernel-team / @runtime / @tools-team
- Created: YYYY-MM-DD
- Last Updated: YYYY-MM-DD
- Links:
  - Tasks: `tasks/TASK-XXXX-...md` (execution + proof)
  - ADRs: `docs/adr/XXXX-...md` (optional, decision records)
  - Related RFCs: `docs/rfcs/RFC-000Y-...md`

## Status at a Glance

- **Phase 0 (<short name>)**: ⬜ / 🟨 / ✅
- **Phase 1 (<short name>)**: ⬜ / 🟨 / ✅
- **Phase 2 (<short name>)**: ⬜ / 🟨 / ✅

Definition:

- “Complete” means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean “never changes again”.

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - <architecture decisions / stable contracts / invariants>
- **This RFC does NOT own**:
  - <explicitly out-of-scope topics that belong to other RFCs/tasks>

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define **stop conditions** and **proof commands**.
- This RFC must link to the task(s) that implement and prove each phase/milestone.

## Context

What problem exists today? Why now?

## Goals

- <goal 1>
- <goal 2>

## Non-Goals

- <non-goal 1>
- <non-goal 2>

## Constraints / invariants (hard requirements)

- **Determinism**: markers/proofs are deterministic; no timing-fluke “usually ok”.
- **No fake success**: never emit “ok/ready” markers unless the real behavior occurred.
- **Bounded resources**: explicit limits for buffers, loops, allocations, queue depth, etc.
- **Security floor**: list the security properties that must be true even in bring-up mode.
- **Stubs policy**: any stub must be explicitly labeled, non-authoritative, and must not claim success.

## Proposed design

### Contract / interface (normative)

Define the stable contract here (ABI, wire format, API, error model, etc.). If it is not stable yet,
say so explicitly and list the versioning strategy.

### Phases / milestones (contract-level)

Keep phases tied to contracts + proofs, not “implementation chores”.

- **Phase 0**: <minimal contract + proof gate>
- **Phase 1**: <hardening>
- **Phase 2**: <scalability/perf/feature>

## Security considerations

- **Threat model**: <spoofing, confused deputy, memory corruption, etc.>
- **Mitigations**: <capability gating, identity binding, bounds checks, provenance>
- **Open risks**: <explicit and tracked>

## Failure model (normative)

- Explicit error conditions and required behavior (errno mapping, retry safety, rollback semantics).
- “No silent fallback”: if a fallback exists, it must be explicit and proven.

## Proof / validation strategy (required)

List the canonical proofs; tasks must implement them.

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p <crate> <filter>
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

### Deterministic markers (if applicable)

- `...`

## Alternatives considered

- <alt 1> (why rejected)
- <alt 2> (why rejected)

## Open questions

- <question 1> (owner + decision deadline if any)

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

- [ ] **Phase 0**: <description> — proof: `<command>`
- [ ] **Phase 1**: <description> — proof: `<command>`
- [ ] **Phase 2**: <description> — proof: `<command>`
- [ ] Task(s) linked with stop conditions + proof commands.
- [ ] QEMU markers (if any) appear in `scripts/qemu-test.sh` and pass.
- [ ] Security-relevant negative tests exist (`test_reject_*`).
