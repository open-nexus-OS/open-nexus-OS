# Supply-Chain v1 Repro Metadata (`meta/repro.env.json`)

`TASK-0029` defines a schema-versioned reproducibility artifact embedded in each bundle.

## Contract

- Repro metadata path in bundle: `meta/repro.env.json`
- In-bundle format: JSON only (human-readable text output is CLI-only and not contract input)
- Current schema: `schema_version = 1`
- Timestamp authority: `SOURCE_DATE_EPOCH` only

`meta/repro.env.json` is used by install-time checks to validate integrity links between manifest, payload, and SBOM digests.

## Capture and verify tools

`tools/repro` provides two subcommands:

```bash
# Capture repro metadata from staged bundle artifacts
cargo run -p repro -- capture <manifest.nxb> <payload.elf> <meta/sbom.json> <meta/repro.env.json>

# Verify schema + expected digest bindings
cargo run -p repro -- verify <meta/repro.env.json> <payload_sha256> <manifest_sha256> <sbom_sha256>
```

## Schema notes (v1)

The schema captures deterministic build context and digest bindings. Unknown or malformed fields are rejected by `repro-verify` in v1 reject-path tests.

## Security and determinism guards

- Secret scanning runs on the generated JSON before acceptance.
- Repro verification is fail-closed for schema mismatch or digest mismatch.
- Metadata generation does not use wall-clock time.

## Proof commands

```bash
cargo test -p repro -- verify
cargo test -p bundlemgrd -- supply_chain
cargo test -p bundlemgrd test_reject_repro_schema_invalid
```
