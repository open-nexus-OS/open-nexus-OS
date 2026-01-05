# `policyd` + policy flow — onboarding

`policyd` is the **policy authority**: it decides whether a subject (service/app) is allowed to use specific capabilities.

Canonical sources:

- Policy overview: `docs/adr/0014-policy-architecture.md`
- End-to-end flow (signing + policy + init): `docs/security/signing-and-policy.md`
- Service architecture context: `docs/adr/0017-service-architecture.md`
- Testing guide: `docs/testing/index.md`

## Responsibilities

- **Load and merge policy** from the policy directory (TOML files under `recipes/policy/`).
- **Canonicalize subjects and capability names** (avoid case/whitespace drift).
- **Answer checks**: `allowed=true/false` + missing capability list for debuggability.

## Where policy is enforced

The policy decision is used in multiple places, but the key boot-time gate is:

- `nexus-init` queries `policyd` before launching a service that requests capabilities.

This is part of the “hybrid security root” strategy:

- Signed bundles/packages + policy gating + capability enforcement (kernel enforces rights on held caps).

## Denials are first-class proofs

Denials must be deterministic and explicit:

- They are validated in host E2E tests (policy E2E harness).
- They are validated in QEMU smoke runs via stable UART markers.

See the testing matrix in `docs/testing/index.md` for how these are exercised.

## Drift-resistant rules

- Don’t create multiple “policy authorities” or shadow allowlists. `policyd` is the authority.
- Don’t invent a new on-disk policy format without an RFC/ADR and a task with proof gates.
- Keep “where the truth lives” clear:
  - contracts/semantics: ADR/RFC
  - “what is green”: tasks + tests/harness
