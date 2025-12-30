---
title: TASK-0047 Policy as Code v1: unified policy tree + evaluator + explain/dry-run + learn→enforce (+ nx policy)
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Depends-on (config system): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Depends-on (audit sink): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
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
- Hot reload and versioning should be driven by `configd` (TASK-0046) once it exists.

This task is **host-first** and **OS-gated**.

## Goal

Deliver Policy as Code v1 such that:

- policy is stored as a versioned tree, schema validated and hash-addressed,
- `nexus-policy` library evaluates decisions with an explain trace,
- `policyd` exposes `Eval` and mode control (learn/enforce) and can reload policy safely (2PC via configd),
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
- **YELLOW (schema format choice)**:
  - If we keep policy files as TOML, we still need a canonical intermediate representation (canonical JSON tree) for hashing and schema validation.
  - Avoid introducing a parallel “YAML → JSON → bin” policy compiler as a separate authority. If we need a compact OS-friendly artifact,
    it must be produced as a **snapshot output** of the same policy tree (tracked in `TASK-0229`).
- **YELLOW (adapters migration)**:
  - Migration must be incremental: do not require converting all policies at once.
  - Each adapter must preserve existing behavior until the new evaluator proves parity.

## Unified policy tree (v1 shape)

Directory layout (single entrypoint):

- `policies/nexus.policy.toml` (root include/import list)
- `policies/abi/*.toml`
- `policies/dsoftbus-acl/*.toml`
- `policies/sandbox/*.toml`
- `policies/egress/*.toml`
- `policies/signing/*.toml`
- `policies/observability/{tracing,metrics}.toml`

Snapshot manifest:

- `policies/manifest.json` with `version=1` and `tree_sha256` (generated_at_ns must be 0 for determinism).

## Contract sources (single source of truth)

- Config distribution + 2PC apply: TASK-0046
- Existing policyd semantics: `source/services/policyd`

## Stop conditions (Definition of Done)

### Proof (Host) — required

Add deterministic host tests (`tests/policy_host/`):

- load + validate policy tree → stable `PolicyVersion`
- eval allow/deny across at least 3 domains (abi, egress, signing) with explain trace containing expected steps
- dry-run: returns allow but emits a would-deny record (bounded)
- learn log normalization is deterministic; generator produces stable output for stable input.

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

## Touched paths (allowlist)

- `policies/` (new)
- `schemas/policy/` (new, optional but preferred)
- `userspace/security/nexus-policy/` (new crate)
- `source/services/policyd/` (extend: Eval + explain + reload + mode)
- `tools/nx/` (`nx policy ...` subcommands; follow-up in TASK-0045)
- `tools/policy-gen/` (optional generator)
- `tests/`
- `docs/security/policy-as-code.md`
 - Policy snapshot artifacts (stable JSON + compact binary) are a follow-up tracked in `TASK-0229`.

## Plan (small PRs)

1. **Policy tree + canonicalization**
   - define root file and includes
   - parse TOML into a canonical JSON tree
   - compute `PolicyVersion = sha256(canonical_json)`

2. **`nexus-policy` library**
   - evaluators as pure functions returning:
     - `Decision { allow, reason_code, trace }`
   - bounded explain traces (max steps)

3. **Extend `policyd`**
   - `Version()`, `Eval(...)`, `ModeGet/Set(subject, learn|enforce)`
   - reload via configd 2PC once configd exists; until then host reads from disk

4. **Adapters (incremental)**
   - pick 1–2 early adapters to prove the model:
     - signing allowlist checks in bundle install path
     - net egress allowlist guardrail (host-only at first)
   - add `POLICY_DRY_RUN=1` to observe would-denies without breaking boot
   - app capability matrix adapters (clipboard/content/intents/notifs, foreground-only): see `TASK-0136`

5. **DevX**
   - `nx policy validate/diff/explain/mode`
   - generator tool optional (learn → conservative TOML stubs)

6. **Docs**
   - tree layout, versioning, explain/dry-run, learn→enforce workflow
