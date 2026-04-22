# RFC-0039: Supply-Chain v1 — bundle SBOM (CycloneDX) + repro metadata + signature allowlist policy (host-first, OS-gated)

- Status: Done
- Owners: @runtime @security
- Created: 2026-04-21
- Last Updated: 2026-04-22 (status flip to Done; proof checklist remained green)
- Links:
  - Tasks: `tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md` (execution + proof)
  - ADR (manifest format = capnp): `docs/adr/0020-manifest-format-capnproto.md`
  - ADR (canonical-vs-interop format policy): `docs/adr/0021-structured-data-formats-json-vs-capnp.md`
  - Related RFCs: `docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md` (manifest.nxb baseline + bundle pipeline)
  - Related RFCs: `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md` (policy authority + audit contract)
  - Related RFCs: `docs/rfcs/RFC-0016-device-identity-keys-v1.md` (keystored / device keys baseline)
  - Related RFCs: `docs/rfcs/RFC-0011-logd-journal-crash-v1.md` (audit sink contract)
  - Related RFCs: `docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md` (proof-manifest layout + evidence-bundle determinism reused here)
  - Production-gate tier: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Updates/Packaging/Recovery → `production-grade`)
  - Future RFC seeds (do not own scope here): supply-chain v2 host (`TASK-0197`), v2 OS enforcement (`TASK-0198`), boot-trust closure (`TASK-0289`)

## Status at a Glance

- **Phase 0 (SBOM generator + bundle embedding)**: ✅
- **Phase 1 (Repro metadata + `repro-verify`)**: ✅
- **Phase 2 (Allowlist authority — `keystored` capnp + `policyd` decision + `bundlemgrd` enforcement)**: ✅
- **Phase 3 (Host reject-path proofs)**: ✅
- **Phase 4 (OS selftest + deterministic markers, gated)**: ✅

Definition:

- "Complete" means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean "never changes again".

### Current closure deltas (resolved for RFC closure)

- RFC status closure sync is complete (`In Review` → `Done`).
- Execution task `TASK-0029` intentionally remains `In Review` until separate task-level finalization.

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in `TASK-0029`.

- **This RFC owns**:
  - The on-bundle layout for supply-chain artefacts (`meta/sbom.json`, `meta/repro.env.json`) and their content-hash-addressable references from `manifest.nxb`.
  - The **single-authority** publisher/key allowlist contract (`keystored::IsKeyAllowed` capnp method) with explicit schema-extensibility ratchet.
  - The install-time enforcement order in `bundlemgrd` (verify → policy → digest → audit-or-fail).
  - The deterministic-marker contract for `bundlemgrd` and selftest-client supply-chain probes.
  - The "production-grade BASELINE" boundary for the supply-chain leg of the Updates/Packaging/Recovery group, and the explicit hard gates that prevent v1 from pre-empting v2/v3 contracts.

- **This RFC does NOT own**:
  - The capnp **sigchain envelope** binding `(manifest, sbom, repro, signature)` into one cryptographically verifiable contract → owned by the future supply-chain v2 host RFC (seed: `TASK-0197`).
  - Any **transparency / Merkle translog** of install/update decisions → owned by the v2 host RFC (`TASK-0197`).
  - **SLSA-style provenance attestations** (in-toto links, builder identity envelopes) → owned by the v2 host RFC (`TASK-0197`).
  - **Anti-downgrade** semantics, rollback indices, monotonic version counters → owned by the v2 OS RFC (`TASK-0198`) and the boot-trust RFC (seed: `TASK-0289`).
  - **`updated` / `storemgrd` install/apply path** changes → owned by the v2 OS RFC (`TASK-0198`).
  - **Boot-anchor / measured-boot** work → owned by the boot-trust RFC (`TASK-0289`).
  - System-set (`.nxs`) SBOM aggregation — same v2 boundary.
  - Kernel changes — out of scope by design.

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define **stop conditions** and **proof commands**.
- This RFC links to `TASK-0029`, which implements and proves each phase.

## Context

The repo already has the building blocks for a real supply-chain v1, but they are wired in isolation:

- `tools/nxb-pack` writes the canonical `manifest.nxb` (capnp, per ADR-0020 + `TASK-0007` `Done`).
- `bundlemgrd` install path already calls `keystored::verify` for raw Ed25519 signatures.
- `keystored` (RFC-0016, `TASK-0008B` `Done`) owns device keys and the verify primitive but exposes only `getAnchors / verify / deviceId` — **no allowlist-check method**.
- `policyd` (RFC-0015, `TASK-0008` `Done`) is the single decision authority but has no install-time supply-chain hook.
- `logd` (RFC-0011, `TASK-0006` `Done`) provides the audit sink.
- `nexus-evidence` (`TASK-0023B` Phase 5) provides reproducible-`tar.gz` packing and a deny-by-default secret scanner — both reusable for SBOM/repro embedding.
- ADR-0021 already designates **CycloneDX JSON** as the canonical SBOM interop format (and `manifest.nxb` capnp as the canonical signed contract — clean canonical-vs-interop split).

What is missing today:

- No SBOM is generated or embedded in any bundle.
- No reproducibility metadata is captured; `repro-verify` does not exist.
- There is no publisher/key allowlist; any device-key-signed bundle that passes `keystored::verify` would install.
- No deterministic markers prove allow/deny supply-chain decisions in QEMU.

The result is that bundle install is *technically* signed but not *policy-bounded*: the signature primitive is correct, the policy layer above it is empty.

## Goals

- Define the on-bundle supply-chain artefact layout (`meta/sbom.json`, `meta/repro.env.json`) with content-hash-addressable references from `manifest.nxb`.
- Define the single-authority publisher/key allowlist as an explicit `keystored` capnp method.
- Define the install-time enforcement contract in `bundlemgrd` with deterministic deny markers.
- Define the deterministic host reject-path proofs and the gated QEMU markers.
- Stay strictly inside the **`production-grade` baseline** — no sigchain envelope, no transparency log, no anti-downgrade, no boot anchors.

## Non-Goals

- Sigchain envelope, Merkle translog, SLSA provenance (→ v2 host RFC, seed `TASK-0197`).
- OS-side enforcement in `updated` / `storemgrd` (→ v2 OS RFC, seed `TASK-0198`).
- Boot-anchor / verified-boot / rollback-indices closure (→ boot-trust RFC, seed `TASK-0289`).
- System-set (`.nxs`) SBOM aggregation (same v2 boundary).
- Kernel changes.

## Constraints / invariants (hard requirements)

- **Determinism**: SBOM/repro are byte-identical across two consecutive packs of the same inputs (timestamps from `SOURCE_DATE_EPOCH`; stable key order; reuse `nexus-evidence` reproducible-`tar.gz`).
- **No fake success**: deterministic markers only after real `bundlemgrd`/`keystored`/`policyd` work; no UART literal that can fire without the install path actually executing.
- **Bounded resources**: explicit size caps for SBOM JSON, repro metadata, and bundle `meta/` entries; reject before parse if exceeded.
- **Security floor**:
  - Single allowlist authority: `keystored` only.
  - Deny-by-default everywhere (publisher, key, alg, integrity, audit).
  - Caller identity from kernel `sender_service_id`, never from payload strings.
  - No secrets ever logged; no secrets ever land in SBOM/repro (reuse `nexus-evidence::scan`).
  - Audit-or-fail: every allow/deny goes to logd; logd unreachable → install fails closed.
- **Stubs policy**: any v2-shaped artefact (sigchain envelope, translog, provenance) MUST NOT appear even as a stub in v1.

## Proposed design

### Contract / interface (normative)

#### 1. Bundle layout (extends `manifest.nxb`)

```text
<bundle>.nxb
├── manifest.nxb          (capnp; existing, ADR-0020)
├── payload.elf           (existing)
└── meta/                 (new in v1)
    ├── sbom.json         (CycloneDX 1.5; SHA-256 referenced from manifest meta)
    └── repro.env.json    (schema-versioned; SHA-256 referenced from manifest meta)
```

- **Path stability**: `meta/sbom.json` and `meta/repro.env.json` paths are fixed v1 contract. v2 (sigchain envelope, `TASK-0197`) MUST be able to add `meta/sigchain.nxb` (capnp) by appending without re-pack and without changing the v1 paths.
- **Content addressing**: `manifest.nxb` v1.1 already carries `payloadDigest` (SHA-256 of `payload.elf`, ADR-0020). v1 of this RFC adds analogous SHA-256 references for `meta/sbom.json` and `meta/repro.env.json` so the existing manifest signature transitively binds them. Schema bump for `BundleManifest` is in scope of this RFC.

#### 2. SBOM payload — CycloneDX JSON (per ADR-0021)

- File: `meta/sbom.json`
- Format: CycloneDX **JSON 1.5** (the named ADR-0021 interop example).
- Fields (deterministic subset):
  - `bomFormat`, `specVersion`, `serialNumber` (deterministic UUIDv5 keyed on `bundle.name + bundle.semver + payload.sha256`),
  - `metadata.timestamp` (`SOURCE_DATE_EPOCH`-derived),
  - `components[]` (best-effort: Cargo.lock slice or crate list),
  - `metadata.tools[]` (rustc version, target triple, build flags hash),
  - hashes per component (`SHA-256`).
- Serialisation: stable key order; no whitespace drift; `nexus-evidence::scan` runs before write — pack refuses if a secret-class string is present.

#### 3. Repro metadata — JSON, schema-versioned

- File: `meta/repro.env.json`
- Schema:

```json
{
  "schema_version": 1,
  "source_date_epoch": 1700000000,
  "rustc": "1.85.0",
  "target": "riscv64imac-unknown-none-elf",
  "rustflags": ["..."],
  "cargo_lock_sha256": "<hex>",
  "host": { "family": "linux", "arch": "x86_64" }
}
```

- Tool: `tools/repro/repro-verify` checks (a) payload SHA-256 matches manifest, (b) schema fields present + valid, (c) two consecutive runs produce byte-identical output.

#### 4. Publisher/key allowlist — `keystored` capnp method

Schema diff (ABI; CAUTION zone per `.cursorrules`):

```capnp
struct IsKeyAllowedRequest {
  publisher @0 :Text;
  alg       @1 :Text;
  pubkey    @2 :Data;
  # @3..@7 reserved-for-v2 (TASK-0197: transparency-log inputs, sigchain pointer)
}

struct IsKeyAllowedResponse {
  allowed @0 :Bool;
  reason  @1 :Text;   # open set; v1 labels: publisher_unknown | key_unknown | alg_unsupported | disabled
  # @2..@5 reserved-for-v2 (TASK-0197: translog index, sigchain pointer)
}
```

- **Single authority**: `keystored` is the only service that loads `recipes/signing/publishers.toml` and answers this method. `bundlemgrd` and `policyd` MUST NOT carry a parallel allowlist.
- **Schema-extensibility ratchet**: explicit `@N` field IDs with reserved-for-v2 gaps so `TASK-0197` can add transparency-log / sigchain fields without renumbering. `reason :Text` (not bool) and `alg :Text` (not enum) so v2 can extend reasons/algorithms without breaking v1 callers.

#### 5. Install-time enforcement order (`bundlemgrd`)

```text
1. signature verifies                                  → keystored::verify
2. policy decision is allow                            → policyd → keystored::is_key_allowed
3. payload digest matches manifest payloadDigest       → SHA-256 over payload.elf
4. meta/sbom.json digest matches manifest sbomDigest   → SHA-256 over meta/sbom.json
5. meta/repro.env.json digest matches manifest reproDigest → SHA-256 over meta/repro.env.json
```

- **Any failure** → reject with stable error label, audit event via logd, deterministic deny marker.
- **Audit-or-fail**: if logd is unreachable, install fails closed (no silent allow).
- **Markers**: stable labels only; variable values (publisher, key fingerprint, error label) live in the audit record, not in the marker string.

### Phases / milestones (contract-level)

- **Phase 0** — SBOM generator (`tools/sbom`) + bundle embedding (`tools/nxb-pack` extension); `BundleManifest` schema bump for `sbomDigest` field; host test that two packs are byte-identical and that the embedded SBOM round-trips through `cyclonedx-cli`.
- **Phase 1** — Repro metadata generator (`tools/repro`) + `repro-verify`; `BundleManifest` schema bump for `reproDigest` field; host test for schema validity + byte-identical pack.
- **Phase 2** — `keystored` capnp method `IsKeyAllowed` + `recipes/signing/publishers.toml` loader; `policyd` install-time hook; `bundlemgrd` enforcement (steps 1–5 above) + audit emission.
- **Phase 3** — Full host reject-path suite (`test_reject_*`), see "Proof / validation strategy" below.
- **Phase 4** — OS selftest install of allowed + unallowed test bundle, deterministic gated markers registered in `proof-manifest/markers/` (Phase-5 layout from `RFC-0038`).

## Security considerations

### Threat model

- **Publisher / key spoofing**: bundle signed by an unallowed publisher or with a key not in the registry.
- **Allowlist bypass**: stale or duplicated allowlist outside `keystored` flips a deny to allow.
- **Payload tamper**: `payload.elf` mutated post-sign; signature on manifest still verifies.
- **SBOM / repro tamper**: fake `meta/sbom.json` claiming benign provenance, or forged `repro.env.json` defeating `repro-verify`.
- **Secret leak via SBOM/repro**: build environment leaks credentials into dependency URLs, env vars, or build flags.
- **Marker spoof**: output that mimics `SELFTEST: sign policy * ok` without the install path executing.
- **Audit suppression**: install proceeds but audit event never reaches logd.

### Mitigations (security invariants)

- **Single allowlist authority** in `keystored` — `bundlemgrd` and `policyd` MUST NOT carry a parallel allowlist.
- **Deny-by-default** on every check (publisher / key / alg / signature / payload digest / SBOM digest / repro digest).
- **Channel-bound identity** (`sender_service_id`); never trust payload strings.
- **Bounded inputs** — explicit caps for SBOM, repro, and bundle entries.
- **Secret scanner** — `nexus-evidence::scan` runs before SBOM/repro embedding; pack refuses on hit.
- **Audit-or-fail** — install fails closed if logd unreachable.
- **Deterministic markers** — stable labels only (`12-debug-discipline.mdc`); variable values go to audit.
- **No nondeterministic timestamps** — `SOURCE_DATE_EPOCH` everywhere; two packs of the same inputs are byte-identical.

### DON'T DO

- DON'T duplicate the publisher/key allowlist outside `keystored`.
- DON'T accept "warn and continue" on policy fail — fail closed.
- DON'T derive caller identity from request payload strings.
- DON'T log raw key material, signatures, or full SBOM content (fingerprints OK, raw bytes not).
- DON'T let `bundlemgrd` skip audit emission to "save a round trip".
- DON'T embed wall-clock timestamps in SBOM or repro metadata.
- DON'T introduce a parallel manifest format under `meta/` — paths are v1 contract.
- DON'T add a sigchain envelope, translog, SLSA provenance, anti-downgrade logic, or boot anchors in v1 (gates 1–6 in `TASK-0029` Production-grade tier).

### Open risks

- **Reproducibility scope**: full bit-for-bit reproducibility across heterogeneous hosts is out of v1 scope; v1 captures the *evidence* (`repro.env.json`) and gates on payload digest + schema validity. Stronger gates land in v2 (`TASK-0197`).
- **Allowlist rotation**: v1 reads `publishers.toml` at startup; live rotation is out of scope and tracked under `TASK-0197`.

## Failure model (normative)

Stable error labels (open set; v1 starting set):

| Label | Where | Meaning |
|---|---|---|
| `policy.publisher_unknown` | `keystored::is_key_allowed` → `bundlemgrd` | Publisher not in `publishers.toml` |
| `policy.key_unknown` | same | Key not registered for that publisher |
| `policy.alg_unsupported` | same | Algorithm not in allowlist |
| `policy.disabled` | same | Publisher entry present but disabled |
| `integrity.payload_digest_mismatch` | `bundlemgrd` step 3 | `payload.elf` SHA-256 ≠ manifest `payloadDigest` |
| `integrity.sbom_digest_mismatch` | step 4 | `meta/sbom.json` SHA-256 ≠ manifest `sbomDigest` |
| `integrity.repro_digest_mismatch` | step 5 | `meta/repro.env.json` SHA-256 ≠ manifest `reproDigest` |
| `audit.unreachable` | `bundlemgrd` | logd capability unwired or send failed → install rejected |
| `pack.secret_leak` | `tools/sbom` / `tools/repro` | `nexus-evidence::scan` hit |
| `pack.repro_schema_invalid` | `tools/repro/repro-verify` | Schema fields missing/invalid |

- **No silent fallback**: every error returns a stable label; no "warn and continue" path exists.
- **Atomicity**: install is all-or-nothing; partial install state must not be observable from the running OS.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS
# SBOM + repro determinism
cargo test -p nxb-pack -- supply_chain
cargo test -p sbom -- determinism
cargo test -p repro -- verify
# Allowlist authority
cargo test -p keystored -- is_key_allowed
# Install-time enforcement
cargo test -p bundlemgrd -- supply_chain
# Reject-path suite (mandatory)
cargo test -p bundlemgrd -- test_reject_unknown_publisher \
                            test_reject_unknown_key \
                            test_reject_unsupported_alg \
                            test_reject_payload_digest_mismatch \
                            test_reject_sbom_digest_mismatch \
                            test_reject_repro_digest_mismatch \
                            test_reject_sbom_secret_leak \
                            test_reject_repro_schema_invalid \
                            test_reject_audit_unreachable
# CycloneDX schema + roundtrip proof (Phase 0)
cargo run -p nxb-pack -- --hello target/supplychain-proof
build/tools/cyclonedx-cli validate \
  --input-file target/supplychain-proof/meta/sbom.json \
  --input-version v1_5 --fail-on-errors
build/tools/cyclonedx-cli convert \
  --input-file target/supplychain-proof/meta/sbom.json \
  --input-format json \
  --output-file target/supplychain-proof/meta/sbom.roundtrip.xml \
  --output-format xml \
  --output-version v1_5
build/tools/cyclonedx-cli convert \
  --input-file target/supplychain-proof/meta/sbom.roundtrip.xml \
  --input-format xml \
  --output-file target/supplychain-proof/meta/sbom.roundtrip.json \
  --output-format json \
  --output-version v1_5
build/tools/cyclonedx-cli validate \
  --input-file target/supplychain-proof/meta/sbom.roundtrip.json \
  --input-version v1_5 --fail-on-errors
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os supply-chain
# Allow + deny selftest probes registered in proof-manifest/markers/
```

### Deterministic markers (gated; stable labels only)

- `bundlemgrd: sign policy allow ok`
- `bundlemgrd: sign policy deny ok`
- `SELFTEST: sign policy allow ok`
- `SELFTEST: sign policy deny ok`
- `SELFTEST: sign policy unknown publisher rejected ok`
- `SELFTEST: sign policy unknown key rejected ok`
- `SELFTEST: sign policy payload tamper rejected ok`

All markers MUST be registered in `source/apps/selftest-client/proof-manifest/markers/` and the relevant profile under `proof-manifest/profiles/` (`RFC-0038` Phase-5 layout). `verify-uart` deny-by-default enforces the ladder.

## Alternatives considered

1. **Capnp envelope around `(manifest, sbom, repro, sig)` already in v1** — rejected. ADR-0021 + the production-grade trajectory keep the canonical signed envelope as a v2 contract owned by `TASK-0197`. Locking it in v1 would either pre-empt v2 or force a re-pack later. The schema-extensibility ratchet on `keystored::IsKeyAllowed` plus stable `meta/` paths keep v2 a non-breaking addition.
2. **Allowlist in `policyd` (or duplicated in `bundlemgrd`)** — rejected. Two sources of truth for "is this publisher+key allowed" creates the exact stale-allowlist bypass we list under threats. `keystored` is the registry; `policyd` queries it; `bundlemgrd` enforces.
3. **SBOM as Cap'n Proto** — rejected per ADR-0021. SBOM is interop-first (`cyclonedx-cli`, scanners); JSON is the on-policy format. Canonical-contract use of capnp lives in the *signed envelope around* the SBOM, not in the SBOM payload itself.
4. **Skip SBOM digest in manifest, sign SBOM independently** — rejected. Two signatures double the rotation surface and break the "manifest signature transitively binds the bundle" property. Adding `sbomDigest` to the manifest is one schema-versioned line and keeps a single signature root.
5. **TOML for allowlist + capnp method** vs **all-capnp** — kept TOML for `publishers.toml` (human-edited authoring input per ADR-0021); kept capnp for the runtime IPC method (canonical contract per ADR-0020). This is the same canonical-vs-authoring split the rest of the repo uses.
6. **`updated` install-path enforcement in v1** — rejected. The Production-grade tier in `TASK-0029` explicitly gates this to `TASK-0198`. Touching `updated` here would silently expand v1 scope and pre-empt v2 OS contracts.

## Resolved open questions (pinned for v1)

These decisions are mirrored in `TASK-0029` and remain fixed for v1 scope.

1. **SBOM granularity**: v1 is payload-centric. `meta/sbom.json` covers bundle identity, payload material/digest, and declared package metadata. Full build-host graph closure is deferred to v2 provenance/sigchain work.
2. **Repro artefact format**: `meta/repro.env.json` is the only in-bundle repro contract artefact. Optional human-readable `.txt` output is CLI-only and not consumed by install policy.
3. **`publishers.toml` reload behavior**: `keystored` reads allowlist state once at startup in v1; live reload is a v2 follow-up.
4. **Multi-key per publisher**: v1 allowlist format supports multiple keys per publisher for rotation safety without ABI changes.
5. **`bundlemgrd` audit emission**: install allow/deny outcomes use a synchronous bounded audit emit/ack path; timeout or unreachable logd fails closed.

## RFC Quality Guidelines (for authors)

- Scope boundaries are explicit; cross-RFC ownership is linked (see "This RFC does NOT own").
- Determinism + bounded resources are specified in Constraints.
- Security invariants are stated (threat model, mitigations, DON'T DO).
- Proof strategy is concrete (host commands + QEMU markers).
- ABI claims are versioned (manifest schema bump + capnp `@N` field IDs with reserved-for-v2 gaps).
- Stubs are explicitly forbidden in v1 (gates 1–6 in `TASK-0029`).

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0**: SBOM generator + bundle embedding + `BundleManifest.sbomDigest` — proof: `cargo test -p nxb-pack -- supply_chain && cargo test -p sbom -- determinism` + `cyclonedx-cli` validate/convert/validate roundtrip
- [x] **Phase 1**: Repro metadata + `repro-verify` + `BundleManifest.reproDigest` — proof: `cargo test -p repro -- verify`
- [x] **Phase 2**: `keystored::IsKeyAllowed` capnp method + `policyd` hook + `bundlemgrd` enforcement order — proof: `cargo test -p keystored -- is_key_allowed && cargo test -p bundlemgrd -- supply_chain`
- [x] **Phase 3**: Host reject-path suite green — proof: `cargo test -p bundlemgrd -- test_reject_unknown_publisher test_reject_unknown_key test_reject_unsupported_alg test_reject_payload_digest_mismatch test_reject_sbom_digest_mismatch test_reject_repro_digest_mismatch test_reject_sbom_secret_leak test_reject_repro_schema_invalid test_reject_audit_unreachable`
- [x] **Phase 4**: OS selftest install of allowed + unallowed bundle, deterministic markers registered in `proof-manifest/markers/` and consumed by `verify-uart` — proof: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os supply-chain`
- [x] Task `TASK-0029` linked with stop conditions + proof commands.
- [x] QEMU markers (stable-label form) appear in `proof-manifest/markers/` and pass `verify-uart`.
- [x] Security-relevant negative tests (`test_reject_*`) green for every label in the Failure-model table.
