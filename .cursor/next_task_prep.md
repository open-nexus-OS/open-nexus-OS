# Next Task Preparation (Drift-Free)

## Candidate next execution

- **task**: `tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md` — `In Progress`
- **contract**: `docs/rfcs/RFC-0044-config-v1-configd-schema-layering-2pc-host-first-os-gated.md` — `In Progress`
- **tier**: Gate J trajectory (`production-floor`) per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
- **follow-up route**: `TASK-0047`, `TASK-0262`, `TASK-0266`, `TASK-0268`, `TASK-0273`, `TASK-0285`

## Drift check vs repo state (2026-04-24)

- [x] `RFC-0044` exists and is linked as contract seed from `TASK-0046`.
- [x] `docs/rfcs/README.md` includes `RFC-0044` index entry.
- [x] `TASK-0046` follow-up list is populated and hand-off expectations are explicit.
- [x] `TASK-0046` includes security section with threat model + hard invariants.
- [x] `TASK-0046` stop conditions include anti-fake-success and marker+state assertion rule.
- [x] Cap'n Proto vs JSON authority split is explicit and ADR-aligned.

## Acceptance criteria status (next cut)

### Host (mandatory)

- [ ] Schema rejects are deterministic and fail-closed (unknown/type/depth/size).
- [ ] Layering precedence is deterministic (`defaults < /system < /state < env`).
- [ ] Canonical effective snapshot bytes are deterministic Cap'n Proto.
- [ ] `configd` `GetEffective`/`GetEffectiveJson` semantic alignment proven.
- [ ] 2PC apply abort path keeps previous effective version unchanged.
- [ ] `nx config` deterministic exit/JSON contract is green.

### Security / reject floor (mandatory)

- [ ] `test_reject_config_unknown_field`
- [ ] `test_reject_config_type_mismatch`
- [ ] `test_reject_config_depth_or_size_overflow`
- [ ] `test_abort_2pc_on_prepare_reject_and_keep_previous_version`
- [ ] `test_no_fake_success_marker_without_state_transition`

## Done condition (next closure step)

- Close `TASK-0046` only when Gate J production-floor proof is green via deterministic host tests and SSOT docs are synchronized.

## Remaining closure items (critical)

- [ ] Add Cap'n Proto schema for canonical effective snapshot under `tools/nexus-idl/schemas/`.
- [ ] Implement deterministic layering + validation + snapshot encode in `userspace/config/nexus-config/`.
- [ ] Implement `configd` API split: `GetEffective` (Cap'n Proto) + `GetEffectiveJson` (derived).
- [ ] Add deterministic 2PC tests for commit/abort and unchanged-version assertions.
- [ ] Extend `nx config` contract proofs (validate/effective/diff/push/reload).

## Immediate execution checklist (no scope drift)

- [ ] Land schema + conformance tests first (contract floor).
- [ ] Land `nexus-config` deterministic model + version hashing on canonical bytes.
- [ ] Land `configd` 2PC orchestration and stable error/result mapping.
- [ ] Land `nx config` deterministic UX under existing `tools/nx`.
- [ ] Keep marker-to-state/result assertion coupling in all OS-gated proofs.
- [ ] Sync task/rfc checklists only after tests are actually green.

## Go / No-Go checklist for 100% closure

- [ ] **GO-1** Cap'n Proto canonical contract is implemented and authoritative.
- [ ] **GO-2** Required reject-path tests are present and green.
- [ ] **GO-3** 2PC abort/rollback semantics are proven with unchanged-version evidence.
- [ ] **GO-4** `nx config` lives under `tools/nx` and passes deterministic CLI proof suite.
- [ ] **GO-5** `TASK-0046` + `RFC-0044` implementation checklists mirror executed evidence.
