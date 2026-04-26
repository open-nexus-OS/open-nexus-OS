---
title: TASK-0047 Policy as Code v1: unified policy tree + evaluator + explain/dry-run + learn→enforce (+ nx policy)
status: In Progress
owner: @runtime
created: 2025-12-22
depends-on:
  - TASK-0006
  - TASK-0046
follow-up-tasks:
  - TASK-0043
  - TASK-0136
  - TASK-0189
  - TASK-0229
  - TASK-0230
  - TASK-0266
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - RFC: docs/rfcs/RFC-0045-policy-as-code-v1-unified-policy-tree-evaluator-explain-dry-run-learn-enforce-nx-policy.md
  - Production gates: tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md
  - Depends-on (config system): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Depends-on (config contract): docs/rfcs/RFC-0044-config-v1-configd-schema-layering-2pc-host-first-os-gated.md
  - Depends-on (audit sink): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Policy architecture baseline: docs/adr/0014-policy-architecture.md
  - Service architecture: docs/adr/0017-service-architecture.md
  - Structured formats: docs/adr/0021-structured-data-formats-json-vs-capnp.md
  - Existing policy service: source/services/policyd/
  - ABI filters v2: tasks/TASK-0028-abi-filters-v2-arg-match-learn-enforce.md
  - DSoftBus ACL hardening: tasks/TASK-0030-dsoftbus-discovery-authz-hardening-mdns-ttl-acl-ratelimit.md
  - Sandboxing v1: tasks/TASK-0039-sandboxing-v1-vfs-namespaces-capfd-manifest.md
  - Sandbox quotas/egress: tasks/TASK-0043-security-v2-sandbox-quotas-egress-abi-audit.md
  - Supply-chain policy: tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md
  - DevX CLI: tasks/TASK-0045-devx-nx-cli-v1.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We currently have multiple independent policy “islands” (or planned ones):

- policyd allow/deny checks (caps + routing/exec checks),
- ABI filters (learn/enforce),
- DSoftBus ACL,
- sandbox quotas and net egress rules,
- signing policy,
- tracing/metrics sampling and gatekeeping.

This creates drift and inconsistent operator experience. We want **Policy as Code**:

- a single versioned policy tree,
- one evaluator model with explain traces and dry-run,
- unified learn→enforce workflow,
- tooling (`nx policy`) to validate/diff/explain and manage modes.

Repo reality today:

- A `policyd` service already exists (host and os-lite paths). We must **extend** it, not introduce a second “policy authority”.
- `userspace/policy/` and `recipes/policy/` already implement the current host/build-time policy seam. v1 must converge this seam instead of creating a parallel `nexus-policy` authority.
- Config v1 (`configd` from `TASK-0046` / `RFC-0044`) now exists and owns deterministic reload/versioning. Policy as Code must consume that authority rather than invent alternate disk, reload, or CLI conventions.
- Future management note:
  - the same unified policy tree should later be able to serve household, school, enterprise, fleet, kiosk, and
    smart-device management inputs, with local enforcement still remaining in `policyd` and related authorities.

This task is **host-first** and **OS-gated**.

## Goal

Deliver Policy as Code v1 such that:

- policy is stored as a versioned tree, schema validated and hash-addressed,
- the policy library/evaluator seam (extending `userspace/policy/`) evaluates decisions with an explain trace,
- `policyd` exposes `Eval`, explain-capable decisions, and mode control (learn/enforce) and reloads policy safely via `configd` 2PC,
- adapters in services switch from ad-hoc checks to the unified evaluator incrementally,
- host tests prove determinism; OS/QEMU markers are added only when the involved services are real.

## Non-Goals

- Kernel changes.
- A perfect “one format to rule them all” on day 1. v1 focuses on unification and determinism, not feature completeness.
- Making policy evaluation a hot-path bottleneck (must be bounded and cacheable).

## Constraints / invariants (hard requirements)

- Kernel untouched.
- Deterministic evaluation:
  - stable precedence rules,
  - canonicalization for hashing,
  - bounded explain traces.
- Bounded memory:
  - cap rule counts per domain,
  - cap explain trace length,
  - cap learn log size and rotation.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success markers.

## Red flags / decision points

- **RED (single authority)**:
  - `policyd` remains the single policy authority. We must not ship `abi-filterd` + `policyd` + `configd` as competing authorities.
  - v1 can keep “guardrails” (ABI filters, vfsd enforcement) but they must consult the unified policy tree.
- **RED (repo-fit migration)**:
  - Do not land `policies/` and `recipes/policy/` as dual live roots.
  - The task must either migrate the existing policy input root fully or prove a bounded importer/compat layer and remove authority ambiguity before closure.
- **YELLOW (schema format choice)**:
  - If TOML remains the human authoring format for v1, canonical JSON must stay a **derived** representation for hashing/schema/debug, not a second authoring authority.
  - Do not introduce YAML or a parallel “JSON compiler” authority. If we need a compact OS-friendly artifact, it must be produced as a **snapshot output** of the same policy tree (tracked in `TASK-0229`).
- **YELLOW (adapters migration)**:
  - Migration must be incremental: do not require converting all policies at once.
  - Each adapter must preserve existing behavior until the new evaluator proves parity.

## Production-grade gate note

This task sits on the Gate B path (`Security, Policy & Identity`, `production-grade`) even though the
execution style stays host-first and OS-gated.

- `TASK-0046` already closed Gate J config/tooling preconditions for reload/versioning.
- `TASK-0047` must therefore describe a **real canonical policy authority** with deterministic negative
  proofs, audit visibility, and no authority drift.
- Follow-up tasks may extend domains and OS consumption, but this task must already lock the single-authority,
  deny-by-default, explainable evaluator contract that Gate B depends on.

## Unified policy tree (v1 shape)

Directory layout (single live entrypoint after migration):

- `policies/nexus.policy.toml` (root include/import list)
- `policies/abi/*.toml`
- `policies/dsoftbus-acl/*.toml`
- `policies/sandbox/*.toml`
- `policies/egress/*.toml`
- `policies/signing/*.toml`
- `policies/observability/{tracing,metrics}.toml`

Migration note:

- Existing `recipes/policy/*.toml` is the current repo baseline.
- v1 may use a temporary importer/parity layer during migration, but closure must leave exactly one live
  policy tree authority and deterministic parity tests for the migrated domains.

Snapshot manifest:

- `policies/manifest.json` with `version=1` and `tree_sha256` (generated_at_ns must be 0 for determinism).

## Contract sources (single source of truth)

- Config distribution + 2PC apply: TASK-0046
- Existing policyd semantics: `source/services/policyd`
- Policy architecture baseline: ADR-0014
- Service boundary / single-authority rules: ADR-0017
- Canonical structured-format rules: ADR-0021

## Security considerations

### Threat model

- **Authority drift**: multiple live policy roots or loaders produce conflicting decisions.
- **Identity spoofing**: adapters pass string subjects instead of kernel-derived identities/service ids.
- **Mode abuse**: unauthenticated learn/enforce transitions weaken enforcement or hide who changed mode.
- **Explain/log blowup**: attacker-crafted requests or policy trees force unbounded traces or learn logs.
- **Policy poisoning**: imported/migrated policy files change semantics silently during the move from current
  `recipes/policy` inputs to the unified tree.

### Security invariants (MUST hold)

- `policyd` remains the single decision authority; adapters query it instead of embedding duplicate policy logic.
- Reload/version transitions are driven by `configd` 2PC; reject/timeout keeps the previous policy version active.
- Effective identity is kernel-derived (`sender_service_id`, service id, or another provenance-safe runtime identity), not trusted payload text.
- Evaluation is deny-by-default, deterministic, and bounded in rule count, trace length, and learn-log emission.
- Explain and audit outputs must never leak secrets or fabricate success.
- Dry-run and learn mode may observe would-deny decisions, but must never grant access that enforce mode would deny.

### DON'T DO

- DON'T ship both `recipes/policy` and `policies/` as active authorities.
- DON'T add a separate policy daemon, compiler, or `nx-*` CLI outside `policyd` + `tools/nx`.
- DON'T trust adapter-supplied string identities in OS-facing enforcement paths.
- DON'T let learn mode or dry-run bypass deny rules.
- DON'T use marker-only QEMU closure; markers must follow real assertions or verified state changes.

### Security proof

Host proofs must include deterministic reject-path coverage, for example:

- `test_reject_unauthenticated_mode_change`
- `test_reject_oversize_or_invalid_policy_tree`
- `test_reject_unbounded_explain_trace`
- `test_reject_adapter_bypass_or_unknown_subject`
- parity tests proving a migrated adapter keeps the old allow/deny behavior before cutover is declared complete

## Stop conditions (Definition of Done)

### Proof (Host) — required

Add deterministic host tests (`tests/policy_host/`):

- load + validate policy tree → stable `PolicyVersion`
- invalid/oversize/ambiguous policy tree → stable reject classification (`test_reject_*`)
- eval allow/deny across at least 3 domains (abi, egress, signing) with explain trace containing expected steps
- dry-run: returns allow but emits a would-deny record (bounded)
- learn log normalization is deterministic; generator produces stable output for stable input.
- authenticated mode change + stale/unauthorized reject coverage are deterministic
- one migrated adapter proves behavior parity before/after unified evaluator cutover

### Proof (OS / QEMU) — gated

Once `policyd` os-lite + `configd` + at least one adapter exist:

- `policyd: ready (ver=<hex8>)`
- `policy: tree loaded (ver=1 sha=<hex8>)`
- `policy: adapters active (domains=...)`
- `SELFTEST: policy learn ok`
- `SELFTEST: policy enforce deny ok`
- `SELFTEST: policy explain ok`

Notes:

- Postflight scripts must delegate to canonical harness/tests; do not invent log-grep success semantics.
- Markers summarize already-asserted behavior; they are not the primary proof.

## Touched paths (allowlist)

- `policies/` (new)
- `schemas/policy/` (new, optional but preferred)
- `recipes/policy/` (migration/parity input only; do not leave as a second live authority)
- `userspace/policy/` (extend existing crate; rename only if explicitly justified during RFC/task execution)
- `source/services/policyd/` (extend: Eval + explain + reload + mode)
- `tools/nx/` (`nx policy ...` subcommands; follow-up in TASK-0045)
- `tools/policy-gen/` (optional generator)
- `tests/`
- `docs/security/policy-as-code.md`
- Policy snapshot artifacts (stable JSON + compact binary) are a follow-up tracked in `TASK-0229`.

## Plan (small PRs)

0. **Preparation: `tools/nx` structure refactor before `nx policy` growth**
   - split the current `tools/nx/src/lib.rs` into a small, stable module tree before adding `nx policy`
   - chosen folder structure:
     - `tools/nx/src/lib.rs` — thin entrypoint (`run()`, dispatch wiring, module exports)
     - `tools/nx/src/cli.rs` — `clap` CLI shape (`Cli`, `Commands`, `*Args`, `*Action`)
     - `tools/nx/src/error.rs` — `ExitClass`, `NxError`, `ExecResult`
     - `tools/nx/src/output.rs` — output envelope + human/JSON printing
     - `tools/nx/src/runtime.rs` — shared runtime config and common repo/env helpers
     - `tools/nx/src/commands/mod.rs` — command module routing
     - `tools/nx/src/commands/new.rs`
     - `tools/nx/src/commands/inspect.rs`
     - `tools/nx/src/commands/idl.rs`
     - `tools/nx/src/commands/postflight.rs`
     - `tools/nx/src/commands/doctor.rs`
     - `tools/nx/src/commands/dsl.rs`
     - `tools/nx/src/commands/config.rs`
     - `tools/nx/src/commands/policy.rs` — added in this task as the new Policy-as-Code command surface
   - keep CLI behavior unchanged during the refactor; this is a structure-first move, not a UX rewrite
   - do not introduce a second binary or `nx-*` drift; all policy tooling remains under `tools/nx`

1. **Repo-fit authority convergence**
   - choose the single live root (`policies/` end state, with bounded migration from `recipes/policy/` if needed)
   - extend the existing `userspace/policy/` seam instead of creating a parallel authority crate
   - prove current-domain parity before declaring migration complete

2. **Policy tree + canonicalization**
   - define root file and includes
   - parse TOML into a canonical JSON tree
   - compute `PolicyVersion = sha256(canonical_json)`

3. **Policy library/evaluator**
   - evaluators as pure functions returning:
     - `Decision { allow, reason_code, trace }`
   - bounded explain traces (max steps)

4. **Extend `policyd`**
   - `Version()`, `Eval(...)`, `ModeGet/Set(subject, learn|enforce)`
   - reload via `configd` 2PC using the already-landed Config v1 authority

5. **Adapters (incremental)**
   - pick 1–2 early adapters to prove the model:
     - signing allowlist checks in bundle install path
     - net egress allowlist guardrail (host-only at first)
   - add `POLICY_DRY_RUN=1` to observe would-denies without breaking boot
   - app capability matrix adapters (clipboard/content/intents/notifs, foreground-only): see `TASK-0136`

6. **DevX**
   - `nx policy validate/diff/explain/mode`
   - implement on top of the prepared `tools/nx/src/{cli,error,output,runtime,commands/}` structure rather than regrowing a monolithic `lib.rs`
   - generator tool optional (learn → conservative TOML stubs)

7. **Docs**
   - tree layout, versioning, explain/dry-run, learn→enforce workflow
