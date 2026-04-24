# Next Task Preparation (Drift-Free)

## Candidate next execution

- **task**: `tasks/TASK-0045-devx-nx-cli-v1.md` — `In Review`
- **contract**: `docs/rfcs/RFC-0043-devx-nx-cli-v1-host-first-production-floor-seed.md` — `Done`
- **tier**: Gate J trajectory (`production-floor`) per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
- **follow-up route**: `TASK-0046`, `TASK-0047`, `TASK-0048`, `TASK-0163`, `TASK-0164`, `TASK-0165`, `TASK-0227`, `TASK-0230`, `TASK-0268`

## Drift check vs repo state (2026-04-24)

- [x] `RFC-0043` exists and is linked as contract seed from `TASK-0045`.
- [x] `docs/rfcs/README.md` includes `RFC-0043` index entry.
- [x] `TASK-0045` header follow-up list is populated and scope split is explicit.
- [x] `TASK-0045` includes security section with threat model + hard invariants.
- [x] `TASK-0045` stop conditions include reject-path tests and anti-fake-success proof rules.
- [x] `tools/nx/` crate is implemented.
- [x] Host proof suite for `nx` exists and is green.

## Acceptance criteria status (next cut)

### Host (mandatory)

- [x] `nx doctor [--json]` deterministic behavior + dependency classification.
- [x] `nx new service|app|test` scaffolding with path-escape rejection.
- [x] `nx inspect nxb <path> [--json]` stable structured summary.
- [x] `nx idl list/check` inventory + precondition checks (no v1 codegen ownership).
- [x] `nx postflight <topic>` allowlist dispatch + bounded output tail + exit passthrough.
- [x] `nx dsl fmt|lint|build` delegation contract (or explicit unsupported).

### Security / reject floor (mandatory)

- [x] `test_reject_new_service_path_traversal`
- [x] `test_reject_new_service_absolute_path`
- [x] `test_reject_unknown_postflight_topic`
- [x] `test_doctor_exit_nonzero_when_required_missing`
- [x] `test_dsl_wrapper_fail_closed_when_backend_missing`
- [x] `test_dsl_wrapper_propagates_delegate_failure`

## Done condition (next closure step)

- Close `TASK-0045` only when Gate J production-floor proof is green via deterministic host tests and SSOT docs are synchronized.

## Remaining delta closure items (critical)

- [x] Reject/error paths with `--json` emit structured JSON envelope.
- [x] Process-level CLI tests assert exit code classes and structured output fields.
- [x] Scaffolding templates include CONTEXT header expectations from task text.

## Immediate execution checklist (no scope drift)

- [x] Add `tools/nx` crate with stable subcommand registry and dispatch.
- [x] Implement exit-code class mapping from `RFC-0043` (0/2/3/4/5/6/7).
- [x] Enforce allowlist-only topic dispatch for `postflight`.
- [x] Enforce reject of traversal/absolute path writes in scaffolding.
- [x] Add host tests asserting exit code + JSON/file effects (not marker/log-only).
- [x] Add docs in `docs/devx/nx-cli.md` and sync `docs/testing/index.md`.
- [x] Sync task/rfc checklists only after tests are actually green.

## Go / No-Go checklist for 100% closure

- [x] **GO-1** `tools/nx` canonical entrypoint exists and is used for covered v1 workflows.
- [x] **GO-2** Required reject-path tests are present and green.
- [x] **GO-3** Delegated failure propagation is proven (no fake success).
- [x] **GO-4** Follow-up extension contract is documented without `nx-*` drift.
- [x] **GO-5** `TASK-0045` + `RFC-0043` implementation checklists mirror executed evidence.
