# Supply-Chain v1 SBOM (`meta/sbom.json`)

`TASK-0029` introduces bundle-level SBOM generation for `.nxb` bundles.

## Contract

- SBOM path in bundle: `meta/sbom.json`
- SBOM carrier format: CycloneDX JSON 1.5 (interop format per ADR-0021)
- Deterministic inputs: bundle name/version, publisher, payload digest/size, manifest digest, `SOURCE_DATE_EPOCH`
- Deterministic output: stable key ordering, fixed timestamp source, no wall-clock time

The SBOM is generated during bundle packing (`tools/nxb-pack`) and is treated as part of the install integrity chain.

## Generation

SBOM generation is implemented in `tools/sbom`.

Command form:

```bash
cargo run -p sbom -- \
  <bundle_name> <bundle_version> <publisher_hex> \
  <payload_sha256> <payload_size> <manifest_sha256> <output_path>
```

Example:

```bash
SOURCE_DATE_EPOCH=1700000000 cargo run -p sbom -- \
  demo.bundle 1.0.0 00000000000000000000000000000000 \
  e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855 \
  0 \
  e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855 \
  /tmp/meta/sbom.json
```

## Security and determinism guards

- Secret scanning runs before write; leaks fail the pack step.
- Known deterministic digests are allowlisted to avoid false positives from digest-like fields.
- Timestamps always come from `SOURCE_DATE_EPOCH`; missing/invalid values fail generation.

## Proof commands

```bash
cargo test -p sbom -- determinism
cargo test -p nxb-pack -- supply_chain
```
