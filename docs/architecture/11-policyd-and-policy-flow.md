# `policyd` + policy flow — onboarding

`policyd` is the **policy authority**: it decides whether a subject (service/app) is allowed to use specific capabilities.

Canonical sources:

- Policy overview: `docs/adr/0014-policy-architecture.md`
- RFC (policy authority + audit): `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md`
- RFC (Policy as Code v1): `docs/rfcs/RFC-0045-policy-as-code-v1-unified-policy-tree-evaluator-explain-dry-run-learn-enforce-nx-policy.md`
- Security overview: `docs/security/policy-as-code.md`
- End-to-end flow (signing + policy + init): `docs/security/signing-and-policy.md`
- Service architecture context: `docs/adr/0017-service-architecture.md`
- Testing guide: `docs/testing/index.md`

## Responsibilities

- **Load and validate policy** from the versioned policy tree rooted at `policies/nexus.policy.toml`.
- **Validate manifest evidence** through `policies/manifest.json` for `nx policy validate`.
- **Canonicalize subjects and capability names** (avoid case/whitespace drift).
- **Answer checks**: `allowed=true/false` + missing capability list for debuggability.
- **Evaluate through `PolicyAuthority`** for service-facing check frames and host `Eval`.
- **Emit bounded audit events** for eval, mode, and reload allow/deny/reject outcomes.
- **Apply reload candidates through Config v1**: `configd` carries `policy.root` in the effective snapshot; `policyd` consumes it via `configd::ConfigConsumer`.

## Policy Evaluation Model (TASK-0008)

Policy is capability-based with service-id binding:

```text
service_id (u64) + action (str) + target (optional str) → Decision (Allow | Deny)
```

**Identity binding**: `service_id` is derived from the kernel-provided `sender_service_id` on IPC.
Requester name in payloads is display-only and MUST NOT grant authority.

**Deny-by-default**: Operations without explicit policy allow are rejected.

**Mode honesty**: dry-run and learn observe would-deny decisions but do not grant what enforce mode denies.

**CLI honesty**: `nx policy mode` validates host-side preconditions only. It does not claim to change a live daemon mode until a real mode RPC exists.

## Policy Tree + Reload Flow

```text
policies/nexus.policy.toml + includes
    -> userspace/policy validates + computes PolicyVersion
    -> policies/manifest.json records the required deterministic tree hash
    -> configd effective snapshot carries candidate root as policy.root
    -> policyd ConfigConsumer prepares candidate PolicyTree
    -> configd 2PC commit applies the new active policy version
```

`recipes/policy/` is legacy documentation/migration context only. It must not be reintroduced as a live TOML authority.

### Policy-Gated Operations

| Operation | Required Capability | Service |
|-----------|---------------------|---------|
| Sign (Ed25519) | `crypto.sign` | keystored |
| Bundle install | `fs.verify` | bundlemgrd |
| Process exec | `proc.spawn` | execd |
| Route request | `ipc.core` | policyd (via init-lite proxy) |

## Audit Trail

Every policy decision emits an audit record:

| Field | Type | Description |
|-------|------|-------------|
| `timestamp_ns` | u64 | Nanoseconds since boot |
| `subject_id` | u64 | `sender_service_id` of requester |
| `action` | str (bounded) | Operation type (e.g., "route", "exec", "sign") |
| `target` | str (bounded, optional) | Target service/resource |
| `decision` | u8 | 0 = Allow, 1 = Deny |
| `reason` | u8 | Reason code (0 = policy, 1 = identity mismatch, 2 = malformed) |

**Sink priority**: logd (RFC-0011) first, UART fallback if logd unavailable.

## Where policy is enforced

The policy decision is used in multiple places, but the key boot-time gate is:

- `nexus-init` queries `policyd` before launching a service that requests capabilities.
- Host/std `policyd` exposes frame operations for `Version`, `Eval`, `ModeGet`, and `ModeSet`.
- The service-facing check frame is proven to route through `PolicyAuthority`.

This is part of the "hybrid security root" strategy:

- Signed bundles/packages + policy gating + capability enforcement (kernel enforces rights on held caps).

## Denials are first-class proofs

Denials must be deterministic and explicit:

- They are validated in host unit/CLI contract tests first.
- QEMU smoke markers remain gated until real OS-lite reload wiring and adapter markers exist.
- Every denial should be audit-logged by the production sink; host tests prove bounded audit event shape.

See the testing matrix in `docs/testing/index.md` for how these are exercised.

### QEMU Proof Markers (Gated)

| Marker | Proves |
|--------|--------|
| `SELFTEST: policy deny audit ok` | A deny decision occurred AND audit record was emitted |
| `SELFTEST: policy allow audit ok` | An allow decision occurred AND audit record was emitted |
| `SELFTEST: keystored sign denied ok` | Policy-gated signing denied without required capability |

Run: `RUN_PHASE=policy RUN_TIMEOUT=190s just test-os`

These markers are not currently claimed by `TASK-0047`; they require OS-facing wiring.

## Drift-resistant rules

- Don't create multiple "policy authorities" or shadow allowlists. `policyd` is the authority.
- Don't invent a new on-disk policy format without an RFC/ADR and a task with proof gates.
- Don't restore `recipes/policy/` as a live policy root.
- Don't add a policy-specific file watcher/reload path; policy reload candidates flow through Config v1.
- Don't treat `nx policy mode` as a live daemon mutation until a real RPC is implemented and tested.
- Don't trust subject identity from payload bytes (use kernel-provided `sender_service_id`).
- Keep "where the truth lives" clear:
  - contracts/semantics: ADR/RFC
  - "what is green": tasks + tests/harness
