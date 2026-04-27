<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Policy as Code v1

## CONTEXT

- Scope: `TASK-0047` / `RFC-0045` host-first Policy-as-Code closure.
- Canonical proof commands:
  - `cargo test -p policy -- --nocapture`
  - `cargo test -p configd -- --nocapture`
  - `cargo test -p policyd -- --nocapture`
  - `cargo test -p nx -- --nocapture`

## Authority Model

`policyd` remains the only policy decision authority. The active policy tree root
is `policies/nexus.policy.toml`; `recipes/policy/` is retained only as a legacy
migration note and must not contain live `*.toml` policy inputs.

Policy authoring remains TOML in v1. The policy crate derives a deterministic
canonical representation for `PolicyVersion`; derived JSON/CLI views are not a
second authority.

`policies/manifest.json` is a deterministic snapshot manifest for the active
tree hash. It is required validation evidence for `nx policy validate`, not a
second authoring source.

## Host-First Guarantees

- Invalid, oversize, ambiguous, traversal, unknown-section, and explain-budget
  failures reject with stable classes.
- Equivalent validated inputs produce the same `PolicyVersion`.
- Evaluation is deny-by-default with bounded explain traces.
- Dry-run and learn observe would-deny decisions but do not grant what enforce
  would deny.
- Config v1 carries the candidate policy root as `policy.root` in the effective
  snapshot; `policyd` stages the resulting `PolicyTree` through the
  `configd::ConfigConsumer` 2PC seam. Invalid candidates do not replace the
  active version.
- `policyd` exposes host frame operations for `Version`, `Eval`, `ModeGet`, and
  `ModeSet` backed by the unified authority, with bounded audit events for
  allow, deny, and reject outcomes.
- The `policyd` service-facing check frame evaluates through `PolicyAuthority`.
- `nx policy validate` requires `policies/manifest.json` and rejects missing or
  stale tree hashes.
- `nx policy mode` is host-side preflight only until a live daemon mode RPC is
  present.
- `nx policy` lives under `tools/nx` and delegates semantics to the shared policy
  crate.

OS/QEMU policy markers remain gated and unclaimed until real OS-lite reload
wiring and real service adapters are present.
