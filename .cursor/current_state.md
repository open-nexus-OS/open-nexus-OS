# Cursor Current State (SSOT)

## Current architecture state

- **last_decision (2026-04-24)**: `TASK-0045`/`RFC-0043` closed; active execution focus moved to `TASK-0046`/`RFC-0044`.
- **active boundary**: `TASK-0046` is config v1 contract and host-first implementation floor (no kernel changes).
- **gate tier**: Gate J (`production-floor`) per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`.

## Active focus (execution)

- **active_task**: `tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md` — `In Progress`
- **active_contract**: `docs/rfcs/RFC-0044-config-v1-configd-schema-layering-2pc-host-first-os-gated.md` — `In Progress`
- **active_proof_command (target)**:
  - `cd /home/jenning/open-nexus-OS && cargo test -p nexus-config -- --nocapture`
  - `cd /home/jenning/open-nexus-OS && cargo test -p configd -- --nocapture`
  - `cd /home/jenning/open-nexus-OS && cargo test -p nx -- --nocapture`

## Active constraints (TASK-0046)

- Kernel untouched.
- Deterministic config layering and bounded parsing/validation.
- Canonical contract boundary:
  - Cap'n Proto for canonical persisted/runtime snapshots,
  - JSON for authoring/validation and derived CLI/debug views only.
- No fake success:
  - reload success only after full prepare+commit path,
  - marker-only evidence is not sufficient for closure.
- CLI authority:
  - extend `tools/nx` via `nx config ...`,
  - no `nx-*` logic forks.

## Execution gates (TASK-0046)

- **Gate A (canonical config authority floor)**: YELLOW
  - Task/RFC contract is aligned; implementation pending.
- **Gate B (format authority floor)**: YELLOW
  - Cap'n Proto canonical snapshot contract declared; schema/encode proofs pending.
- **Gate C (proof quality floor)**: YELLOW
  - behavior-first host proof matrix defined; tests not yet green.
- **Gate D (CLI/no-drift floor)**: YELLOW
  - `nx config` contract specified; deterministic proof implementation pending.
- **Gate E (2PC honesty floor)**: YELLOW
  - anti-fake-success rules in place; state/result-correlated proofs pending.

## Closure todo list (TASK-0046 deltas)

- [ ] **C1** Add canonical Cap'n Proto effective snapshot schema under `tools/nexus-idl/schemas/`.
- [ ] **C2** Implement deterministic layering + validation + snapshot encode in `nexus-config`.
- [ ] **C3** Implement `configd` `GetEffective` (Cap'n Proto) and `GetEffectiveJson` (derived).
- [ ] **C4** Add 2PC apply/abort proofs with explicit unchanged-version assertions on reject.
- [ ] **C5** Extend `nx config` with deterministic exits and JSON/human output contract.

Closure evidence:

- `RFC-0044` contract seed created and linked.
- `TASK-0046` security, gate expectations, and follow-up expectation matrix synchronized.

## Required reject proofs (minimum floor)

- schema: unknown/type/depth/size rejects fail closed with stable non-zero classification.
- layering: deterministic precedence `defaults < /system < /state < env`.
- snapshot: equivalent inputs produce byte-identical Cap'n Proto effective snapshot.
- 2PC: any prepare reject/timeout aborts and keeps previous effective version active.
- CLI: `nx config` outputs deterministic exits and derived `--json` semantics.

## Follow-up split (preserve scope)

- `TASK-0047`: policy-as-code consumer cutover on top of `configd` contract.
- `TASK-0262`: determinism/hygiene floor alignment and anti-fake-success discipline.
- `TASK-0266`: single-authority and naming contract continuity.
- `TASK-0268`: `nx` convergence, no `nx-*` logic drift.
- `TASK-0273`: placeholder authority cleanup without parallel config authority.
- `TASK-0285`: QEMU harness phase/failure evidence discipline.

## Carry-over note

- `TASK-0045` and `RFC-0043` are closed and archived; no reopen implied by `TASK-0046` work.
