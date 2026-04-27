# ADR-0014: Policy Architecture

## Status
Accepted

## Context
The policy system provides capability-based access control for Open Nexus OS. It determines which subjects (users, services, applications) have access to which capabilities based on versioned policy documents.

`TASK-0047` / `RFC-0045` superseded the original recipe-directory baseline with Policy as Code v1. The current host-first contract keeps `policyd` as the single authority, moves live authoring to `policies/`, routes policy reload through Config v1, and exposes deterministic `nx policy` tooling under the existing `tools/nx` binary.

## Decision
Implement a versioned policy-tree system with the following architecture:

### Core Components
- **Policy Tree**: TOML-authored root at `policies/nexus.policy.toml` with explicit includes.
- **Policy Manifest**: `policies/manifest.json`, a deterministic tree-hash evidence artifact required by `nx policy validate`.
- **Policy Evaluation Library**: `userspace/policy` — pure library for canonical loading, `PolicyVersion`, bounded evaluation, explain traces, and stable reject classes.
- **Capability Checker**: Runtime capability validation via `policyd` backed by `PolicyAuthority`.
- **Config Reload Integration**: Config v1 effective snapshots carry candidate roots as `policy.root`; `policyd` consumes them via `configd::ConfigConsumer` 2PC.
- **DevX Surface**: `nx policy validate|diff|explain|mode` under `tools/nx` only; no `nx-*` split.
- **Subject Canonicalization**: Consistent subject name handling via `service_id`
- **Audit Trail**: Policy eval/lifecycle decisions produce bounded audit events; OS logd integration remains the production sink goal.

### Policy Format
- **File Format**: TOML files included from `policies/nexus.policy.toml`
- **Subject Mapping**: Maps service names to capability lists
- **Capability Lists**: Arrays of capability names
- **Canonical Version**: `PolicyVersion = sha256(canonical policy JSON)`
- **Manifest Evidence**: `policies/manifest.json` must match the canonical tree hash
- **Legacy Recipes**: `recipes/policy/` is documentation/migration-only and must not contain live TOML authority

### Security Model (TASK-0008)
- **Capability-Based**: Access control based on declared capabilities
- **Service-ID Binding**: Policy decisions bind to `sender_service_id` (kernel-provided, unforgeable)
- **Deny-by-Default**: Operations without explicit policy allow are rejected
- **Single Authority**: `policyd` is the sole decision service (no duplication)
- **Learn/Dry-run Honesty**: Learn/dry-run observe would-deny decisions but do not grant what enforce mode denies
- **Audit Trail**: Every allow/deny/reject lifecycle decision produces bounded audit evidence
- **Bounded Parsing**: Inputs are size-bounded; malformed requests rejected

### Integration Points
- **Service Manager (samgrd)**: Check service capabilities
- **Bundle Manager (bundlemgrd)**: Validate bundle capability requirements
- **Exec Daemon (execd)**: Authorize process spawn via policyd
- **Keystore (keystored)**: Policy-gated signing operations
- **Init-lite**: Proxy policy checks during early boot

## Consequences
- **Positive**: Fine-grained access control
- **Positive**: Policy changes flow through Config v1 2PC instead of a parallel reload plane
- **Positive**: Clear denial information for debugging
- **Positive**: Complete audit trail of all security decisions
- **Negative**: Policy complexity can grow
- **Negative**: OS/QEMU closure still requires real OS-lite reload wiring and adapter markers before being claimed

## Implementation Notes
- Live policy root is `policies/nexus.policy.toml`; `recipes/policy/` is legacy-only.
- Subject names are canonicalized (lowercase, trimmed) and converted to `service_id`
- Capability names are canonicalized for consistent matching
- Policy includes allow incremental policy updates with deterministic versioning
- Denial information includes specific missing capabilities
- Host `policyd` frame operations cover `Version`, `Eval`, `ModeGet`, `ModeSet`, and service-facing check frames.
- `nx policy mode` is host preflight-only until a live daemon mode RPC exists.

## Related RFCs
- RFC-0015: Policy Authority & Audit Baseline v1 (contract + proof gates)
- RFC-0045: Policy as Code v1 (unified policy tree + evaluator + explain/dry-run + learn→enforce + `nx policy`)
