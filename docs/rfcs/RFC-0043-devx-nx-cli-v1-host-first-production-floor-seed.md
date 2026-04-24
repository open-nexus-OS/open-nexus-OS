# RFC-0043: DevX nx CLI v1 (host-first, production-floor) — single entrypoint contract seed

- Status: In Progress
- Owners: @runtime @tools-team
- Created: 2026-04-24
- Last Updated: 2026-04-24
- Links:
  - Tasks: `tasks/TASK-0045-devx-nx-cli-v1.md` (execution + proof)
  - Related tasks: `tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md`, `tasks/TASK-0047-policy-as-code-v1-unified-engine.md`, `tasks/TASK-0048-crashdump-v2a-host-pipeline-nxsym-nx-crash.md`, `tasks/TASK-0164-sdk-v1-part1b-nx-sdk-templates-client-stubs.md`, `tasks/TASK-0165-sdk-v1-part2a-devtools-lints-pack-sign-ci.md`, `tasks/TASK-0227-diagnostics-v1-bugreport-bundles-nx-diagnose-offline-deterministic.md`, `tasks/TASK-0230-nx-sec-v1-cli-security-introspection-deny-tests-offline.md`, `tasks/TASK-0268-devx-v1-nx-cli-convergence-subcommands-no-nx-drift.md`
  - Related RFCs: `docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md`, `docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md`
  - Production gate policy: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate J: DevX, Config & Tooling, production-floor)

## Status at a Glance

- **Phase 0 (canonical nx baseline + safe delegation contract)**: ⬜
- **Phase 1 (scaffolding + inspect + idl list/check + deterministic tests)**: ⬜
- **Phase 2 (follow-up extension surface + no-drift closure)**: ⬜

Definition:

- "Complete" means this RFC's host CLI contract is implemented with deterministic host proofs and explicit anti-drift boundaries for follow-up subcommands.

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - a single authoritative host CLI entrypoint (`tools/nx`) and its baseline command behavior,
  - deterministic delegation rules (`nx postflight`, `nx dsl *`) with fail-closed semantics,
  - scaffolding/inspect/doctor/idl helper contract v1,
  - no-drift extension contract for future `nx <topic>` subcommands.
- **This RFC does NOT own**:
  - config semantics (`TASK-0046`),
  - policy engine semantics (`TASK-0047`),
  - crash format/symbolization semantics (`TASK-0048`/`TASK-0049`),
  - SDK codegen/template policy details (`TASK-0163`..`TASK-0165`),
  - diagnostics bundle format (`TASK-0227`) and security introspection semantics (`TASK-0230`),
  - conversion of every historical standalone tool in one shot (`TASK-0268` handles convergence breadth).

### Relationship to tasks (single execution truth)

- `TASK-0045` is the execution SSOT for this RFC.
- Task stop conditions and proof commands are authoritative for closure.
- Follow-on `nx <topic>` behavior must use new tasks/RFCs for new contracts, not silently expand this RFC.

## Context

Repo tooling is fragmented (`nxb-pack`, `pkgr`, `arch-check`, `qemu-run`, postflight scripts). This creates cognitive overhead and inconsistent operator behavior.

We need one host CLI path that is:

- deterministic,
- fail-closed,
- honest about delegated success/failure,
- and explicitly extensible without spawning `nx-*` drift.

Gate alignment: this is **production-floor** work (Gate J), not production-grade.

## Goals

- Define a stable v1 baseline contract for `nx` command families:
  - `new`, `inspect`, `postflight`, `doctor`, `idl list/check`, `dsl fmt/lint/build` (delegation floor).
- Enforce fail-closed behavior for unsafe input and unsupported flows.
- Define extension constraints so follow-up tasks can add subcommands without forking the CLI surface.

## Non-Goals

- Defining runtime OS behavior.
- Replacing canonical proof runners (`cargo test`, `just`, `scripts/qemu-test.sh`).
- Owning config/policy/crash/sdk domain contracts beyond command-surface integration points.
- Full IDL codegen contract (owned by `TASK-0163`).

## Constraints / invariants (hard requirements)

- **Determinism**:
  - stable argument parsing,
  - stable JSON field names/order semantics (or explicit canonical serialization),
  - stable exit-code classes.
- **No fake success**:
  - delegated commands are authoritative; `nx` cannot print "ok" if delegate failed.
- **Bounded resources**:
  - bounded output tails,
  - bounded diagnostics payload display,
  - no unbounded recursive scans by default.
- **Security floor**:
  - path traversal and absolute-write escapes are rejected,
  - topic execution is allowlist-mapped (no shell interpolation from user input),
  - secrets/tokens are never printed in normal output.
- **Stubs policy**:
  - unsupported commands must return explicit "unsupported" class (not pretend success).

## Proposed design

### Contract / interface (normative)

`nx` is the canonical entrypoint binary at `tools/nx`.

#### Command surface v1 (required)

- `nx doctor [--json]`
- `nx new service <name> [--root <path>]`
- `nx new app <app_id> [--root <path>]`
- `nx new test <name> [--root <path>]`
- `nx inspect nxb <path> [--json]`
- `nx idl list [--root tools/nexus-idl/schemas] [--json]`
- `nx idl check [--root tools/nexus-idl/schemas] [--json]`
- `nx postflight <topic> [--tail <N>] [--json]`
- `nx dsl fmt|lint|build [<args...>]` (wrapper/delegation floor only in v1)

#### Exit-code contract (required)

- `0`: success.
- `2`: usage/argument parse error.
- `3`: validation/security reject (e.g., traversal, unknown topic).
- `4`: required dependency missing (doctor/check).
- `5`: delegated command failed (non-zero delegate).
- `6`: unsupported/not implemented for current repo state.
- `7`: internal IO/state error.

`nx` must map all failures into one of these classes.

#### Delegation contract (required)

- Delegation targets are declared as static mappings in code/config.
- `nx postflight <topic>` resolves topic by allowlist key to executable + fixed arg vector.
- No runtime shell-expression construction from user input.
- Delegated stdout/stderr capture is bounded (`--tail` default bounded; deterministic truncation label when clipped).

#### Scaffolding contract (required)

- Writes are confined to allowlisted roots:
  - `source/services/`,
  - `userspace/apps/`,
  - `tests/`.
- Reject:
  - names with path separators (`/`, `\`),
  - traversal segments (`..`),
  - absolute paths.
- v1 does not auto-edit workspace manifests; prints deterministic next-step hints.

#### `nx dsl` floor contract (required)

- `nx dsl fmt|lint|build` either:
  - delegates to configured backend and returns its success/failure truthfully, or
  - returns `unsupported` with deterministic guidance when backend absent.
- No positive marker/output may claim DSL success without delegated success.

### Phases / milestones (contract-level)

- **Phase 0**: Canonical `nx` baseline + fail-closed delegation and exit-code classes.
- **Phase 1**: Scaffolding + inspect + idl list/check + doctor + deterministic reject tests.
- **Phase 2**: Follow-up extension contract documented/proven (`nx config/policy/crash/sdk/diagnose/sec`-ready) without `nx-*` drift.

## Security considerations

- **Threat model**:
  - command injection via topic/argument handling,
  - path traversal and write-escape in scaffolding,
  - false-positive success masking failures.
- **Mitigations**:
  - static topic allowlist mapping,
  - path normalization + explicit reject rules,
  - strict exit-code truth propagation for delegated commands.
- **DON'T DO**:
  - no dynamic `sh -c` assembly from user strings,
  - no acceptance of `../` or absolute write targets,
  - no "ok/ready" success strings without real success.
- **Open risks**:
  - accidental subcommand drift if future tasks add standalone `nx-*` binaries; controlled by explicit extension contract + `TASK-0268`.

## Failure model (normative)

- Unknown topic => reject (`3`) + list valid topics.
- Missing dependency/tool => fail (`4`) with actionable hint.
- Delegate non-zero => fail (`5`) and surface bounded tail.
- Unsupported command surface in current repo => fail (`6`) with deterministic guidance.
- No silent fallback to success.

## Gate mapping (TRACK alignment)

This RFC maps to **Gate J (DevX, Config & Tooling)**, tier **production-floor**.

Gate J closure statements mapped to this RFC:

- "one authoritative CLI path exists for diagnostics" -> `tools/nx` canonical entrypoint.
- "config/schema surfaces do not drift per subsystem" -> extension contract enforces `nx <topic>` convergence path.
- "naming/harness/tooling reinforce runtime proof model" -> exit-code truth, deterministic rejects, no fake-success markers.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p nx -- --nocapture
```

### Required host test families (normative)

- `test_new_service_creates_expected_tree`
- `test_reject_new_service_path_traversal`
- `test_reject_new_service_absolute_path`
- `test_inspect_nxb_json_stable_fixture`
- `test_postflight_success_passthrough`
- `test_postflight_failure_passthrough`
- `test_reject_unknown_postflight_topic`
- `test_doctor_reports_missing_required_tools`
- `test_doctor_exit_nonzero_when_required_missing`
- `test_dsl_wrapper_fail_closed_when_backend_missing`
- `test_dsl_wrapper_propagates_delegate_failure`

### Proof (OS/QEMU)

Not required for v1 contract closure (host-only tooling contract).  
Any OS wiring claims belong to follow-up tasks and must add their own OS proof commands.

### Deterministic markers (if applicable)

No success markers are required for this RFC.  
Proof authority is tests + deterministic exit/status contracts.

## Alternatives considered

- Keep multiple standalone CLIs (`nx-*`) and document conventions (rejected: drift-prone and weak for Gate J).
- Auto-edit workspace manifests during scaffolding (rejected for v1: high-risk for non-idempotent drift).
- Allow shell-based topic execution for flexibility (rejected: injection risk + non-deterministic behavior).

## Open questions

- Should `nx dsl` wrapper backend discovery be env-based, config-file-based, or both in v1?
- Should `nx postflight` mapping live in code, TOML, or generated manifest while preserving deterministic behavior?

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [ ] **Phase 0**: canonical `nx` baseline + fail-closed delegation and exit-code classes — proof: `cd /home/jenning/open-nexus-OS && cargo test -p nx phase0 -- --nocapture`
- [ ] **Phase 1**: scaffolding/inspect/idl/doctor contract + reject-path tests — proof: `cd /home/jenning/open-nexus-OS && cargo test -p nx phase1 -- --nocapture`
- [ ] **Phase 2**: follow-up extension contract + no-drift guarantees validated — proof: `cd /home/jenning/open-nexus-OS && cargo test -p nx phase2 -- --nocapture && rg "tools/nx/" tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md tasks/TASK-0047-policy-as-code-v1-unified-engine.md tasks/TASK-0048-crashdump-v2a-host-pipeline-nxsym-nx-crash.md tasks/TASK-0164-sdk-v1-part1b-nx-sdk-templates-client-stubs.md tasks/TASK-0165-sdk-v1-part2a-devtools-lints-pack-sign-ci.md tasks/TASK-0227-diagnostics-v1-bugreport-bundles-nx-diagnose-offline-deterministic.md tasks/TASK-0230-nx-sec-v1-cli-security-introspection-deny-tests-offline.md tasks/TASK-0268-devx-v1-nx-cli-convergence-subcommands-no-nx-drift.md`
- [ ] Task(s) linked with stop conditions + proof commands.
- [ ] QEMU markers (if any) appear in `scripts/qemu-test.sh` and pass. (N/A for v1 host-only closure)
- [ ] Security-relevant negative tests exist (`test_reject_*`).
