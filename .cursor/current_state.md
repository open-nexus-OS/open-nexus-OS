# Cursor Current State (SSOT)

## Current architecture state

- **last_decision (2026-04-24)**: `RFC-0044` remains `Done`; `TASK-0046` is now held at `In Review` while the proven host-first config contract floor is reviewed.
- **active boundary**: Config v1 authority is now locked for host-first closure:
  - Cap'n Proto for canonical runtime/persistence snapshots,
  - JSON-only authoring/validation plus derived CLI/debug views,
  - deterministic layering `defaults < /system < /state < env`,
  - honest 2PC reload semantics with no fake success.
- **gate tier**: Gate J (`production-floor`) per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`.

## Active review state

- **active_task**: `tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md` — `In Review`
- **active_contract**: `docs/rfcs/RFC-0044-config-v1-configd-schema-layering-2pc-host-first-os-gated.md` — `Done`
- **next_candidate_after_review**: `tasks/TASK-0047-policy-as-code-v1-unified-engine.md` — `Draft`

## Locked constraints from TASK-0046 review candidate

- Kernel untouched.
- Canonical config authority stays in `nexus-config` + `configd` + `nx config`.
- Layered config authoring under `/system/config` and `/state/config` is JSON-only.
- `nx config push` writes deterministic state overlay `state/config/90-nx-config.json`.
- Marker-only evidence remains insufficient for any future OS/QEMU config closure claims.

## Closure evidence (TASK-0046)

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
- Header discipline synced on touched Rust sources; `TEST_COVERAGE` fields now describe the actual current test counts/state.

## Follow-up split (preserve scope)

- `TASK-0047`: policy-as-code consumer cutover on top of Config v1 authority.
- `TASK-0262`: determinism/hygiene floor alignment and anti-fake-success discipline.
- `TASK-0266`: single-authority and naming contract continuity.
- `TASK-0268`: `nx` convergence, no `nx-*` logic drift.
- `TASK-0273`: downstream consumer adoption without parallel config authority.
- `TASK-0285`: QEMU harness phase/failure evidence discipline for OS-gated closure.
