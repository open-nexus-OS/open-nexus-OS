# Next Task Preparation (Drift-Free)

## Candidate next execution

- **task**: `tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md` — `In Review`
- **contract seed (RFC)**: `docs/rfcs/RFC-0040-zero-copy-vmos-v1-plumbing-host-first-os-gated.md` — `Done`
- **closure gate task**: `tasks/TASK-0290-kernel-zero-copy-closure-v1b-vmo-seals-reuse-truth.md` — `Draft`
- **tier**: production-grade trajectory per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate A + Gate C relevant closure obligations)

## Drift check vs `current_state.md`

- [x] `TASK-0029`/`RFC-0039` are done and archived from active execution scope.
- [x] `TASK-0031` header links include the new seed RFC (`RFC-0040`).
- [x] `RFC-0040` explicitly delegates kernel production closure to `TASK-0290`.
- [x] Kernel-side production closure ownership is explicit (`TASK-0290`), no silent scope absorption into `TASK-0031`.
- [x] Rust discipline expectations are explicit (`newtype`, ownership, `#[must_use]`, `Send`/`Sync` review).

## Acceptance criteria (must be testable per cut)

### Host (mandatory)

- Deterministic `nexus-vmo` tests prove typed handle semantics and bounded mapping behavior. ✅
- Reject-path tests exist and pass (authorization, bounds, invalid handle/rights state). ✅
- Tests target behavior contracts (Soll) and not implementation accidentals. ✅

### OS / QEMU (mandatory for closure claims)

- Deterministic marker ladder is green for real producer->consumer two-process map/verify closure. ✅
- No fake success markers for degraded/stub paths. ✅
- Marker contract is registered and enforced by canonical harness (`scripts/qemu-test.sh` + `verify-uart` path). ✅

## Security checklist (mandatory)

- [x] Threat model is explicit in task + RFC.
- [x] Invariants and `DON'T DO` list are explicit.
- [x] Negative-path proof requirement is explicit.
- [x] Capability-transfer and rights boundaries remain deny-by-default.
- [x] Production-grade closure ownership is explicit (`TASK-0290`) and out-of-scope for RFC-0040 closure.

## Out-of-scope handoff (normative)

This prep stays honest about scope:

1. `TASK-0031`/`RFC-0040` close at plumbing + honest proof floor (host + OS).
2. `TASK-0290` is the kernel-side closeout for seal rights, write-map denial, lifecycle closure, and reuse/copy-fallback production truth.
3. Gate alignment remains tracked via `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`.

## Linked contracts

- `tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md`
- `docs/rfcs/RFC-0040-zero-copy-vmos-v1-plumbing-host-first-os-gated.md`
- `tasks/TASK-0290-kernel-zero-copy-closure-v1b-vmo-seals-reuse-truth.md`
- `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
- `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md`
- `docs/standards/SECURITY_STANDARDS.md`
- `docs/standards/RUST_STANDARDS.md`

## Done condition (current)

- `TASK-0031` is `In Review` while host + OS plumbing proofs are validated and out-of-scope handoff is explicit.
- RFC-0040 is `Done` under the same scoped stop conditions; production closure remains separate in `TASK-0290`.
