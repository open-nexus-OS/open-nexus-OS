# Signing and policy enforcement

Open Nexus OS pairs manifest signing with runtime capability policies. Bundle
signatures prove provenance, while the policy layer restricts which processes
may consume specific kernel capabilities.

This is part of the system’s **hybrid security root**: verified boot + signed bundles/packages +
policy gating + capability enforcement (see `docs/agents/VISION.md`).

## Evaluation order

1. **Bundle manifest** – `bundlemgrd` parses the signed manifest and records the
   capabilities declared by the service (`caps = [...]`).
2. **Policy lookup** – `nexus-init` queries `policyd.Check(subject, requiredCaps)`
   before launching a service. Policies are assembled from the TOML files under
   `recipes/policy/`, merged lexically with later files overriding earlier
   entries.
3. **Execution** – only when `policyd` returns `allowed=true` does init request
   execution from `execd`. Denials are logged as `init: deny <name> missing=cap`.

Unknown services default to an empty allowlist, so any non-empty capability
request is denied by default.

## Extending policies

- Add a new `*.toml` file under `recipes/policy/` or update `base.toml`.
- Use lowercase service and capability names; entries are normalised.
- Later files override earlier ones. For temporary developer overrides, drop a
  `local-*.toml` file so it sorts after the base policy.
- Keep policy files in version control whenever possible so QEMU and postflight
  checks can enforce the correct allowlists.

## Denial handling

When a service requests capabilities that are not permitted, `policyd` returns
`allowed=false` along with the missing capability names. `nexus-init` records the
failure as `init: deny <name>` and skips `execd`. Host tests and the OS
postflight harness assert that the denial path is covered by both host E2E
checks and QEMU UART markers.
