---
title: TASK-0029 Supply-Chain v1: bundle SBOM (CycloneDX) + repro metadata + signature allowlist policy (host-first, OS-gated)
status: In Review
owner: @runtime @security
created: 2025-12-22
updated: 2026-04-22
depends-on:
  - TASK-0006
  - TASK-0007
  - TASK-0008B
follow-up-tasks:
  - TASK-0197
  - TASK-0198
  - TASK-0289
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Contract seed (RFC): docs/rfcs/RFC-0039-supply-chain-v1-bundle-sbom-repro-sign-policy.md
  - Depends-on (manifest.nxb baseline, Done): tasks/TASK-0007-updates-packaging-v1_1-userspace-ab-skeleton.md
  - Depends-on (device identity/keys, Done): tasks/TASK-0008B-device-identity-keys-v1-virtio-rng-rngd-keystored-keygen.md
  - Depends-on (audit sink, Done): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - ADR (manifest format = capnp): docs/adr/0020-manifest-format-capnproto.md
  - ADR (canonical-vs-interop format policy): docs/adr/0021-structured-data-formats-json-vs-capnp.md
  - Policy baseline: docs/security/signing-and-policy.md
  - Packaging docs: docs/packaging/nxb.md
  - System-set packaging: docs/packaging/system-set.md
  - Keystored capnp schema (to extend): tools/nexus-idl/schemas/keystored.capnp
  - Production-gate tier (Updates/Packaging/Recovery → production-grade): tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md
  - Testing contract: scripts/qemu-test.sh
  - Standards (security): docs/standards/SECURITY_STANDARDS.md
  - Standards (debug discipline / deterministic markers): .cursor/rules/12-debug-discipline.mdc
---

## Context

We want auditable supply-chain basics:

- SBOM per bundle,
- reproducibility evidence / reproducibility gates,
- install-time signature policy enforcement (publisher/key allowlist).

Repo reality today (verified 2026-04-21):

- `tools/nxb-pack` writes the canonical `manifest.nxb` (capnp-encoded, see `tools/nxb-pack/src/main.rs:50`). The earlier `manifest.json` drift was closed by `TASK-0007` (`Done`, "manifest.nxb unification") — see "Red flags / decision points" below.
- `tools/nxs-pack/` exists in-tree (system-set packer baseline; SBOM aggregation is still a follow-up — see `TASK-0197`).
- `source/services/updated/` exists in-tree, but its supply-chain enforcement path (sigchain / translog / SBOM verification at staging+apply) is explicitly owned by `TASK-0198`, not by this task.
- `bundlemgrd` exists (host and OS-lite paths) and already uses `keystored::verify` for raw Ed25519 signature verification.
- `keystored` capnp schema today (`tools/nexus-idl/schemas/keystored.capnp`) only exposes `getAnchors / verify / deviceId`; it has **no allowlist-check method**. v1 must add one (see "Plan" §3 and "Red flags" YELLOW-keystored-API).
- `nexus-evidence` (host-only crate, landed in `TASK-0023B` Phase 5) already provides reproducible `tar.gz` packing (`mtime=0`, fixed gzip OS byte, lex-sorted entries) and a deny-by-default secret scanner. Reuse those primitives instead of re-rolling determinism for SBOM/repro artefacts.
- `TASK-0006` (logd / audit sink, `Done`) and `TASK-0008B` (rngd + keystored keygen, `Done`) are both available — audit events and real device keys are usable right now, no gating needed.

### Format policy (per ADR-0020 + ADR-0021) — already decided, reused here

To prevent re-litigation: the structured-data split is already locked by accepted ADRs, and v1 conforms to it.

- **Canonical bytes (signed contract)** → Cap'n Proto. Examples already in tree: `manifest.nxb` (ADR-0020), `.nxs` state snapshots, `.nxir` scene IR, `.nxf` feeds, `*.lc` i18n catalogs.
- **Interop / export artefacts (ecosystem-readable)** → JSON. ADR-0021 §"Derived views and authoring" names **"SBOM CycloneDX JSON"** explicitly as the canonical example of this category.

Implication for v1:

- The SBOM payload itself is **CycloneDX JSON** (`meta/sbom.json`) — primary purpose is interop with `cyclonedx-cli`, scanners, and external tooling. This is on-policy, not a deviation.
- The **canonical signed envelope** that binds `(manifest.nxb, sbom.json, repro.env.json, signature)` into one cryptographically verifiable contract IS the right place for Cap'n Proto — but that envelope is the **v2 sigchain** (`TASK-0197`), not v1. v1 only embeds artefacts into the existing `.nxb` under a stable `meta/` path with content-hash-addressable references.
- v1 must therefore design the `meta/` layout so that the v2 sigchain envelope can wrap it **without re-pack and without changing the bundle wire format** (see "Production-grade tier" gates below).

## Goal

Deliver supply-chain v1 for **bundles** first:

- Generate and embed a CycloneDX JSON SBOM inside each `.nxb`.
- Capture reproducibility metadata and provide a deterministic host-side verify step.
- Enforce a publisher/key allowlist policy in `bundlemgrd` install path.
- Provide host tests and (once OS bundle install path exists) OS selftest markers.

## Non-Goals

- System-set (`.nxs`) SBOMs and updated-stage enforcement (follow-up once tools/services exist).
- Full reproducible builds for *all* targets immediately (we start with deterministic hashing + metadata capture + a best-effort gate).
- Kernel changes.

## Constraints / invariants (hard requirements)

- Kernel untouched.
- No fake success: markers only after real verification steps.
- Deterministic parsing:
  - SBOM JSON must be canonicalised or hashed based on canonical fields (avoid “timestamp drift”).
  - Repro metadata must use `SOURCE_DATE_EPOCH` and explicitly record relevant inputs.
- No blanket `allow(dead_code)`; avoid `unwrap/expect`.

## Red flags / decision points

- **RESOLVED — manifest format drift (was RED)**:
  - History: `tools/nxb-pack` previously wrote `manifest.json`, drifting from the canonical `manifest.nxb` direction.
  - Closure: `TASK-0007` (`Done`, "manifest.nxb unification") landed the canonical capnp manifest. `tools/nxb-pack/src/main.rs:50` now emits `manifest.nxb` directly. Supply-chain v1 builds on top of this baseline and **must not** introduce a parallel format. SBOM and repro artefacts live alongside the manifest under a stable `meta/` path so they remain manifest-format-independent.
- **YELLOW (reproducibility scope)**:
  - "Bit-for-bit reproducible" across environments is hard. v1 gates on:
    - stable digest of payload bytes,
    - build metadata capture (rustc, flags, target, crate graph hash),
    - optional verification re-run in CI/host toolchain.
  - Reuse `nexus-evidence` reproducible-`tar.gz` primitives (`mtime=0`, fixed gzip OS byte, lex-sorted) so the SBOM/repro embedding inherits the same determinism floor that `TASK-0023B` Phase 5 already proved.
- **YELLOW (policy authority)**:
  - Publisher allowlists are rooted in a **single authority**: `keystored` owns the anchors/key registry and answers `is_key_allowed(publisher, alg, pubkey)`; `policyd` owns the install-time decision (allow/deny) and audit context; `bundlemgrd` only enforces what `policyd` returns. No duplicated allowlist logic in `bundlemgrd`.
- **YELLOW (keystored API surface)**:
  - `tools/nexus-idl/schemas/keystored.capnp` today exposes only `getAnchors / verify / deviceId`. v1 must add an explicit `IsKeyAllowedRequest / IsKeyAllowedResponse` capnp method (concrete schema bump, RFC-0014/RFC-0009 dep-gate clean) — not "or equivalent RPC". This is an ABI touch and is in CAUTION territory per `.cursorrules`; treat the schema diff as part of the task review.
- **YELLOW (deterministic markers)**:
  - Per `.cursor/rules/12-debug-discipline.mdc`, gating markers must not embed run-variable data. Replace any `bundlemgrd: sign policy ok (publisher=<…>)` form with stable labels (e.g. `bundlemgrd: sign policy allow ok` / `bundlemgrd: sign policy deny ok`); publisher identity goes into the audit record (logd), not the marker string.

## Security

This task is security-relevant (signing, key registry, install-time policy, supply-chain artefacts). Section is mandatory per `.cursorrules`.

### Threat model

- **Publisher / key spoofing**: attacker submits a bundle signed by an unallowed publisher or with a key not in the registry, attempting to gain install authority.
- **Allowlist bypass**: attacker exploits a duplicated or stale allowlist outside `keystored` to get a deny-decision flipped to allow.
- **Payload tamper**: attacker mutates `payload.elf` after signing; signature still verifies on the manifest but payload digest no longer matches.
- **SBOM tamper / injection**: attacker injects fake `meta/sbom.json` claiming benign provenance, or adds entries to suppress later vulnerability scans.
- **Repro-meta forgery**: attacker writes plausible `meta/repro.env.json` (rustc, flags, source date) without a corresponding rebuild, defeating the host-side `repro-verify` step.
- **Secret leak via SBOM/repro**: build environment leaks credentials into SBOM dependency URLs, repro env vars, or build flags.
- **Marker spoof**: attacker generates output that mimics `SELFTEST: sign policy * ok` without the real bundlemgrd path running.
- **Audit suppression**: install-time policy decisions execute but never reach logd, hiding deny events.

### Security invariants (MUST hold)

- **Single allowlist authority**: only `keystored` (anchors/key registry) answers "is this publisher+alg+key allowed". `bundlemgrd` and `policyd` MUST NOT carry a parallel allowlist.
- **Deny-by-default**: unknown publisher, unknown key, unsupported algorithm, malformed signature, or payload digest mismatch → reject install with stable error label; no "warn-and-continue".
- **Identity from channel, not payload**: `bundlemgrd` derives caller identity from `sender_service_id` (kernel IPC), never from a string field in the request payload.
- **Bounded inputs**: SBOM JSON, repro metadata, and bundle entries MUST have explicit size caps; reject before parsing if exceeded.
- **No secrets in artefacts**: SBOM and repro metadata MUST flow through the `nexus-evidence::scan` deny-by-default secret scanner before being embedded; PEM blocks, `*PRIVATE_KEY*=…` env strings, and ≥64-char base64 high-entropy blobs MUST refuse the pack step.
- **Audit-or-fail**: every allow/deny decision MUST emit an audit event via logd; if logd is unreachable, the install MUST fail closed (no silent allow).
- **Deterministic markers**: gating UART markers carry stable labels only; variable values (publisher, key fingerprint) live in audit records, not in markers (`12-debug-discipline.mdc`).
- **No nondeterministic timestamps**: SBOM uses `SOURCE_DATE_EPOCH`; repro metadata records the same. Two consecutive packs of the same inputs MUST produce byte-identical SBOM and repro artefacts.

### DON'T DO

- DON'T duplicate the publisher/key allowlist outside `keystored`.
- DON'T accept "warn and continue" on policy fail — fail closed.
- DON'T derive caller identity from request payload strings.
- DON'T log raw key material, signatures, or SBOM content (signature/key fingerprints are OK; raw bytes are not).
- DON'T let `bundlemgrd` skip the audit emission to "save a round trip".
- DON'T embed wall-clock timestamps in SBOM or repro metadata.
- DON'T introduce a parallel manifest format under `meta/` — SBOM and repro live alongside the canonical `manifest.nxb`.
- DON'T mark a publisher allowlist update in production without going through `keystored` rotation procedure (out of v1 scope; tracked in `TASK-0197`).

### Reject-path proof (host) — MUST be added

- `test_reject_unknown_publisher` — install bundle signed by publisher not in allowlist → stable `policy.publisher_unknown` error.
- `test_reject_unknown_key` — install bundle signed by allowed publisher with key not in registry → stable `policy.key_unknown` error.
- `test_reject_unsupported_alg` — install bundle signed with algorithm not in allowlist → stable `policy.alg_unsupported` error.
- `test_reject_payload_digest_mismatch` — install bundle whose `payload.elf` was mutated post-sign → stable `integrity.payload_digest_mismatch` error.
- `test_reject_sbom_digest_mismatch` — install bundle whose `meta/sbom.json` bytes no longer match manifest digest → stable `integrity.sbom_digest_mismatch` error.
- `test_reject_repro_digest_mismatch` — install bundle whose `meta/repro.env.json` bytes no longer match manifest digest → stable `integrity.repro_digest_mismatch` error.
- `test_reject_sbom_secret_leak` — pack bundle whose generated SBOM contains a PEM block / private-key path → pack step refuses (reuse `nexus-evidence::scan` semantics).
- `test_reject_repro_schema_invalid` — `repro-verify` rejects a metadata file with missing/extra schema fields.
- `test_reject_audit_unreachable` — `bundlemgrd` install path with logd capability unwired MUST fail (no silent allow).

### QEMU hardening markers (gated)

Once OS bundle install path + keystored are functional in QEMU (this task wires the gate):

- `bundlemgrd: sign policy allow ok` (stable label; publisher in audit record only)
- `bundlemgrd: sign policy deny ok`
- `SELFTEST: sign policy allow ok`
- `SELFTEST: sign policy deny ok`
- `SELFTEST: sign policy unknown publisher rejected ok`
- `SELFTEST: sign policy unknown key rejected ok`
- `SELFTEST: sign policy payload tamper rejected ok`

## Production-grade tier

Per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`, the "Updates, Packaging & Recovery" group is tagged **`production-grade`** (release-critical for kiosk/IoT and consumer floors).

**This task is the `production-grade` BASELINE for the supply-chain leg of that group.** It is *not* the full closure. Full `production-grade` is reached only at `TASK-0197` + `TASK-0198` + `TASK-0289` closure. v1 must stay on that trajectory without locking the wrong contract.

### v1 baseline floor (this task delivers)

- Bundle-level CycloneDX JSON SBOM under stable `meta/sbom.json`.
- Bundle-level repro metadata under stable `meta/repro.env.json` (`SOURCE_DATE_EPOCH`-based).
- Single-authority publisher/key allowlist in `keystored` (capnp `IsKeyAllowedRequest/Response`).
- `policyd` decision + `bundlemgrd` enforcement (verify → policy → digest), deny-by-default, audit-or-fail via logd.
- Host `test_reject_*` set covering allowlist, integrity, secret-leak, schema-validity, audit-unreachable.
- QEMU markers (gated, deterministic-label form) once the OS install path is wired in selftest-client.

### Explicit hard gates — v1 MUST NOT pre-empt later tasks

These gates are mechanically enforceable in review/CI and exist so v1 stays *baseline* and does not freeze a contract that v2/v3 must change:

1. **No sigchain envelope.** v1 MUST NOT introduce a binding capnp envelope around `(manifest, sbom, repro, signature)`. That envelope is owned by `TASK-0197`. v1 binds via SHA-256 references from the existing manifest only.
2. **No transparency / Merkle translog.** v1 MUST NOT add any append-only log of install/update decisions, Merkle root recording, or auditor-replay structure. Owned by `TASK-0197`.
3. **No SLSA-style provenance records.** v1 captures build environment in `repro.env.json` (rustc, flags, target, crate graph hash), but MUST NOT introduce SLSA attestations, in-toto links, or provenance envelopes. Owned by `TASK-0197`.
4. **No anti-downgrade enforcement.** v1 MUST NOT add rollback indices, monotonic version counters, or downgrade-rejection logic. Owned by `TASK-0198` (OS-side) and `TASK-0289` (boot-anchored).
5. **No `updated` / `storemgrd` install-path changes.** v1 touches `bundlemgrd` only. The `updated` (staging+apply) and `storemgrd` (install/update) paths are explicitly owned by `TASK-0198`. Touched-paths allowlist enforces this.
6. **No boot-anchor / measured-boot work.** v1 stays inside the running OS. Verified-boot anchors, rollback indices tied to boot, and measured-boot handoff are owned by `TASK-0289`.
7. **Schema-extensibility ratchet.** Any new capnp schema added by v1 (specifically the `keystored` allowlist method) MUST:
   - use named fields with explicit `@N` IDs and leave a documented gap (e.g. `@0..@5` reserved-for-v2) so `TASK-0197` can add transparency-log / sigchain fields without renumbering;
   - return a `reason: Text` (stable label) — never a bare `Bool` — so v2 can add new reason classes without breaking v1 callers;
   - keep `alg :Text` (not a closed enum) so v2 can add new algorithms without a schema break.
8. **`meta/` layout-extensibility ratchet.** SBOM and repro live under `meta/<file>` with paths that are content-hash-addressable from the manifest. v2 (`TASK-0197`) MUST be able to add `meta/sigchain.nxb` (capnp envelope) by appending to the bundle without re-packing or re-signing existing entries.
9. **Format policy compliance.** SBOM is JSON (per ADR-0021); the `meta/` payload format MUST NOT silently introduce a parallel TOML/YAML/protobuf carrier. Any new bytes added to a bundle by v1 are either (a) CycloneDX JSON (interop) or (b) a small JSON schema-versioned descriptor.
10. **No marker drift.** v1 MUST NOT emit gating markers outside the stable-label form (`12-debug-discipline.mdc`); publisher/key/fingerprint values go to logd only. Markers MUST be registered in `proof-manifest/markers/` (Phase-5 layout).

### Trajectory map (where this task sits)

```text
TASK-0029 (v1, this task)        → host-first SBOM + repro + allowlist + bundlemgrd enforcement
   │                                [production-grade BASELINE]
   ▼
TASK-0197 (v2a, host-side)        → sigchain envelope + Merkle translog + SLSA provenance
   │                                [production-grade core, host]
   ▼
TASK-0198 (v2b, OS-side)          → enforcement in updated/storemgrd/bundlemgrd + anti-downgrade
   │                                [production-grade core, OS]
   ▼
TASK-0289 (boot-trust floor)      → verified-boot anchors + rollback indices + measured boot
                                    [production-grade closure across boot+OS]
```

Do not use this task alone as evidence of `production-grade` anti-rollback, transparency, or boot trust.

## Contract sources (single source of truth)

- Bundle install path: `source/services/bundlemgrd`
- Signature verification primitive: `keystored` capnp verify API
- Allowlist authority (to be extended in this task): `tools/nexus-idl/schemas/keystored.capnp`
- Packaging direction (canonical manifest = capnp): `docs/packaging/nxb.md` + ADR-0020 + TASK-0007 (`Done`)
- Structured-data format policy (where SBOM/repro fit): ADR-0021 — SBOM CycloneDX JSON is the named interop example; `manifest.nxb` (capnp) stays the canonical signed contract
- Reproducible-tar primitives: `source/libs/nexus-evidence/` (READ-ONLY in this task)
- Production-gate tier: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Updates/Packaging/Recovery → `production-grade`)
- QEMU marker contract: `scripts/qemu-test.sh` + `source/apps/selftest-client/proof-manifest/`

## Stop conditions (Definition of Done)

### Proof (Host) — required

- Deterministic host tests that:
  - create a test bundle with embedded SBOM and repro metadata,
  - verify install succeeds with an allowed publisher/key,
  - reject install with unknown publisher/key (stable error),
  - reject install if payload digest mismatch (stable integrity error),
  - verify repro metadata schema is stable and `repro-verify` produces deterministic results.

### Proof (OS / QEMU) — gated

Once OS bundle install path + keystored are functional in QEMU:

- `bundlemgrd: sign policy allow ok` (stable label; publisher value lives in the audit record, never in the marker)
- `bundlemgrd: sign policy deny ok`
- `SELFTEST: sign policy allow ok`
- `SELFTEST: sign policy deny ok`
- `SELFTEST: sign policy unknown publisher rejected ok`
- `SELFTEST: sign policy unknown key rejected ok`
- `SELFTEST: sign policy payload tamper rejected ok`

Notes:

- Postflight scripts must delegate to canonical tests/harness; no independent "log greps = success".
- Markers MUST be added to `source/apps/selftest-client/proof-manifest/markers/` (Phase-5 layout) and registered in the appropriate profile under `proof-manifest/profiles/`. `verify-uart` deny-by-default will then enforce the ladder. No marker outside the manifest.
- Any QEMU evidence bundle produced for this task MUST seal cleanly under both `--policy=bringup` (local dev) and `--policy=ci` (CI), reusing the `TASK-0023B` Phase-5 pipeline.

## Touched paths (allowlist)

- `tools/nxb-pack/` (embed SBOM + repro metadata into bundle, reusing reproducible-tar primitives where possible)
- `tools/sbom/` (new: generate CycloneDX JSON v1.5)
- `tools/repro/` (new: repro metadata capture + `repro-verify` tool)
- `source/services/bundlemgrd/` (install-time enforcement; routes decisions through `policyd`)
- `source/services/keystored/` (allowlist check + key registry API implementation)
- `source/services/policyd/` (allow/deny decision + audit context)
- `tools/nexus-idl/schemas/keystored.capnp` (**ABI**: add `IsKeyAllowedRequest / IsKeyAllowedResponse`; CAUTION zone — schema diff is part of task review)
- `recipes/signing/` (new allowlist TOML)
- `source/libs/nexus-evidence/` (READ-ONLY — reuse `scan` + reproducible-tar; no API change here)
- `tests/` (host tests, including the `test_reject_*` set listed in Security)
- `docs/supplychain/` (new docs)
- `docs/testing/index.md`
- `scripts/qemu-test.sh` (gated marker update)
- `source/apps/selftest-client/proof-manifest/` (new markers + profile registration; do NOT touch `[meta]` schema)

## Resolved open questions (pinned for v1)

These decisions are frozen for TASK-0029 implementation and mirrored in `RFC-0039`.

1. **SBOM granularity**: v1 is payload-centric. `meta/sbom.json` covers bundle identity, payload digest/material, and declared package metadata; it does not freeze a full build-host crate-graph closure.
2. **Repro artefact format**: only `meta/repro.env.json` is part of the bundle contract. Any human-readable `.txt` rendering is CLI output only and never install-time input.
3. **`publishers.toml` reload model**: `keystored` loads the allowlist once at startup in v1. Live/signal reload is explicitly deferred to supply-chain v2 follow-ups.
4. **Publisher key cardinality**: v1 supports multiple keys per publisher to permit key rotation without ABI or file-format churn.
5. **Audit emission semantics**: `bundlemgrd` uses synchronous, bounded audit emit/ack before final success. If logd is unreachable or the audit path times out, install fails closed.

## Plan (small PRs)

1. **SBOM generator (host tool)**
   - Add `tools/sbom` to emit CycloneDX JSON (v1.5) — explicitly the ADR-0021-blessed interop format for SBOMs; this is *not* a deviation from the repo's "capnp for canonical contracts" rule.
   - Contents:
     - bundle name/version/publisher,
     - sha256 hashes (payload + important meta files),
     - dependency list (best effort: Cargo.lock slice or crate list if available),
     - build environment (rustc version, target triple, flags).
   - Embed into `.nxb` as `meta/sbom.json` — path stable, content-hash-addressable from the manifest, layout chosen so `TASK-0197` can append `meta/sigchain.nxb` (capnp envelope) later without re-pack (gate #8 above).
   - Determinism: serialise with stable key order; timestamps from `SOURCE_DATE_EPOCH`; reuse `nexus-evidence` reproducible-tar when adding to the bundle so byte-identical packs are guaranteed.

2. **Repro metadata + verifier**
   - Add `tools/repro`:
     - write `meta/repro.env.json` (schema versioned, timestamps from `SOURCE_DATE_EPOCH`)
     - `repro-verify` checks:
       - payload digest matches manifest metadata
       - env schema present and valid.

3. **Publisher/key allowlist policy**
   - Add `recipes/signing/publishers.toml` with:
     - allowed publishers,
     - allowed algorithms,
     - allowed keys (fingerprints or raw pubkeys).
   - Extend `tools/nexus-idl/schemas/keystored.capnp` with an explicit method (schema-extensibility ratchet from gate #7):
     - `IsKeyAllowedRequest { publisher @0 :Text; alg @1 :Text; pubkey @2 :Data; # @3..@7 reserved-for-v2 (TASK-0197: transparency-log inputs) }`
     - `IsKeyAllowedResponse { allowed @0 :Bool; reason @1 :Text; # @2..@5 reserved-for-v2 (TASK-0197: translog index, sigchain pointer) }`
     - Stable initial reason labels: `publisher_unknown` / `key_unknown` / `alg_unsupported` / `disabled` (open set — v2 may add classes without breaking v1 callers).
     - `alg :Text` (not enum) so v2 can add algorithms without a schema break.
     - Schema bump is part of the task review (CAUTION zone per `.cursorrules`); document the reserved-field gap in the schema comment so v2 doesn't have to re-discover the contract.
   - Implement the method in `keystored` (load `publishers.toml` once at startup; deny-by-default on unknown).
   - `policyd` consults `keystored::is_key_allowed` and returns the install decision + audit context to `bundlemgrd`.
   - `bundlemgrd` install path enforces (in order):
     1. signature verifies (`keystored::verify`),
     2. `policyd` decision is `allow` (which itself relies on `keystored::is_key_allowed`),
     3. payload digest matches manifest metadata.
     - Any failure → reject with stable error label, audit event via logd, deterministic deny marker.
   - Audit events go through logd (TASK-0006, `Done`) — fail closed if logd unreachable.
   - Markers use stable labels only (`bundlemgrd: sign policy allow ok` / `bundlemgrd: sign policy deny ok`); variable values land in the audit record.

4. **Tests**
   - Host tests for allow/deny/tamper + sbom presence + repro verify.

5. **OS selftest + markers (gated)**
   - Add selftest install of an allowed test bundle and an unallowed test bundle.

## Docs (English)

- `docs/supplychain/sbom.md`: where SBOM lives in `.nxb`, how to inspect.
- `docs/supplychain/repro.md`: schema, `SOURCE_DATE_EPOCH`, verifier usage.
- `docs/supplychain/sign-policy.md`: publisher allowlist format, key rotation, failure modes.
- `docs/testing/index.md`: how to run host tests; expected OS markers once enabled.

## Implementation checkpoint (2026-04-22)

- C-01 complete: deterministic SBOM generation + embedding in `tools/nxb-pack`.
- C-02 complete: repro metadata capture + verify flow (`tools/repro`).
- C-03/C-04/C-05 complete: `keystored` ABI + allowlist runtime + policy/enforcement chain in `bundlemgrd`.
- C-06 complete: all required host reject-path tests are green.
- C-07 complete for wiring: marker registrations and `supply-chain` profile wiring are in `proof-manifest`.
- C-08 docs/state sync complete in this task file, RFC, testing index, and status views.

Host proof snapshot (green):

```bash
cargo test -p nxb-pack -- supply_chain
cargo test -p sbom -- determinism
cargo test -p repro -- verify
cargo test -p keystored -- is_key_allowed
cargo test -p bundlemgrd -- supply_chain
cargo test -p bundlemgrd test_reject_unknown_publisher
cargo test -p bundlemgrd test_reject_unknown_key
cargo test -p bundlemgrd test_reject_unsupported_alg
cargo test -p bundlemgrd test_reject_payload_digest_mismatch
cargo test -p bundlemgrd test_reject_sbom_digest_mismatch
cargo test -p bundlemgrd test_reject_repro_digest_mismatch
cargo test -p bundlemgrd test_reject_sbom_secret_leak
cargo test -p bundlemgrd test_reject_repro_schema_invalid
cargo test -p bundlemgrd test_reject_audit_unreachable
# CycloneDX schema + roundtrip proof
cargo run -p nxb-pack -- --hello target/supplychain-proof
build/tools/cyclonedx-cli validate --input-file target/supplychain-proof/meta/sbom.json --input-version v1_5 --fail-on-errors
build/tools/cyclonedx-cli convert --input-file target/supplychain-proof/meta/sbom.json --input-format json --output-file target/supplychain-proof/meta/sbom.roundtrip.xml --output-format xml --output-version v1_5
build/tools/cyclonedx-cli convert --input-file target/supplychain-proof/meta/sbom.roundtrip.xml --input-format xml --output-file target/supplychain-proof/meta/sbom.roundtrip.json --output-format json --output-version v1_5
build/tools/cyclonedx-cli validate --input-file target/supplychain-proof/meta/sbom.roundtrip.json --input-version v1_5 --fail-on-errors
```

OS/QEMU note:

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os supply-chain` is green.
- `verify-uart` is clean for `profile=supply-chain`; evidence bundle emitted under `target/evidence/`.

## Follow-ups (separate tasks)

- System-set (`.nxs`) SBOM aggregation and updated-stage enforcement: `tools/nxs-pack/` and `source/services/updated/` now exist in-tree, but extending them with SBOM/sigchain enforcement is owned by `TASK-0197` (host core) and `TASK-0198` (OS enforcement). v1 stays scoped to bundles.
- Stronger reproducibility gates in CI for selected artifacts (tracked under `TASK-0197`).
- Supply-chain hardening v2 (sigchain envelope + local transparency log + SBOM validation/provenance + anti-downgrade enforcement) → `TASK-0197` / `TASK-0198`.
- Boot-trust closure (verified-boot anchors + rollback indices + measured boot handoff) → `TASK-0289`.
