# Cursor Current State (SSOT)

## Current architecture state

- **last_decision (2026-04-26)**: `TASK-0046` and `RFC-0044` are `Done`; `TASK-0047` and `RFC-0045` are now `In Progress`.
- **active boundary**: Config v1 authority is locked and becomes mandatory carry-in for Policy as Code:
  - Cap'n Proto remains canonical for runtime/persistence config snapshots,
  - JSON remains authoring/validation plus derived CLI/debug view only,
  - deterministic layering stays `defaults < /system < /state < env`,
  - `configd` owns deterministic reload/version transitions and honest 2PC semantics.
- **gate tier**: active execution prep now sits on Gate B (`Security, Policy & Identity`, `production-grade`) per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`; `TASK-0047` Phase 0 also preserves Gate J `tools/nx` no-drift rules while refactoring CLI structure.

## Active execution state

- **active_task**: `tasks/TASK-0047-policy-as-code-v1-unified-engine.md` — `In Progress`
- **active_contract**: `docs/rfcs/RFC-0045-policy-as-code-v1-unified-policy-tree-evaluator-explain-dry-run-learn-enforce-nx-policy.md` — `In Progress`
- **completed_predecessor**: `tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md` — `Done`
- **completed_predecessor_contract**: `docs/rfcs/RFC-0044-config-v1-configd-schema-layering-2pc-host-first-os-gated.md` — `Done`

## Locked carry-in constraints from TASK-0046

- Kernel untouched.
- Canonical config authority stays in `nexus-config` + `configd` + `nx config`.
- Layered config authoring under `/system/config` and `/state/config` is JSON-only.
- `nx config push` writes deterministic state overlay `state/config/90-nx-config.json`.
- Marker-only evidence remains insufficient for any future OS/QEMU closure claims.

## Current focus for TASK-0047 prep

- `policyd` remains the single policy authority; no parallel `abi-filterd`/compiler/daemon authority may appear.
- `TASK-0047` must extend the existing `userspace/policy/` seam instead of inventing a second policy crate authority.
- Migration must not leave `recipes/policy/` and `policies/` as dual live roots.
- `configd` reload/versioning is reused; no separate policy reload plane.
- `nx policy` must stay under `tools/nx/`; no `nx-*` drift.
- Phase 0 is explicit: refactor `tools/nx/src/lib.rs` into `cli.rs`, `error.rs`, `output.rs`, `runtime.rs`, and `commands/{new,inspect,idl,postflight,doctor,dsl,config,policy}.rs` without changing current CLI behavior.

## Proven carry-in evidence (TASK-0046)

- Host proof floor is green:
  - `cargo test -p nexus-config -- --nocapture`
  - `cargo test -p configd -- --nocapture`
  - `cargo test -p nx -- --nocapture`
- Required proof classes are covered:
  - schema rejects: unknown/type/depth/size fail closed with stable classification,
  - lexical-order layer directory merge + deterministic precedence,
  - byte-identical Cap'n Proto snapshots for equivalent inputs,
  - 2PC reject/timeout/commit-failure keeps prior version active,
  - `nx config` deterministic exit and `--json` contracts,
  - `nx config effective --json` parity with `configd` version + derived JSON for the same layered inputs.

## Follow-up split (preserve scope)

- `TASK-0047`: Policy as Code v1 on top of Config v1 authority.
- `TASK-0262`: determinism/hygiene floor alignment and anti-fake-success discipline.
- `TASK-0266`: single-authority and naming contract continuity.
- `TASK-0268`: `nx` convergence, no `nx-*` logic drift.
- `TASK-0273`: downstream consumer adoption without parallel config authority.
- `TASK-0285`: QEMU harness phase/failure evidence discipline for OS-gated closure.
