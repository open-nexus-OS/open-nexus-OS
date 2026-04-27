# RFC-0045: Policy as Code v1 (unified policy tree + evaluator + explain/dry-run + learn→enforce + `nx policy`) host-first, OS-gated contract seed

- Status: Done
- Owners: @runtime @security @tools-team
- Created: 2026-04-26
- Last Updated: 2026-04-26
- Links:
  - Tasks: `tasks/TASK-0047-policy-as-code-v1-unified-engine.md` (execution + proof)
  - ADRs: `docs/adr/0014-policy-architecture.md`, `docs/adr/0017-service-architecture.md`, `docs/adr/0021-structured-data-formats-json-vs-capnp.md`
  - Related RFCs: `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md`, `docs/rfcs/RFC-0032-abi-syscall-guardrails-v2-userland-kernel-untouched.md`, `docs/rfcs/RFC-0043-devx-nx-cli-v1-host-first-production-floor-seed.md`, `docs/rfcs/RFC-0044-config-v1-configd-schema-layering-2pc-host-first-os-gated.md`
  - Production gate policy: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate B: Security, Policy & Identity, production-grade)

## Status at a Glance

- **Phase 0 (CLI structure floor for `nx policy`)**: ✅
- **Phase 1 (single policy tree + canonical version + evaluator contract)**: ✅
- **Phase 2 (policyd reload/eval/mode + adapter + `nx policy` contract)**: ✅

Definition:

- "Complete" means this RFC's contract is implemented with deterministic host proofs green and any OS/QEMU markers claimed by the execution task are real, gated, and non-fake-success.

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - the single-authority Policy-as-Code v1 contract on top of the existing `policyd` baseline,
  - the policy-tree authority boundary and canonical versioning rules,
  - bounded evaluator semantics (`allow`/`deny`, reason codes, explain trace, dry-run, learn→enforce lifecycle),
  - `configd`-driven reload/versioning semantics for policy updates,
  - the canonical `nx policy` command-surface contract under `tools/nx`,
  - the Phase 0 `tools/nx` module-structure preparation required to add `nx policy` without regrowing monolithic `lib.rs`.
- **This RFC does NOT own**:
  - the baseline deny-by-default + audit foundation already closed in `RFC-0015`,
  - sandbox profile distribution specifics (`TASK-0189`),
  - snapshot artifact/binary compiler semantics (`TASK-0229`),
  - security introspection CLI beyond `nx policy` (`TASK-0230`),
  - app-capability-matrix domain semantics (`TASK-0136`),
  - quota/egress domain closure specifics (`TASK-0043`),
  - kernel sysfilter or kernel policy enforcement changes.

### Relationship to tasks (single execution truth)

- `TASK-0047` is the execution SSOT for this RFC.
- Task stop conditions and proof commands are authoritative for closure.
- Follow-on policy tasks must use this RFC as the v1 contract seed, or introduce a new RFC if they need behavior outside this scope.

## Context

The repo already has a policy baseline:

- `policyd` exists in host and OS-lite form,
- `userspace/policy/` provides the current host/build-time policy seam,
- `recipes/policy/` is the current authoring root for the baseline policy data,
- `RFC-0015` closed the baseline policy-authority and audit contract,
- `TASK-0046` / `RFC-0044` closed Config v1 as the deterministic reload/versioning authority.

What does not exist yet is a single, versioned Policy-as-Code contract that unifies policy islands such as ABI filters, signing policy, sandbox/egress domains, DSoftBus ACLs, and observability gatekeeping without creating new competing authorities.

Without a new contract seed, follow-on work risks:

- authority drift (`recipes/policy/` vs `policies/`, `policyd` vs helper daemons/tools),
- reload/versioning drift outside `configd`,
- `nx policy` growth that re-fragments the CLI surface,
- and marker-only or implementation-shaped tests instead of behavior-first proof.

## Goals

- Define one Policy-as-Code v1 authority contract centered on `policyd`.
- Define how authoring inputs become a canonical, hash-addressed policy tree with deterministic versioning.
- Define bounded evaluator semantics: explain trace, dry-run, learn→enforce lifecycle, and stable reject behavior.
- Define `configd` as the only reload/versioning authority for policy updates.
- Define `nx policy` as the only CLI surface for policy authoring/introspection/mode actions, under the existing `tools/nx`.

## Non-Goals

- Replacing every existing policy consumer in one change.
- Defining snapshot binary layout or OS artifact compiler details (`TASK-0229`).
- Defining a second policy daemon, second CLI binary, or separate policy compiler authority.
- Kernel-enforced policy semantics.
- Claiming OS/QEMU closure before real adapters and reload paths exist.

## Constraints / invariants (hard requirements)

- **Single authority**:
  - `policyd` remains the decision authority.
  - Guardrails/adapters may enforce locally, but they must consult the unified policy tree rather than re-implement policy truth.
- **Config authority reuse**:
  - Policy reload/version transitions MUST be driven by `configd` (`RFC-0044`), not ad-hoc file polling or separate daemons.
- **Determinism**:
  - equivalent validated inputs MUST produce the same policy version,
  - explain traces MUST be bounded and deterministic,
  - reject classes MUST be explicit and stable.
- **Derived/canonical split**:
  - if TOML remains authoring format, canonical JSON is derived only for canonicalization/hash/schema/debug needs,
  - derived views MUST NOT become a second live authoring authority.
- **Bounded resources**:
  - cap rule counts per domain,
  - cap explain trace steps,
  - cap learn-log emission and generator expansion.
- **Security floor**:
  - effective identity is kernel-derived/provenance-safe, never trusted from arbitrary payload strings,
  - dry-run or learn mode MUST NOT grant access that enforce mode would deny,
  - audit/explain output MUST NOT leak secrets.
- **No fake success**:
  - markers are supporting evidence only and MUST follow real assertions or verified state transitions.
- **Stubs policy**:
  - any stub/compat layer during migration is explicitly labeled, bounded, and non-authoritative.

## Proposed design

### Contract / interface (normative)

#### Authority and data-flow contract

Policy as Code v1 uses this authority model:

1. Human-authored policy inputs live in one policy tree authority.
2. Inputs are normalized/canonicalized into a deterministic tree representation.
3. `PolicyVersion = sha256(canonical_policy_json)` (or an equivalent canonical deterministic representation).
4. `configd` distributes and versions reload candidates; `policyd` applies them with fail-closed reload semantics.
5. Services/adapters query `policyd` or consume policy data derived from the same unified tree.

#### Authoring-root contract

- During migration, the repo may temporarily read from the current `recipes/policy/` baseline while introducing the v1 tree shape.
- Closure MUST leave exactly one live policy-tree authority.
- Shipping both `recipes/policy/` and `policies/` as concurrent authorities is forbidden.

#### Evaluator contract

The evaluator seam extends `userspace/policy/` rather than introducing a parallel authority crate.

Required logical result shape:

```text
Decision {
  allow: bool,
  reason_code: stable enum/string code,
  trace: bounded explain steps,
  mode: enforce | learn | dry-run,
  would_deny: bool
}
```

Normative semantics:

- deny-by-default unless an explicit matching allow path exists,
- explain trace is bounded and deterministic,
- dry-run may observe would-deny decisions but MUST NOT grant what enforce mode would deny,
- learn mode collects bounded observation data and MUST NOT bypass deny rules.

#### Reload / lifecycle contract

- `policyd` reload is fed by `configd` candidate/application flow.
- Prepare reject / timeout / commit failure MUST leave the prior policy version active.
- Mode changes (`learn` / `enforce`) MUST be authenticated and audited.
- Stale or unauthorized lifecycle transitions MUST reject fail-closed.

#### CLI contract (`nx policy`)

`nx policy` is the canonical CLI surface and lives under `tools/nx` only.

Initial v1 subcommand family:

- `nx policy validate`
- `nx policy diff`
- `nx policy explain`
- `nx policy mode`

Any future policy-related operator UX must extend `nx policy` or a later explicitly approved `nx` subcommand family; it must not create `nx-policy` or other standalone drift.

#### Phase 0 `tools/nx` structure contract

Before adding `nx policy`, `tools/nx/src/lib.rs` is refactored into this stable structure:

- `tools/nx/src/lib.rs`
- `tools/nx/src/cli.rs`
- `tools/nx/src/error.rs`
- `tools/nx/src/output.rs`
- `tools/nx/src/runtime.rs`
- `tools/nx/src/commands/mod.rs`
- `tools/nx/src/commands/new.rs`
- `tools/nx/src/commands/inspect.rs`
- `tools/nx/src/commands/idl.rs`
- `tools/nx/src/commands/postflight.rs`
- `tools/nx/src/commands/doctor.rs`
- `tools/nx/src/commands/dsl.rs`
- `tools/nx/src/commands/config.rs`
- `tools/nx/src/commands/policy.rs`

This Phase 0 refactor MUST preserve CLI behavior; it is a structure-first anti-drift preparation, not a UX rewrite.

### Phases / milestones (contract-level)

- **Phase 0**: `tools/nx` structure refactor lands with unchanged CLI behavior and explicit room for `commands/policy.rs`.
- **Phase 1**: single policy-tree authority, canonical versioning, evaluator result contract, and reject semantics are defined and host-proofed.
- **Phase 2**: `policyd` reload/eval/mode contract and `nx policy` surface are implemented against Config v1, with at least one migrated adapter proving parity before cutover claims.

### Host-first closure snapshot (2026-04-26)

- Phase 0 landed with unchanged `nx` behavior and the planned module structure under `tools/nx/src/commands/`.
- The active policy tree root is `policies/`; `recipes/policy/` is no longer a live TOML authority.
- `PolicyVersion` is derived from deterministic canonical policy data.
- Invalid, oversize, ambiguous, traversal, unknown-section, and over-budget explain traces reject with stable classes.
- Config v1 carries the candidate policy root in the effective snapshot as `policy.root`; `policyd` stages the resulting `PolicyTree` through the `configd::ConfigConsumer` 2PC seam.
- External host frame operations for `Version`, `Eval`, `ModeGet`, and `ModeSet` are backed by the unified authority and emit bounded audit events for allow/deny/reject outcomes.
- `policies/manifest.json` records the deterministic policy tree hash and `nx policy validate` rejects missing or mismatched manifests.
- Signing capability and exec/capability adapter parity are proven against the legacy `PolicyDoc::check` behavior before claiming unified evaluator cutover.
- The `policyd` service-facing check frame now evaluates through `PolicyAuthority`, so the first adapter path no longer bypasses unified evaluator semantics.
- `nx policy validate|diff|explain|mode` lives under `tools/nx` only and delegates policy semantics to `userspace/policy`; `nx policy mode` is host preflight-only until a live daemon mode RPC exists.

Proof evidence:

- `cargo test -p policy -- --nocapture` — green, 18 tests.
- `cargo test -p nexus-config -- --nocapture` — green, 10 tests.
- `cargo test -p configd -- --nocapture` — green, 8 tests.
- `cargo test -p policyd -- --nocapture` — green, 25 tests.
- `cargo test -p nx -- --nocapture` — green, 23 unit tests + 8 CLI contract tests.

OS/QEMU markers remain gated and unclaimed for this RFC closure.

## Security considerations

- **Threat model**:
  - authority drift from multiple active policy roots or daemons,
  - identity spoofing through adapter-supplied subject strings,
  - unauthorized mode changes or stale reloads,
  - explain/log blowup from malicious policy input or requests,
  - migration poisoning where a compat/import path changes semantics silently.
- **Mitigations**:
  - single-authority `policyd` contract,
  - `configd`-owned reload/version transitions,
  - kernel-derived/provenance-safe identity binding,
  - bounded traces/logs/rule counts,
  - parity tests for migrated domains before cutover.
- **DON'T DO**:
  - do not ship both `recipes/policy/` and `policies/` as active authorities,
  - do not add `nx-*` drift or a second policy daemon/compiler,
  - do not let dry-run/learn become an implicit allow path,
  - do not trust unbound subject strings in OS-facing enforcement paths,
  - do not use marker-only closure claims.
- **Open risks**:
  - downstream domains beyond the first `policyd` host check cutover may still require bounded compat/parity phases before full OS-facing unification.

## Failure model (normative)

- Invalid / oversize / ambiguous policy input => reject with explicit stable class before activation.
- Unauthorized or stale mode/reload transition => reject fail-closed; active version remains unchanged.
- Explain trace over budget => deterministic reject or truncation per contract, never unbounded growth.
- Missing adapter parity proof => domain remains explicitly compat-scoped; no fake "unified" claim.
- Unsupported OS/QEMU path => explicit unsupported/gated status, not success.
- No silent fallback: any compatibility layer or fallback path must be explicit and bounded.

## Gate mapping (TRACK alignment)

This RFC maps to **Gate B (Security, Policy & Identity)**, tier **production-grade**.

Gate B closure statements mapped to this RFC:

- "Sensitive operations are deny-by-default and routed through canonical policy authority" -> `policyd` single-authority contract.
- "Reject paths are first-class" -> deterministic `test_reject_*` host proof requirements.
- "Audit trails exist for security decisions without leaking secrets" -> explain/audit boundedness and authenticated lifecycle semantics.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p policy -- --nocapture
cd /home/jenning/open-nexus-OS && cargo test -p nexus-config -- --nocapture
cd /home/jenning/open-nexus-OS && cargo test -p configd -- --nocapture
cd /home/jenning/open-nexus-OS && cargo test -p policyd -- --nocapture
cd /home/jenning/open-nexus-OS && cargo test -p nx -- --nocapture
```

### Required host proof families (normative)

- CLI structure refactor preserves existing `nx` behavior while enabling `commands/policy.rs`.
- invalid/oversize/ambiguous policy tree => stable `test_reject_*` coverage.
- deterministic policy version for stable inputs.
- allow/deny evaluation with expected explain steps across multiple domains.
- dry-run / learn-mode behavior proves "observe without bypass".
- authenticated mode changes succeed; unauthorized/stale transitions reject deterministically.
- migrated adapter parity proves old and unified behavior agree before cutover claim.
- policy reload is exercised through the `configd::ConfigConsumer` 2PC seam, not a parallel file polling path.
- Config v1 carries the policy candidate root in `policy.root`; tests must fail if `policyd` ignores the effective snapshot.
- missing or mismatched policy manifests reject fail-closed.
- `nx policy mode` remains host preflight-only unless a live daemon mode RPC is added and proven.

### Proof (OS/QEMU)

Execution-task owned and explicitly gated until `policyd` OS-lite reload plus at least one real adapter exists.

Canonical command once gated:

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=210s ./scripts/qemu-test.sh
```

### Deterministic markers (if applicable)

- `policyd: ready (ver=<hex8>)`
- `policy: tree loaded (ver=1 sha=<hex8>)`
- `policy: adapters active (domains=...)`
- `SELFTEST: policy learn ok`
- `SELFTEST: policy enforce deny ok`
- `SELFTEST: policy explain ok`

Markers are supporting evidence only; they do not replace real assertions.

## Alternatives considered

- Keep expanding `RFC-0015`:
  - rejected because it would silently turn a completed baseline RFC into a backlog container.
- Introduce a separate policy compiler/daemon:
  - rejected because it creates authority drift and duplicates `configd`/`policyd` roles.
- Add `nx policy` directly inside monolithic `tools/nx/src/lib.rs`:
  - rejected because it increases CLI drift risk and makes follow-up growth less maintainable.

## Open questions

- Which downstream OS-facing adapter should be the first QEMU-gated policy marker owner after the host check cutover?

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0**: `tools/nx` structure refactor lands with unchanged CLI behavior — proof: `cd /home/jenning/open-nexus-OS && cargo test -p nx -- --nocapture`
- [x] **Phase 1**: policy-tree authority + canonical version + evaluator reject semantics land — proof: `cd /home/jenning/open-nexus-OS && cargo test -p policy -- --nocapture`
- [x] **Phase 2**: `policyd` reload/eval/mode + `nx policy` contract land with parity proof — proof: `cd /home/jenning/open-nexus-OS && cargo test -p policyd -- --nocapture && cargo test -p nx -- --nocapture`
- [x] Task(s) linked with stop conditions + proof commands.
- [ ] QEMU markers remain gated and intentionally unclaimed by this host-first closure.
- [x] Security-relevant negative tests exist (`test_reject_*`).
