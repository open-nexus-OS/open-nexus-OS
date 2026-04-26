# Next Task Preparation (Drift-Free)

## Active execution

- **task**: `tasks/TASK-0047-policy-as-code-v1-unified-engine.md` — `In Progress`
- **contract**: `docs/rfcs/RFC-0045-policy-as-code-v1-unified-policy-tree-evaluator-explain-dry-run-learn-enforce-nx-policy.md` — `In Progress`
- **tier**: Gate B trajectory (`production-grade`) per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
- **completed predecessor**: `tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md` — `Done`

## Carry-in from completed TASK-0046

- [x] `configd` is the single host-first config authority.
- [x] Canonical runtime/persistence snapshots are Cap'n Proto.
- [x] Layered config authoring is JSON-only under `/system/config` and `/state/config`.
- [x] `nx config` is the only CLI surface for config UX.
- [x] 2PC honesty and subscriber/update notification seams are covered by host tests.

## Queue-head prep notes

- `TASK-0047` must consume Config v1 as-is rather than introducing a parallel config or policy-distribution path.
- `RFC-0045` now exists and must remain the contract seed for `TASK-0047`; do not silently expand `RFC-0015`.
- `TASK-0047` task/RFC are pre-aligned to repo reality:
  - Gate B (`Security, Policy & Identity`, `production-grade`) framing is explicit,
  - a full security section exists (threat model, invariants, DON'T DO, reject-proof expectations),
  - repo-fit migration is explicit: do not leave `recipes/policy/` and `policies/` as dual live roots,
  - existing `userspace/policy/` + `policyd` seams must be extended rather than replaced by a parallel authority crate,
  - host proofs call for behavior-first negative tests (`test_reject_*`) and adapter parity instead of marker-only closure.

## Phase 0 prep notes (`tools/nx`)

- Before `nx policy` is implemented, refactor `tools/nx/src/lib.rs` into:
  - `tools/nx/src/lib.rs`
  - `tools/nx/src/cli.rs`
  - `tools/nx/src/error.rs`
  - `tools/nx/src/output.rs`
  - `tools/nx/src/runtime.rs`
  - `tools/nx/src/commands/mod.rs`
  - `tools/nx/src/commands/{new,inspect,idl,postflight,doctor,dsl,config,policy}.rs`
- Phase 0 is structure-first only:
  - preserve existing `nx` behavior,
  - do not introduce a second binary,
  - do not regrow a monolithic `lib.rs` during `nx policy` work.

## Open prep decisions to resolve in planning

- Decide whether v1 migrates directly to `policies/` or uses a bounded importer/parity layer first.
- Choose the first migrated adapter pair for honest parity proof (e.g. signing + egress, or another repo-fit pair).
- Map the exact proof floor to crates/tests without inventing synthetic marker-only evidence.

## Immediate no-drift checklist

- [x] `TASK-0046` and `RFC-0044` are treated as `Done`.
- [x] `docs/rfcs/README.md`, `tasks/IMPLEMENTATION-ORDER.md`, and `tasks/STATUS-BOARD.md` reflect the new carry-in state.
- [x] `.cursor` workfiles reflect `TASK-0047` in-progress state rather than `TASK-0046` review.
- [x] Create RFC seed for `TASK-0047` before implementation starts (`docs/rfcs/RFC-0045-policy-as-code-v1-unified-policy-tree-evaluator-explain-dry-run-learn-enforce-nx-policy.md`).
- [ ] Preserve Config v1 authority boundaries; do not fork config storage, reload, or CLI semantics.
- [ ] Preserve single policy authority during migration; do not leave `recipes/policy/` and `policies/` both live.
- [ ] Keep `nx policy` under `tools/nx/`; no `nx-*` drift and no parallel policy compiler/daemon.
- [ ] Keep Phase 0 behavior-preserving: refactor structure first, then add `commands/policy.rs`.
- [ ] Ensure plan-first execution maps `TASK-0047` stop conditions one-by-one to proofs before implementation starts.
