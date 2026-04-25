# Next Task Preparation (Drift-Free)

## Candidate next execution

- **task**: `tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md` — `In Review`
- **contract**: `docs/rfcs/RFC-0044-config-v1-configd-schema-layering-2pc-host-first-os-gated.md` — `Done`
- **tier**: Gate J trajectory (`production-floor`) per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
- **next candidate after review**: `tasks/TASK-0047-policy-as-code-v1-unified-engine.md` — `Draft`

## Carry-in from reviewed TASK-0046

- [x] `configd` is the single host-first config authority.
- [x] Canonical runtime/persistence snapshots are Cap'n Proto.
- [x] Layered config authoring is JSON-only under `/system/config` and `/state/config`.
- [x] `nx config` is the only CLI surface for config UX.
- [x] 2PC honesty and subscriber/update notification seams are covered by host tests.

## Queue-head prep notes

- `TASK-0046` is review-ready with RFC locked to `Done`; `TASK-0047` should consume Config v1 as-is rather than introducing a parallel config or policy-distribution path.
- Any `TASK-0047` contract seed must reference:
  - `tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md`
  - `docs/rfcs/RFC-0044-config-v1-configd-schema-layering-2pc-host-first-os-gated.md`
  - `docs/adr/0017-service-architecture.md`
  - `docs/adr/0021-structured-data-formats-json-vs-capnp.md`

## Immediate no-drift checklist

- [x] `TASK-0046` is `In Review` and `RFC-0044` is `Done`.
- [x] `docs/rfcs/README.md`, `tasks/IMPLEMENTATION-ORDER.md`, and `tasks/STATUS-BOARD.md` reflect the review state.
- [x] `.cursor` workfiles reflect active `TASK-0046` review rather than final closure.
- [ ] Create RFC seed for `TASK-0047` before implementation starts.
- [ ] Preserve Config v1 authority boundaries; do not fork config storage, reload, or CLI semantics.
