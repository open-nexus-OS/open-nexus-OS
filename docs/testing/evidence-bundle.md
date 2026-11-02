---
schema: 1
title: "Evidence bundle"
role: spec
subsystem: devx
sources: [source/libs/nexus-evidence, tools/seal-evidence.sh, tools/verify-evidence.sh, tools/gen-ci-key.sh, tools/gen-bringup-key.sh, keys/README.md]
contracts: [RFC-0038, ADR-0027]
verified: 2026-11-02
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Evidence bundle

> **Purpose** — the normative format of a sealed evidence bundle: layout, schema, canonical hash, signature, key classes, secret scan and the harness gates.
> **Owns** — `source/libs/nexus-evidence/` (host-only library + `nexus-evidence` CLI), `tools/seal-evidence.sh`, `tools/verify-evidence.sh`
> **Contracts** — [RFC-0038](../rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md) § Phase 5 (signed evidence bundles; this page extends its hash to cover the UART), [ADR-0027](../adr/0027-selftest-client-two-axis-architecture.md) (why the crate is host-only)
> **Invariants** — the canonical hash order is fixed; reordering is a schema break. Private keys never enter the repo or a bundle. Under `CI=1` the audit trail cannot be switched off.
> **Read next** — [proof-manifest.md](proof-manifest.md), [replay-and-bisect.md](replay-and-bisect.md)

A bundle states: build `<git-sha>` under profile `<P>` against manifest `<H>` produced ladder `<T>`,
attested by key `<K>`. Every successful `just test-os <profile>` and every `tools/os2vm.sh` node
assembles one; it is sealed when sealing is required (§9). The Rust types in `source/libs/nexus-evidence/src/lib.rs` are the schema source;
the tables mirror them.

## 1. Layout

```
target/evidence/<utc>-<profile>-<gitsha>.tar.gz         # scripts/qemu-test.sh
target/evidence/<utc>-<profile>-<gitsha>-<a|b>.tar.gz   # tools/os2vm.sh, per node
```

`target/evidence/` is gitignored. Entries, in lexicographic order:

| Entry | Content | In the hash |
|---|---|---|
| `config.json` | full `ConfigArtifact` incl. `wall_clock_utc` | yes, without `wall_clock_utc` |
| `manifest.tar` | inner tar of the proof-manifest tree (`manifest.toml`, `phases.toml`, `markers/*.toml`, `profiles/*.toml`), paths relative to the manifest root's parent | yes, bytes as-is |
| `meta.json` | `{"schema_version": 1, "profile": "<name>"}` — read first by verifiers | yes |
| `signature.bin` | present once sealed | no (it is the signature) |
| `trace.jsonl` | one `TraceEntry` per line, UART appearance order | yes, sorted |
| `uart.log` | raw serial output, line endings kept on disk | yes, normalized |

**Reproducibility.** Every tar header (outer and inner): `mtime=0`, `uid=gid=0`, `mode=0o644`,
empty `uname`/`gname`, regular entries; the gzip header has `mtime=0`. Two assemblies of the same
inputs on the same host are byte-identical. Two seals of one run with different `wall_clock_utc`
differ in bytes but share the canonical hash.

## 2. Schema

### `BundleMeta`

| Field | Type | Notes |
|---|---|---|
| `schema_version` | `u8` | `1`; append-only |
| `profile` | `String` | a profile name `nexus-proof-manifest` accepts |

### `ManifestArtifact`, `UartArtifact`

`bytes: Vec<u8>` each. The manifest is hashed verbatim; the UART is normalized `\r\n` → `\n` at
hash time only.

### `TraceEntry` (`trace.jsonl`)

| Field | Type | Notes |
|---|---|---|
| `marker` | `String` | the exact manifest literal |
| `phase` | `String` | from the manifest declaration, never from UART position |
| `ts_ms_from_boot` | `Option<u64>` | from a `[ts=<n>ms] ` prefix when present |
| `profile` | `String` | constant per bundle, stored per entry |

### `ConfigArtifact` (`config.json`)

| Field | Type | Hashed | Notes |
|---|---|---|---|
| `profile` | `String` | yes | equals `BundleMeta.profile` |
| `env` | `BTreeMap<String,String>` | yes | the `--env` pairs (`scripts/qemu-test.sh` passes `PROFILE` and the `REQUIRE_SMP`/`_QEMU_DHCP`/`_DSOFTBUS` gates), sorted keys |
| `kernel_cmdline` | `String` | yes | what the harness passed |
| `qemu_args` | `Vec<String>` | yes | argv order kept |
| `host_info` | `String` | yes | host identity (`uname -srm` from the harness) |
| `build_sha` | `String` | yes | `git rev-parse HEAD` |
| `rustc_version` | `String` | yes | `rustc --version` |
| `qemu_version` | `String` | yes | first line of `qemu-system-riscv64 --version` |
| `wall_clock_utc` | `String` | **no** | RFC 3339 seal time |

`Bundle.signature` is `Option<Signature>`.

## 3. Canonical hash

```
H(x)   = SHA-256(x)
H_root = SHA-256( H(meta_canonical) || H(manifest_bytes) || H(uart_normalized)
                  || H(trace_canonical) || H(config_canonical) )
```

Five 32-byte digests in this fixed order. Covering the UART means any byte change in any hashed
artifact fails verification.

| Term | Encoding |
|---|---|
| `meta_canonical` | UTF-8 `schema_version=<u8>\nprofile=<profile>\n` |
| `manifest_bytes` | `manifest.tar` bytes, no normalization |
| `uart_normalized` | UART bytes with `\r\n` → `\n`; trailing newline kept |
| `trace_canonical` | entries sorted by `(marker, phase)` as raw bytes; each `{"marker":…,"phase":…,"ts_ms_from_boot":<u64\|null>,"profile":…}` in that key order, `serde_json` escaping, joined by `\n`, no trailing `\n` |
| `config_canonical` | compact JSON, keys in order `profile, env, kernel_cmdline, qemu_args, host_info, build_sha, rustc_version, qemu_version`; `wall_clock_utc` omitted; `env` keys sorted |

Invariants, each locked in `source/libs/nexus-evidence/tests/canonical_hash.rs`: trace reorder,
env key order, UART line endings and `wall_clock_utc` do not change `H_root`; any manifest byte does.

## 4. Assembly and trace extraction

```text
nexus-evidence assemble --uart=<uart.log> --manifest=<proof-manifest/manifest.toml> \
  --profile=<name> --out=target/evidence/<utc>-<profile>-<gitsha>.tar.gz \
  [--kernel-cmdline=<s>] [--qemu-arg=<a>]... [--host-info=<s>] [--build-sha=<sha>] \
  [--rustc-version=<s>] [--qemu-version=<s>] [--wall-clock=<rfc3339>] [--env=<K=V>]...
nexus-evidence inspect        <bundle.tar.gz>
nexus-evidence canonical-hash <bundle.tar.gz>
```

`assemble` runs no host tools itself; the harness collects `uname`/`git`/`rustc`/`qemu` once and
passes them in, so assembly is a pure function of its inputs.

`extract_trace(uart, manifest, profile)`:

- Strips an optional `[ts=<u64>ms] ` prefix into `ts_ms_from_boot`; a malformed prefix is
  `MalformedTrace` (`malformed_ts_*`), never silent loss.
- For each line, every manifest literal contained as a substring yields one entry
  (length-descending, deduplicated per line), so log-level and service prefixes need no parsing.
- A line whose body starts with `SELFTEST:` or `dsoftbusd:` must match a declared literal;
  otherwise `MalformedTrace { unknown_marker }`. Other unmatched lines are noise. `dbg:` lines
  are never in the manifest and never in the trace.

## 5. Signature

`signature.bin` is 102 bytes, big-endian:

| Offset | Len | Field | Value |
|---|---|---|---|
| 0 | 4 | magic | `NXSE` |
| 4 | 1 | version | `0x01` |
| 5 | 1 | label | `0x01` Ci, `0x02` Bringup |
| 6 | 32 | hash | `H_root` |
| 38 | 64 | sig | Ed25519 over `hash` |

The signature does not feed back into the hash; resealing with the same key gives the same bytes.
An unknown version is `UnsupportedSignatureVersion`; bad magic, length or label is
`SignatureMalformed`. `verify --policy=ci|bringup|any` enforces the key class
(`KeyLabelMismatch` otherwise).

```text
nexus-evidence seal   <bundle.tar.gz> --privkey=<path> --label=ci|bringup
nexus-evidence verify <bundle.tar.gz> --pubkey=<path> [--policy=ci|bringup|any]
nexus-evidence keygen --seed=<hex32> --pubkey-out=<path> [--privkey-out=<path>]
```

`tools/seal-evidence.sh` and `tools/verify-evidence.sh` resolve the key for the label/policy and
refuse to run without it.

Tamper classes (`source/libs/nexus-evidence/tests/sign_verify.rs`), each `SignatureMismatch`:
A flip a byte of `manifest.tar`; B append to `uart.log`; C add a forged `trace.jsonl` entry;
D change a hashed `config.json` field; E swap `signature.bin` between two bundles. Also: wrong
public key → `SignatureMismatch`; bringup bundle under `--policy=ci` → `KeyLabelMismatch`.

| Exit | Meaning |
|---|---|
| 0 | sealed / verified, policy satisfied |
| 1 | schema or signature failure (`SignatureMismatch`, `KeyLabelMismatch`, `SignatureMalformed`, `UnsupportedSignatureVersion`, `SignatureMissing`, `MalformedTrace`, …) |
| 2 | missing key material or input file |

## 6. Keys

Resolved by `nexus_evidence::key::from_env_or_dir`, in this order:

| Class | Private key | Public key |
|---|---|---|
| CI (`0x01`) | `NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64` (32-byte seed, env only, from the CI secret store) | `keys/evidence-ci.pub.ed25519` (committed) |
| Bringup (`0x02`) | `$NEXUS_EVIDENCE_BRINGUP_PRIVKEY`, default `~/.config/nexus/bringup-key/private.ed25519` (64 hex chars, mode `0600` or `KeyMaterialPermissions`) | `~/.config/nexus/bringup-key/public.ed25519` |

Neither present → `KeyMaterialMissing`. Rotation: `tools/gen-ci-key.sh` (see `keys/README.md`),
`tools/gen-bringup-key.sh` for a developer key. `verify --policy=ci` is the only check that CI-grade
evidence was not sealed on a laptop.

## 7. Secret scan

`Bundle::seal` scans before signing; the first hit aborts with `SecretLeak { artifact, line, kind }`.
Scanned: `uart.log` lines, each trace `marker`, and `config.json` `kernel_cmdline`, `qemu_args[i]`,
`host_info` and every `env` pair as `KEY=VALUE`.

| `LeakKind` | Label | Trigger |
|---|---|---|
| `PemPrivateKey` | `pem_private_key` | `BEGIN <RSA\|EC\|OPENSSH\|PGP\|DSA> PRIVATE KEY` or `BEGIN PRIVATE KEY` |
| `BringupKeyPath` | `bringup_key_path` | `bringup-key/private` |
| `PrivateKeyEnvAssignment` | `private_key_env_assignment` | case-insensitive `…PRIVATE_KEY…=` followed by ≥40 base64 chars |
| `HighEntropyBlob` | `high_entropy_blob` | ≥64 contiguous base64 chars on one line |

`source/libs/nexus-evidence/scan.toml` (`[allowlist] substrings = [...]`) can suppress only
`HighEntropyBlob`; each entry needs review and a one-line justification. Fixture tests use
`Bundle::seal_with(key, label, &allowlist)`; production always calls `Bundle::seal` with an empty
allowlist.

## 8. Errors

| `EvidenceError` | Trigger | Exit |
|---|---|---|
| `MissingArtifact` | a required field empty at assembly | 1 |
| `MalformedTrace` | bad `[ts=` prefix or unknown assertion-class marker | 1 |
| `MalformedConfig` | a config field violates the schema | 1 |
| `CanonicalizationFailed` | internal serialization error | 2 |
| `SchemaVersionUnsupported` | `schema_version` not accepted | 1 |
| `SignatureMissing`, `SignatureMalformed`, `SignatureMismatch`, `KeyLabelMismatch`, `UnsupportedSignatureVersion` | §5 | 1 |
| `SecretLeak` | §7 | 1 |
| `KeyMaterialMissing`, `KeyMaterialPermissions` | §6 | 2 |

Variants are append-only.

## 9. Harness gates

`scripts/qemu-test.sh` (after `verify-uart`) and `tools/os2vm.sh` assemble a bundle and seal it
when required. The label is `ci` when `NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64` is set, else `bringup`.
An assemble failure exits 1 in both. A seal failure exits 1 in `tools/os2vm.sh`
(`OS2VM_E_EVIDENCE_NODE_A/B`) but is only a warning in `scripts/qemu-test.sh`.

| Env | Effect |
|---|---|
| `NEXUS_EVIDENCE_SEAL=1` | seal even outside CI (otherwise the bundle stays unsigned) |
| `CI=1` | implies `NEXUS_EVIDENCE_SEAL=1` |
| `NEXUS_EVIDENCE_DISABLE=1` | skip the post-pass; rejected with exit 1 when sealing is required: `NEXUS_EVIDENCE_DISABLE=1 is rejected when CI=1 (or NEXUS_EVIDENCE_SEAL=1) — refusing to drop the audit trail` |
| `NEXUS_EVIDENCE_BIN=<path>` | override the `nexus-evidence` binary |

`source/libs/nexus-evidence/tests/qemu_seal_gate.rs` locks the disable/seal decision.
