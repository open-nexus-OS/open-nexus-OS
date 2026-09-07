# `abi_profile` schema corpus (RFC-0091 §1)

Shared grammar fixtures for BOTH parsers — the host `policy` crate
(`userspace/policy/src/schema.rs`) and policyd's build-time table
(`source/services/policyd/build.rs`, which includes the same `schema.rs`
by path). `userspace/policy/tests/schema_corpus.rs` runs every file:

- `ok_*.toml` must parse and compile (`[abi_profile]` and `[quota]` sections, RFC-0091 / RFC-0072 amendment);
- `reject_*.toml` must fail with the `SchemaError` named in the file's
  first `# expect:` line (or `parse` for a TOML-level reject).

These files are NOT loaded by the policy root (`nexus.policy.toml`
includes only `base.toml`).
