<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Evidence Bundle — normative spec

- Status: Phase-5 of [TASK-0023B](../../tasks/TASK-0023B-selftest-client-production-grade-deterministic-test-architecture-refactor.md), Cut **P5-01** (skeleton: schema + canonical hash only).
- Owners: @runtime
- Anchor RFC: [RFC-0038](../rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md) §"Phase 5 — Signed evidence bundles" (lines 416-465).
- Crate: [`source/libs/nexus-evidence/`](../../source/libs/nexus-evidence/) (host-only).
- Companion: [Proof Manifest schema](proof-manifest.md) (the manifest a bundle seals against).

## Why this file exists

A green QEMU run currently leaves only a UART log on the runner. Phase 5 promotes every successful `just test-os PROFILE=…` invocation into a portable, attestable artifact:

> "Build `<git-sha>` under profile `<P>` against manifest `<H>` produced ladder `<T>` and was attested by key `<K>`."

This document is the **normative schema** for that artifact. The `nexus-evidence` crate is the implementation; the bundles it produces and verifies must obey the contract described here.

Cut layout is incremental:

| Cut | Surface added |
| --- | --- |
| **P5-01** | Schema definitions, canonical hash. No assembly, no signing. |
| P5-02 | `Bundle::assemble`, `trace.jsonl` extractor, `config.json` builder, `nexus-evidence` CLI (`assemble`, `inspect`, `canonical-hash`). |
| P5-03 | Ed25519 signing/verification, signature byte format, `tools/{seal,verify}-evidence.sh`, 5 tamper classes. |
| P5-04 | CI/bringup key separation, secret scanner. |
| P5-05 | `qemu-test.sh` post-pass seal integration, CI gates. |

This document grows section-by-section across cuts. Sections explicitly tagged "(P5-0X)" are introduced by that cut; earlier cuts are normative for their portion.

## 1. Bundle layout (P5-01 normative)

A sealed bundle is a single tarball:

```
target/evidence/<utc>-<profile>-<git-sha>.tar.gz
```

Components (all 5 required from P5-03 onward; `signature.bin` absent during P5-01/P5-02):

| File             | Purpose                                                                  | Hashed? |
| ---------------- | ------------------------------------------------------------------------ | ------- |
| `manifest.tar`   | Verbatim copy of the `proof-manifest` tree used for the run (v1 single file or v2 split tree, packed in a nested tar to preserve directory structure). | Yes (`H(manifest_bytes)`) |
| `uart.log`       | Unfiltered serial output from the run.                                   | Yes (`H(uart_normalized)`) |
| `trace.jsonl`    | Extracted marker ladder; one JSON object per line.                       | Yes (`H(sorted_trace)`)    |
| `config.json`    | Profile, env, kernel cmdline, qemu args, host info, build SHA, rustc/qemu versions, wall-clock UTC. | Yes (`H(sorted_config)`), with the **`wall_clock_utc` field excluded** |
| `signature.bin`  | Ed25519 signature over the canonical hash. (P5-03+.)                     | No (it IS the signature) |

P5-02 will define the assembly flow and freeze the on-tar layout (including deterministic file order and `mtime=0` for reproducibility).

## 2. Schema (P5-01 normative)

The Rust types in [`source/libs/nexus-evidence/src/lib.rs`](../../source/libs/nexus-evidence/src/lib.rs) are the canonical schema source; the table below mirrors them.

### 2.1 `BundleMeta`

| Field            | Type     | Notes                                                            |
| ---------------- | -------- | ---------------------------------------------------------------- |
| `schema_version` | `u8`     | Bundle schema version. **`1` for Phase-5.** Append-only enum.    |
| `profile`        | `String` | Profile name as accepted by `nexus-proof-manifest` (e.g. `full`). |

### 2.2 `ManifestArtifact`

| Field   | Type       | Notes                                                                       |
| ------- | ---------- | --------------------------------------------------------------------------- |
| `bytes` | `Vec<u8>`  | Verbatim bytes of the manifest as packed for the bundle. Hashed as-is.      |

For a v1 manifest this is the single TOML file's bytes. For a v2 manifest (P5-00 onward) this is a deterministic tar of the split tree (file order = lexicographic; file `mtime=0`); the inner tar bytes are what gets hashed. P5-02 finalizes the inner-tar layout.

### 2.3 `UartArtifact`

| Field   | Type      | Notes                                                                                |
| ------- | --------- | ------------------------------------------------------------------------------------ |
| `bytes` | `Vec<u8>` | Raw serial output. Line endings are normalized (`\r\n` → `\n`) at hash-time only. |

### 2.4 `TraceArtifact` / `TraceEntry`

`trace.jsonl` is one `TraceEntry` per line, JSON-encoded:

| Field             | Type             | Notes                                                            |
| ----------------- | ---------------- | ---------------------------------------------------------------- |
| `marker`          | `String`         | The exact UART marker literal (e.g. `"SELFTEST: vfs ok"`).        |
| `phase`           | `String`         | The manifest phase the marker belongs to (looked up via the manifest, NOT inferred from position). |
| `ts_ms_from_boot` | `Option<u64>`    | Extracted from a `[ts=…ms]` prefix when present in the UART line; `None` otherwise (the harness does not yet emit timestamps in all paths). |
| `profile`         | `String`         | Profile this entry was recorded under. Constant per bundle but stored per-entry for future merged-bundle replay. |

P5-02 will define the extractor (`extract_trace`) and lock the regex.

### 2.5 `ConfigArtifact`

| Field             | Type                       | Hashed? | Notes                                                                       |
| ----------------- | -------------------------- | ------- | --------------------------------------------------------------------------- |
| `profile`         | `String`                   | Yes     | Echoes `BundleMeta.profile`; locked together for cross-check.              |
| `env`             | `BTreeMap<String, String>` | Yes     | Resolved profile env (output of `nexus-proof-manifest list-env --profile=<p>`). `BTreeMap` enforces key sort order. |
| `kernel_cmdline`  | `String`                   | Yes     | Whatever the harness passed via `-append`.                                  |
| `qemu_args`       | `Vec<String>`              | Yes     | Captured from `qemu-test.sh`. Argv order is preserved (significant).        |
| `host_info`       | `String`                   | Yes     | Output of `uname -a` (single line).                                         |
| `build_sha`       | `String`                   | Yes     | `git rev-parse HEAD` at run time.                                           |
| `rustc_version`   | `String`                   | Yes     | `rustc --version`.                                                          |
| `qemu_version`    | `String`                   | Yes     | `qemu-system-riscv64 --version` (first line).                               |
| `wall_clock_utc`  | `String`                   | **No**  | RFC-3339 timestamp of seal time. Reproducibility carve-out: two reseals on the same host produce byte-identical bundles modulo this field. |

### 2.6 `Signature` (placeholder until P5-03)

P5-01 declares the `Signature` field on `Bundle` as `Option<Signature>` and ships an empty placeholder type so downstream code can pattern on `bundle.signature.is_some()`. The signing payload, byte format, key labels, and verification semantics land in P5-03.

## 3. Canonical hash (P5-01 normative)

The canonical hash binds the 5 artifacts (4 hashed, 1 excluded) into a single 32-byte digest that the signature is computed over.

### 3.1 Formula

Let `H(x) = SHA-256(x)`. The canonical bundle hash is:

```
H_root = SHA-256(
    H(meta_canonical)         ||  // 32 bytes
    H(manifest_bytes)         ||  // 32 bytes
    H(uart_normalized)        ||  // 32 bytes
    H(trace_canonical)        ||  // 32 bytes
    H(config_canonical)           // 32 bytes
)
```

The intermediate `H(...)` blocks are concatenated in the **fixed order** above (meta, manifest, uart, trace, config) and hashed once more to produce the 32-byte root. Order is normative; reordering fields is a schema break.

This extends RFC-0038 line 435 (which under-specifies UART coverage) so that any byte-level tampering of any of the 4 hashed artifacts produces `SignatureMismatch` at verify time. The extension is intentional and is the basis for P5-03 tamper class B (modify `uart.log` → mismatch).

### 3.2 Canonicalization rules per term

#### `meta_canonical`

Encoded as the UTF-8 bytes of:

```
schema_version=<u8>\nprofile=<profile>\n
```

Single trailing `\n`, no surrounding whitespace, no quoting.

#### `manifest_bytes`

The exact `manifest_artifact.bytes` field, hashed as-is. No normalization (the manifest is the source of truth for itself).

#### `uart_normalized`

`uart_artifact.bytes` with `\r\n` → `\n` normalization applied. No other transformation. Trailing newline (if any) is preserved.

#### `trace_canonical`

The trace entries are sorted by the tuple `(marker, phase)` (both as raw UTF-8 byte sequences, lexicographic order) before serialization. This makes the hash invariant under harness re-ordering or merged-bundle replay.

Each sorted entry is serialized as a single JSON line in the form:

```
{"marker":"<escaped>","phase":"<escaped>","ts_ms_from_boot":<u64-or-null>,"profile":"<escaped>"}\n
```

Field order is fixed (`marker`, `phase`, `ts_ms_from_boot`, `profile`); `ts_ms_from_boot` is the literal `null` token when `None`, otherwise a bare integer. JSON string escaping follows `serde_json`'s default rules (backslash, quote, control characters). One `\n` separates entries; no trailing `\n` after the last entry. The whole concatenation is the hash input.

#### `config_canonical`

Serialized as a single JSON object with keys in the **fixed order**:

```
profile, env, kernel_cmdline, qemu_args, host_info, build_sha, rustc_version, qemu_version
```

`wall_clock_utc` is **omitted** entirely. `env` is an object whose keys are sorted lexicographically (the `BTreeMap` provides this). `qemu_args` keeps argv order. The whole object is emitted as **compact** JSON (no whitespace, no trailing newline) — i.e. `{"profile":"full","env":{...},...}`.

### 3.3 Determinism guarantees (P5-01 invariants)

- **Reorder-invariance**: re-shuffling `trace.entries` in memory does not change `H_root`.
- **Env-key-order-invariance**: the env map's iteration order does not change `H_root` (sorted keys + `BTreeMap`).
- **Line-ending-invariance**: `\r\n`/`\n` differences in `uart.bytes` do not change `H_root`.
- **Wall-clock-invariance**: reseal of the same run on the same host produces the same `H_root` even though `wall_clock_utc` differs.
- **Manifest-sensitivity**: any byte change in `manifest_artifact.bytes` changes `H_root`.

Each invariant is locked by a dedicated test in [`tests/canonical_hash.rs`](../../source/libs/nexus-evidence/tests/canonical_hash.rs).

## 3a. Assembly (P5-02 normative)

Bundle assembly turns three on-disk inputs (UART transcript, manifest tree, host-introspection results) into a fully-populated [`Bundle`] in memory. Writing it to disk is a separate step; sealing is yet another (P5-03).

### CLI

```text
nexus-evidence assemble \
  --uart=<uart.log> \
  --manifest=<proof-manifest/manifest.toml> \
  --profile=<name> \
  --out=target/evidence/<utc>-<profile>-<git-sha>.tar.gz \
  [--kernel-cmdline=<str>] [--qemu-arg=<a>]... \
  [--host-info=<str>] [--build-sha=<sha>] \
  [--rustc-version=<str>] [--qemu-version=<str>] \
  [--wall-clock=<rfc3339>] [--env=<KEY=VALUE>]...

nexus-evidence inspect        <bundle.tar.gz>
nexus-evidence canonical-hash <bundle.tar.gz>
```

The `assemble` subcommand does **not** shell out to `uname` / `git` / `rustc` / `qemu-system-riscv64` itself — the harness wrapper at P5-05 collects those once per run and forwards them via `--host-info=…` / `--build-sha=…` / `--rustc-version=…` / `--qemu-version=…`. Tests and direct invocations supply them inline. This keeps `Bundle::assemble` a pure transformation, which is what makes the reproducibility guarantee in §3.3 a property of the library, not of the runtime environment.

### On-tar layout (P5-02 freezes this)

The outer `<utc>-<profile>-<git-sha>.tar.gz` contains entries in **lexicographically sorted** order:

| Entry           | Content                                                                  |
| --------------- | ------------------------------------------------------------------------ |
| `config.json`   | Full [`ConfigArtifact`] (incl. `wall_clock_utc`).                        |
| `manifest.tar`  | Inner tar of the proof-manifest tree (paths relative to the manifest root's parent). For v1 manifests this is one file (`proof-manifest.toml`); for v2 it is the full split layout (`manifest.toml`, `phases.toml`, `markers/<phase>.toml`, `profiles/<set>.toml`). |
| `meta.json`     | `{"schema_version": 1, "profile": "<name>"}`.                            |
| `signature.bin` | Present only after P5-03 seal.                                           |
| `trace.jsonl`   | One [`TraceEntry`] per line, in **UART appearance order** (the canonical hash sorts internally). |
| `uart.log`      | Raw bytes (line endings preserved on disk; normalized at hash-time).     |

Note: the spec freezes a 6-file layout (5 mandatory + 1 conditional `signature.bin`) — adding `meta.json` is the deliberate concrete realization of [`BundleMeta`] on disk so verifiers can read the bundle schema version before parsing anything else.

### Reproducibility rules

Every tar header (outer and inner) is normalized:

- `mtime = 0`
- `uid = 0`, `gid = 0`
- `mode = 0o644`
- `uname = ""`, `gname = ""`
- `entry_type = Regular`

The gzip wrapper sets `mtime = 0` in its header (`flate2::GzBuilder::mtime(0)`).

Together these guarantees mean: two assemblies of the same UART + same manifest + same gather-opts on the same host produce **byte-identical** outer tar.gz bytes. Two reseals on the **same** run with **different** `wall_clock_utc` produce a different `config.json` (and therefore different outer-tar bytes) but the same canonical hash (`wall_clock_utc` is excluded from the hash by §3.2).

### Trace extractor rules

`extract_trace(uart, manifest, profile)` walks the UART line-by-line, mirroring the substring-match strategy of the P4-09 `verify-uart` analyzer. Behavior:

- Optional `[ts=<u64>ms] ` prefix: stripped and parsed into `ts_ms_from_boot`. A malformed timestamp prefix (`[ts=fooms] …`, `[ts=42` without close, etc.) returns [`EvidenceError::MalformedTrace`] with a `malformed_ts_*` diagnostic — never `None`-with-silent-data-loss.
- For every non-empty line body, scan the manifest's **full** marker-literal universe and emit one [`TraceEntry`] per literal that is contained as a substring (length-descending order; per-line dedup so overlapping prefixes do not double-count). Real UART lines have log-level prefixes (`[INFO selftest] KSELFTEST: foo bar ok`) and span ~25 distinct service prefixes (`samgrd: ready`, `policyd: ready`, `vfsd: ready`, `init: start`, `dsoftbusd: …`, …); substring matching covers them all uniformly.
- `phase` on each [`TraceEntry`] is taken from the manifest's `[marker."<literal>"]` declaration — **never** from UART position. A vfs marker emitted before a bringup marker still carries `phase = "vfs"`.
- **Deny-by-default for assertion-class prefixes**: a line whose body starts with `SELFTEST:` or `dsoftbusd:` MUST resolve to at least one manifest literal. An orphan `SELFTEST: foo` printout that wasn't registered in the manifest is a hard error (`EvidenceError::MalformedTrace { unknown_marker }`). Other prefixes (kernel banners, init progress, harness logs) are benign noise: silently skipped if they don't match any literal.
- `dbg:` lines are by spec excluded from the manifest universe (rule `12-debug-discipline`) and therefore never appear in the trace.

Output order is UART appearance order (preserved for human inspection / diffing); the canonical hash sorts internally.

## 3b. Signing & verification (P5-03 normative)

P5-03 introduces the seal pipeline: an existing unsigned bundle is read, the canonical hash is computed, an Ed25519 signature is produced over that hash, and a `signature.bin` blob is added to the tarball in-place. The bundle's canonical hash is unchanged (the signature does not feed back into the hash), so seal/verify is order-stable: re-sealing produces a byte-identical signature for the same hash + same key.

### Signature byte format

`signature.bin` is **102 bytes**, big-endian byte order throughout:

```text
offset  length  field    value
------  ------  -------  -------------------------------------------
0       4       magic    b"NXSE" (0x4E, 0x58, 0x53, 0x45)
4       1       version  0x01
5       1       label    0x01 = Ci  |  0x02 = Bringup
6       32      hash     canonical_hash(bundle) (see §3.1)
38      64      sig      Ed25519(privkey, hash)
```

Bumping `version` is a hard break: verifiers built before the bump MUST refuse the new bundle ([`EvidenceError::UnsupportedSignatureVersion`]) rather than silently accept a possibly-incompatible payload. Any other structural failure (bad magic, bad length, unknown label byte) returns [`EvidenceError::SignatureMalformed`] with a stable diagnostic.

### Key labels

The two label bytes encode key class. Verifiers can enforce a class via `--policy=ci|bringup`; a mismatch returns [`EvidenceError::KeyLabelMismatch`] with the expected and observed labels.

| Label byte | `KeyLabel` variant | Class       | Private key source                                              | Public key source                                       |
| ---------- | ------------------ | ----------- | --------------------------------------------------------------- | ------------------------------------------------------- |
| `0x01`     | `Ci`               | CI runner   | `$NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64` (env, never persisted)  | `keys/evidence-ci.pub.ed25519` (committed)              |
| `0x02`     | `Bringup`          | Local dev   | `~/.config/nexus/bringup-key/private.ed25519` (chmod 0600)      | `~/.config/nexus/bringup-key/public.ed25519`            |

P5-03 ships a placeholder CI pubkey under `keys/evidence-ci.pub.ed25519` (deterministically derived from a known seed; matching private key is **not** confidential). P5-04 regenerates this file with system entropy via `tools/gen-ci-key.sh`; until then any "CI" verification is bring-up only.

### Tamper classes (P5-03 lock)

The 5 tamper classes that `tests/sign_verify.rs` enforces. Each mutates exactly one artifact and proves the verifier rejects the mutation:

| Class | Mutation                                            | Expected error                              |
| ----- | --------------------------------------------------- | ------------------------------------------- |
| A     | Flip a byte in `manifest.tar` after sealing         | [`EvidenceError::SignatureMismatch`]        |
| B     | Append a line to `uart.log` after sealing           | [`EvidenceError::SignatureMismatch`]        |
| C     | Add a forged entry to `trace.jsonl` after sealing   | [`EvidenceError::SignatureMismatch`]        |
| D     | Modify a hashed `config.json` field (NOT wall_clock) | [`EvidenceError::SignatureMismatch`]        |
| E     | Swap `signature.bin` between two valid bundles      | [`EvidenceError::SignatureMismatch`]        |

Plus: cross-key rejection (signed by key B, verified by pubkey A → `SignatureMismatch`); policy mismatch (bringup-signed bundle, `--policy=ci` → `KeyLabelMismatch`); unsupported version (`version != 0x01` → `UnsupportedSignatureVersion`); bad magic (`magic != "NXSE"` → `SignatureMalformed`).

### CLI surface

```text
nexus-evidence seal   <bundle.tar.gz> --privkey=<path> --label=ci|bringup
nexus-evidence verify <bundle.tar.gz> --pubkey=<path> [--policy=ci|bringup|any]
nexus-evidence keygen --seed=<hex32bytes> --pubkey-out=<path> [--privkey-out=<path>]
```

`tools/seal-evidence.sh` and `tools/verify-evidence.sh` are thin shell wrappers: they resolve the right key source for the requested label/policy and invoke the CLI. Both refuse to run if the resolved key source is missing — callers always get a clean exit code rather than a half-sealed bundle. P5-04 will land `tools/gen-{ci,bringup}-key.sh` to generate the keypairs themselves; until then `nexus-evidence keygen --seed=…` is the manual fallback.

### Exit codes

| Code | Meaning                                                                  |
| ---- | ------------------------------------------------------------------------ |
| 0    | Sealed / verified ok and policy satisfied                                |
| 1    | Schema / signature failure — `SignatureMismatch`, `KeyLabelMismatch`, `SignatureMalformed`, `UnsupportedSignatureVersion`, `SignatureMissing` |
| 2    | Missing key material or input file (key source unset, bundle not found)  |

## 4. Error class table (P5-01 surface)

| `EvidenceError` variant       | Trigger                                                                       | Stable exit code (P5-03+) |
| ----------------------------- | ----------------------------------------------------------------------------- | ------------------------- |
| `MissingArtifact`             | A required field on `Bundle` is empty / unset when assembly demands it.       | 1                         |
| `MalformedTrace`              | A `TraceEntry` violates schema invariants (P5-02 will refine to per-line cause). | 1                         |
| `MalformedConfig`             | A `ConfigArtifact` field violates schema invariants (P5-02 will refine).      | 1                         |
| `CanonicalizationFailed`      | Internal serialization error in canonicalization (should never fire in P5-01; reserved). | 2                         |
| `SchemaVersionUnsupported`    | `BundleMeta.schema_version` is not in the parser's accepted set.              | 1                         |

The signature/seal-related variants (`SignatureMissing`, `SignatureMalformed`, `SignatureMismatch`, `KeyLabelMismatch`, `UnsupportedSignatureVersion`) are added in P5-03; the secret-scanner variants (`SecretLeak`, `KeyMaterialMissing`, `KeyMaterialPermissions`) in P5-04. Variants are append-only; no rename or removal across cuts.

## 3c. Key separation (P5-04)

The signing pipeline distinguishes two key classes; they are
resolved by [`nexus_evidence::key::from_env_or_dir`] in this strict
priority order:

1. **CI key** — env var `NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64`.
   The base64-decoded payload is a 32-byte Ed25519 seed. The CI
   runner injects this from its secret store; it never lands on
   disk. `tools/gen-ci-key.sh` is the documented ceremony for
   rotating it (see `keys/README.md`).

2. **Bringup key** — file at `$NEXUS_EVIDENCE_BRINGUP_PRIVKEY` (or
   the default `~/.config/nexus/bringup-key/private.ed25519`). The
   file format is the same as `nexus-evidence keygen --privkey-out`
   produces (64 hex chars + newline). The file **must** be mode
   `0600`; otherwise the resolver returns
   [`EvidenceError::KeyMaterialPermissions`] with the offending
   mode. `tools/gen-bringup-key.sh` is the documented helper.

3. **Neither** → [`EvidenceError::KeyMaterialMissing`] with a
   diagnostic that names both expected sources.

The resolved [`KeyLabel`] is encoded into the signature byte stream
(§3b); downstream `verify --policy=ci` rejects bringup-signed
bundles, and vice versa. This is the only mechanism enforcing
"CI-grade evidence didn't get sealed by a developer's laptop".

## 3d. Secret scanner (P5-04)

`Bundle::seal` runs a deny-by-default scanner before signing. Any
match aborts the seal with [`EvidenceError::SecretLeak`]; nothing
silent. Scanned artifacts:

- `uart.log` (line by line, lossy UTF-8)
- `trace.jsonl` (each entry's `marker`)
- `config.json`: `kernel_cmdline`, every `qemu_args[i]`, `host_info`,
  every `env` key/value pair joined as `KEY=VALUE`.

Patterns (`LeakKind`):

| Kind                        | Stable label                  | Trigger                                                                                                |
| --------------------------- | ----------------------------- | ------------------------------------------------------------------------------------------------------ |
| `PemPrivateKey`             | `pem_private_key`             | Substring `BEGIN <RSA\|EC\|OPENSSH\|PGP\|DSA> PRIVATE KEY` or bare `BEGIN PRIVATE KEY`.                |
| `BringupKeyPath`            | `bringup_key_path`            | Substring `bringup-key/private` (covers the default bringup key path and any reasonable variation).    |
| `PrivateKeyEnvAssignment`   | `private_key_env_assignment`  | Case-insensitive `…PRIVATE_KEY…=` followed by ≥40 contiguous base64-alphabet chars in the tail.        |
| `HighEntropyBlob`           | `high_entropy_blob`           | ≥64 contiguous base64-alphabet chars on a single line, not suppressed by the allowlist.                |

The first hit wins; the resulting `EvidenceError::SecretLeak`
carries the artifact name (`uart.log` / `trace.jsonl` / `config.json`),
a 1-indexed line number, and the stable label above.

### Allowlist

`source/libs/nexus-evidence/scan.toml` carries an allowlist for
the high-entropy heuristic only. PEM blocks, bringup-key paths, and
`PRIVATE_KEY=…` env assignments are unconditional rejects and
cannot be allowlisted. Schema:

```toml
[allowlist]
substrings = [
  "known-benign-fragment-1",
  "known-benign-fragment-2",
]
```

P5-04 ships this file empty; entries land case-by-case as
false-positives surface against fresh QEMU runs (each entry should
be PR-reviewed and accompanied by a one-line justification in the
PR body).

### Test-only escape hatch

`Bundle::seal_with(key, label, &allowlist)` lets fixture tests
suppress the high-entropy heuristic on synthetic UART. Production
callers always use `Bundle::seal`, which calls
`scan_for_secrets_with(self, &ScanAllowlist::empty())`.

## 5. Operational gates (P5-05)

`scripts/qemu-test.sh` and `tools/os2vm.sh` both run a post-pass
that assembles + (conditionally) seals an evidence bundle after the
manifest-driven `verify-uart` succeeds. Bundles land at:

```
target/evidence/<utc>-<profile>-<gitsha>.tar.gz          # qemu-test.sh
target/evidence/<utc>-<profile>-<gitsha>-<a|b>.tar.gz    # os2vm.sh (per node)
```

Both paths are gitignored (see `.gitignore`).

### Env knobs

| Env                          | Effect                                                                                                                                                  |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `NEXUS_EVIDENCE_SEAL=1`      | Force the seal step even outside CI. Failure to seal → exit 1.                                                                                          |
| `CI=1`                       | Implies `NEXUS_EVIDENCE_SEAL=1`. Also rejects `NEXUS_EVIDENCE_DISABLE=1`.                                                                               |
| `NEXUS_EVIDENCE_DISABLE=1`   | Skip the post-pass entirely (assemble + seal). **Rejected with exit 1 when `CI=1` or `NEXUS_EVIDENCE_SEAL=1`** so the audit trail cannot be silently dropped. |
| `NEXUS_EVIDENCE_BIN=<path>`  | Override the `nexus-evidence` CLI lookup; mirrors the same knob on the seal/verify wrappers.                                                            |

### Label resolution

The post-pass calls `tools/seal-evidence.sh <bundle> --label=<resolved>`
where the label is:

- `ci` if `NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64` is set in the
  environment (CI runner injected the secret);
- `bringup` otherwise.

This means a developer running `just test-os` with `NEXUS_EVIDENCE_SEAL=1`
on their laptop will sign with the bringup key (resolved per
§3c); CI runs sign with the CI key.

### CI hard gates

Two failure modes are mandatory exit-1:

1. A successful run whose `nexus-evidence assemble` or
   `tools/seal-evidence.sh` fails. The script does **not** soften
   into a warning.
2. `NEXUS_EVIDENCE_DISABLE=1` set when seal is required. The
   diagnostic is stable: `NEXUS_EVIDENCE_DISABLE=1 is rejected when
   CI=1 (or NEXUS_EVIDENCE_SEAL=1) — refusing to drop the audit
   trail`.

## 6. References

- RFC-0038 §"Phase 5 — Signed evidence bundles" — contract source (lines 416-465).
- RFC-0014 §3 v2 — phase ordering (manifest is normative for the phase set).
- ADR-0027 — selftest-client two-axis architecture (why this crate is host-only).
- [docs/testing/proof-manifest.md](proof-manifest.md) — the manifest schema this bundle hashes.
- [docs/testing/index.md](index.md) — testing methodology and QEMU markers.
