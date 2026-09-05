# Legacy policy recipes

Policy as Code v1 migrated the active policy authority to `policies/`.

This directory is retained only as a migration note and must not contain live
`*.toml` policy inputs. If a temporary parity fixture is needed during a future
migration, keep it explicitly test-scoped and do not point `policyd` or `nx
policy` at this directory as an authority.

Active capability policy files live under `policies/` and conform to this
structure:

```toml
[allow]
"service-name" = ["capability.one", "capability.two"]

[abi_profile."service-name"]
epoch = 1

[[abi_profile."service-name".statefs]]
action = "allow"
prefix = "/state/app/service-name/"

[[abi_profile."service-name".net.bind]]
action = "allow"
ports = ["1024-65535"]
address = "loopback"
```

Files are merged in lexical order and later files override earlier entries for
the same service. Service and capability names are normalized to lowercase
before evaluation.

## Adding new policy files

1. Create a new `*.toml` file under `policies/` with your overrides or service additions.
2. List every capability the service requires; omit any optional capabilities.
3. Keep service names consistent with their bundle manifest entries.
4. Add the file to `policies/nexus.policy.toml` so it participates in the
   canonical version hash.

Unknown services default to an empty allowlist, so any non-empty capability
request will be denied unless explicitly permitted.

## ABI profile section (RFC-0091 schema v2)

`[abi_profile."<service>"]` configures the per-subject ABI guardrail profile
served by `policyd`: `epoch`, an optional `limits` table (`deadline_ms`,
`max_payload`), and rule arrays `statefs`, `net.bind`, `net.connect`. The
full grammar, precedence (most specific wins, deny beats allow) and bounds are
documented in `docs/security/abi-filters.md`; the single parser is
`userspace/policy/src/schema.rs` with fixtures under `policies/tests/`.
Legacy v1 keys (`statefs_put_allow_prefix`, `net_bind_min_port`) still parse
(transcoded, `epoch = 0`) but must not be mixed with v2 sections.

Profiles are static per boot; the only runtime transition is the
authenticated, epoch-guarded mode switch (RFC-0091 §6).
