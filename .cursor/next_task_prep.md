# Next Task Preparation (Drift-Free)

## Candidate next execution

- **task**: `tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md` — `In Progress`
- **contract seed (RFC)**: `docs/rfcs/RFC-0040-zero-copy-vmos-v1-plumbing-host-first-os-gated.md` — `In Progress`
- **closure gate task**: `tasks/TASK-0290-kernel-zero-copy-closure-v1b-vmo-seals-reuse-truth.md` — `Draft`
- **tier**: production-grade trajectory per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate A + Gate C relevant closure obligations)

## Drift check vs `current_state.md`

- [x] `TASK-0029`/`RFC-0039` are done and archived from active execution scope.
- [x] `TASK-0031` header links include the new seed RFC (`RFC-0040`).
- [x] `RFC-0040` explicitly requires production-grade closure before status `Complete`.
- [x] Kernel-side production closure ownership is explicit (`TASK-0290`), no silent scope absorption into `TASK-0031`.
- [x] Rust discipline expectations are explicit (`newtype`, ownership, `#[must_use]`, `Send`/`Sync` review).

## Acceptance criteria (must be testable per cut)

### Host (mandatory)

- Deterministic `nexus-vmo` tests prove typed handle semantics and bounded mapping behavior.
- Reject-path tests exist and pass (authorization, bounds, invalid handle/rights state).
- Tests target behavior contracts (Soll) and not implementation accidentals.

### OS / QEMU (mandatory for closure claims)

- Deterministic marker ladder proves real producer -> transfer -> consumer map/verify flow.
- No fake success markers for degraded/stub paths.
- Marker contract is registered and enforced by canonical harness (`scripts/qemu-test.sh` + `verify-uart` path).

## Security checklist (mandatory)

- [x] Threat model is explicit in task + RFC.
- [x] Invariants and `DON'T DO` list are explicit.
- [x] Negative-path proof requirement is explicit.
- [x] Capability-transfer and rights boundaries remain deny-by-default.
- [x] Production-grade closure is required before complete claims.

## Production-grade requirement (normative)

This prep stays honest about scope:

1. `TASK-0031` is the plumbing/honesty floor and must not claim production-grade alone.
2. `RFC-0040` cannot be marked complete until production-grade closure obligations are proven.
3. `TASK-0290` is the kernel-side closeout for seal rights, write-map denial, and reuse/copy-fallback truth.
4. Gate alignment must remain visible against `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`.

## Linked contracts

- `tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md`
- `docs/rfcs/RFC-0040-zero-copy-vmos-v1-plumbing-host-first-os-gated.md`
- `tasks/TASK-0290-kernel-zero-copy-closure-v1b-vmo-seals-reuse-truth.md`
- `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
- `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md`
- `docs/standards/SECURITY_STANDARDS.md`
- `docs/standards/RUST_STANDARDS.md`

## Done condition (current)

- Task status may move from `In Progress` to `Done` only with reality-synced task/RFC docs and proven per-cut checks.
- RFC status may move to `Complete` only once production-grade closure obligations (via `TASK-0290`) are green.
