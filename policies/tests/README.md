# `abi_profile` schema corpus (RFC-0091 §1)

Shared grammar fixtures for BOTH parsers — the host `policy` crate
(`userspace/policy/src/schema.rs`) and policyd's build-time table
(`source/services/policyd/build.rs`, which includes the same `schema.rs`
and `expose.rs` by path; ingressd's `build.rs` compiles its exposure table
from the same `expose.rs`). `userspace/policy/tests/schema_corpus.rs` runs every file:

- `ok_*.toml` must parse and compile (`[abi_profile]`, `[quota]` and `[[expose]]` sections, RFC-0091 / RFC-0072 amendment / RFC-0092);
- `reject_*.toml` must fail with the `SchemaError` named in the file's
  first `# expect:` line (or `parse` for a TOML-level reject).

These files are NOT loaded by the policy root (`nexus.policy.toml`
includes only `base.toml`).
