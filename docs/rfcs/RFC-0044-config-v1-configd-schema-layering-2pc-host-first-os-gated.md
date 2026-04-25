# RFC-0044: Config v1 (`configd` + schemas + layering + 2PC + `nx config`) host-first, OS-gated contract seed

Status: Done
Owner: Platform Team
Created: 2026-04-24
Area: Config / Control Plane / Security
Execution SSOT: `tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md`
Supersedes: none
Superseded-by: none

## Summary

Define the canonical Config v1 contract for deterministic configuration authoring, validation, canonical snapshot materialization, runtime distribution, and transactional apply/rollback.
The canonical runtime and persisted effective snapshot uses Cap'n Proto; JSON remains authoring and derived debug/CLI view only.

## Problem Statement

Current configuration paths are fragmented and do not yet provide a single canonical, deterministic, fail-closed contract across CLI tooling (`nx config`), schema validation, `configd` APIs, and apply semantics.
Without a strict contract, follow-up tasks risk format drift, authority ambiguity, and non-reproducible runtime behavior.

## Goals

- Define one canonical contract boundary for Config v1 across tooling and service API.
- Enforce deterministic canonical effective snapshot bytes via Cap'n Proto.
- Keep JSON as authoring and derived view only; never runtime/persistence authority.
- Establish bounded, fail-closed validation and deterministic reject behavior.
- Define transactional apply semantics with staged validation, commit, and rollback guarantees.

## Non-Goals

- Full OS-wide closure for every downstream consumer in this RFC.
- Replacing all existing configuration surfaces in one change.
- New policy semantics beyond schema/layering/apply contract required for Config v1.

## Scope

- `tools/nx` (`nx config ...`) contract behavior for authoring/validation/effective export.
- `userspace/config/nexus-config` model + validation + canonical snapshot materialization.
- `source/services/configd` API contract for `GetEffective` (Cap'n Proto), `GetEffectiveJson` (derived), and `Subscribe` update notifications.
- Cap'n Proto schema(s) for canonical effective snapshot and versioning.

## Design Overview

1. Inputs are authored in JSON and validated against strict schemas.
2. Layering and precedence produce a deterministic effective model.
3. Effective model is encoded as canonical Cap'n Proto bytes (runtime/persistence authority).
4. `configd` exposes:
   - `GetEffective`: canonical Cap'n Proto effective snapshot + version.
   - `GetEffectiveJson`: derived JSON view + same version, for debug/CLI ergonomics.
   - `Subscribe`: committed-version update notifications for downstream consumers.
5. Apply path uses bounded two-phase semantics: validate/stage then commit or rollback.

## Contract (Normative)

- Canonical authority:
  - Runtime and persisted effective snapshots MUST be Cap'n Proto.
  - Canonical bytes MUST be deterministic for equivalent validated inputs.
- JSON:
  - JSON MUST be treated as authoring and derived view only.
  - JSON MUST NOT be used as canonical runtime or persistence authority.
- Validation:
  - Unknown fields, invalid types, and out-of-bounds values MUST fail closed.
  - Rejects MUST be explicit, stable, and non-ambiguous.
- Apply/transaction:
  - Commit happens only after full validation/stage success.
  - On commit failure, rollback MUST restore prior consistent state.
  - Partial apply MUST NOT be observable as success.
- CLI:
  - `nx config` error classes and exits MUST be deterministic and script-safe.
  - Human output and `--json` derived output MUST represent the same semantic result.

## Security Considerations

### Threat model

- Malformed/untrusted config input attempting parser or validator bypass.
- Drift between authoring view (JSON) and runtime authority representation.
- Partial apply leading to inconsistent policy/runtime state.
- Replay or stale snapshot confusion across service boundaries.

### Security invariants

- Canonical effective snapshot bytes are deterministic Cap'n Proto.
- Validation is bounded and fail-closed before any state transition.
- Runtime consumers treat only canonical snapshots as authority.
- Versioned reads prevent silent stale-state acceptance.

### DON'T DO

- Do not promote JSON debug/export views to runtime canonical authority.
- Do not allow permissive fallback on schema/validation failures.
- Do not report success when commit/rollback invariants are not met.

## Determinism / Failure Semantics

- Equivalent validated input sets MUST produce byte-identical canonical snapshots.
- Repeated apply with unchanged effective input MUST be idempotent.
- All reject paths MUST map to explicit, stable error classes.

## Test & Proof Strategy

Host-first closure for this RFC requires:

- Schema/validation rejects:
  - unknown field/type/bounds rejects are deterministic.
- Canonical snapshot proof:
  - same input -> same Cap'n Proto bytes;
  - changed input -> changed bytes and version progression.
- `configd` API proof:
  - `GetEffective` Cap'n Proto bytes and `GetEffectiveJson` derived view align semantically.
- 2PC/apply proof:
  - stage/commit success path;
  - forced commit failure -> rollback to previous version with no fake success.
- `nx config` proof:
  - deterministic exit classes for validate/effective/apply/status;
  - `nx config effective --json` aligns in version and derived JSON semantics with `configd`.

Planned host proof commands (execution task-owned):

- `cargo test -p nexus-config -- --nocapture`
- `cargo test -p configd -- --nocapture`
- `cargo test -p nx -- --nocapture`

OS-gated integration closure remains in execution SSOT follow-up gates.

## Migration / Rollout

- Introduce contract surfaces behind Config v1 task slices.
- Keep old behavior only where explicitly compatibility-scoped.
- Promote to default only after host proofs are green and gate criteria are met.

## Risks & Mitigations

- Risk: format authority drift between JSON and canonical snapshot.
  - Mitigation: enforce Cap'n Proto canonical contract and dedicated conformance tests.
- Risk: transactional edge cases under failure injection.
  - Mitigation: deterministic failure-injection tests for stage/commit/rollback boundaries.
- Risk: scope creep into unrelated policy/runtime semantics.
  - Mitigation: keep execution strictly bounded by `TASK-0046` and listed follow-up tasks.

## Follow-up Notes

- Field-level schema evolution windows remain follow-up policy work; this RFC locks the v1 host-first contract floor only.
- Real downstream consumer adoption and OS/QEMU marker proofs remain execution follow-up scope outside this RFC.

## Status at a Glance

- [x] Contract seed exists and is linked to execution SSOT.
- [x] Canonical Cap'n Proto schema merged.
- [x] `nexus-config` deterministic effective snapshot materialization merged.
- [x] `configd` `GetEffective`/`GetEffectiveJson` contract merged.
- [x] `configd` `Subscribe` update-notification contract merged.
- [x] `nx config` deterministic CLI surface merged and proofed.
- [x] Host proof floor green across config crates/services/tooling.

## Implementation Checklist (Execution-Linked)

Host-first closure is complete in this cut; OS/QEMU marker closure remains explicitly gated by execution SSOT and is not claimed here.

Phase 0 (contract + schema floor):

- [x] Seed contract RFC created and aligned with ADR constraints.
- [x] Cap'n Proto schema for canonical effective snapshot added.
- [x] Schema and versioning conformance tests added.

Phase 1 (library + service floor):

- [x] Deterministic layering + canonical snapshot encode in `nexus-config`.
- [x] `configd` APIs expose canonical + derived view contract.
- [x] `configd` subscriber/update notification seam is covered by deterministic host tests.
- [x] 2PC apply/rollback invariants covered by deterministic tests.

Phase 2 (CLI + closure floor):

- [x] `nx config` commands aligned to contract and deterministic exits.
- [x] JSON-only authoring path is enforced for layered source files and state push output.
- [x] Host-first proof commands documented and green.
- [x] Execution SSOT status sync and gate notes updated.
