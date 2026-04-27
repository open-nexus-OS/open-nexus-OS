<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0021: Structured Data Formats (JSON vs Cap'n Proto)

**Status**: Accepted  
**Date**: 2026-01-26  
**Owners**: @runtime, @ui  

---

## Context

The repo uses structured data for multiple purposes:

- runtime IPC payloads,
- persisted `/state` snapshots,
- build artifacts used by tooling/tests (goldens),
- authoring inputs (human-edited files),
- interoperability/export bundles.

If we do not explicitly distinguish **canonical contracts** from **derived views**, we get format drift, nondeterministic bytes, and slow parsing in OS mode.

---

## Decision

### 1) Canonical contracts: Cap'n Proto

Use **Cap'n Proto** when the bytes are a **contract** (runtime, persistence, signing) and when we benefit from:

- determinism (stable bytes without “canonical JSON” hacks),
- bounded/fast parsing and **zero-copy** readers,
- schema evolution with versioning.

**Applies to:**

- **IPC contracts** (`tools/nexus-idl/schemas/*.capnp`)
- **Config v1 effective snapshots** (`TASK-0046` / `RFC-0044`): `configd` runtime/persistence authority uses Cap'n Proto bytes
  - Policy as Code v1 candidate roots are carried in those effective snapshots as `policy.root`
- **Scene-IR canonical artifact**: `.nxir` (Cap'n Proto)  
  - JSON is only a derived view for host goldens/debug
- **/state persisted snapshots**: `.nxs` (“Nexus Snapshot”, Cap'n Proto)
- **compiled i18n catalogs**: `.lc` (Cap'n Proto encoded; compact schema)

### 2) Derived views and authoring: JSON (and JSONL)

Use **JSON** when humans or external tools need to read/write it, or when the bytes are *not* the canonical contract:

- **Authoring inputs** (human-edited): e.g. i18n source catalogs
- **Config v1 authoring inputs**: layered `/system/config/*.json`, `/state/config/*.json`, and derived `nx config ... --json` views
- **Policy v1 authoring inputs**: TOML under `policies/`; `policies/manifest.json` is deterministic validation evidence, not a second authority
- **Debug/inspection views**: `--print-json`, `--export-json`
- **Host goldens** where diffs matter (`.nxir.json`)
- **Interop/export artifacts** where ecosystem compatibility matters (e.g. SBOM CycloneDX JSON)

Use **JSONL** for **append-only event logs** (bounded records, easy streaming), not for canonical snapshots.

---

## Naming conventions

### Canonical binaries (Cap'n Proto)

- **Scene-IR**: `*.nxir`
- **State snapshots**: `*.nxs`
- **i18n catalogs**: `*.lc` (Cap'n Proto encoded)
- **Bundle manifest**: `manifest.nxb` (already decided in ADR-0020)
- **Offline feeds/catalogs**: `*.nxf` (“Nexus Feed”, Cap'n Proto; deterministic, signable)
- **Compiled themes (optional)**: `*.nxtheme` (Cap'n Proto; derived from `*.nxtheme.toml`)

### Derived JSON views

- Suffix with `.json` and document that it is a derived view:
  - `.nxir.json` (derived from `.nxir`)
  - `nx <tool> export --json` outputs deterministic JSON

---

## Consequences

### Positive

- Single source of truth bytes for runtime/persistence.
- Determinism becomes easy to prove (byte-stable artifacts).
- OS paths stay fast and bounded (no JSON tokenization on hot paths).
- Humans and tooling keep readable outputs via JSON views.

### Negative

- Requires schemas and codegen for canonical binaries (upfront work).
- JSON views must be explicitly derived (avoid “two sources of truth”).

---

## References / Linked Work

- ADR-0020: Bundle Manifest Format (`manifest.nxb` with Cap'n Proto)
- Config v1: `tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md`, `docs/rfcs/RFC-0044-config-v1-configd-schema-layering-2pc-host-first-os-gated.md`
- Policy as Code v1: `tasks/TASK-0047-policy-as-code-v1-unified-engine.md`, `docs/rfcs/RFC-0045-policy-as-code-v1-unified-policy-tree-evaluator-explain-dry-run-learn-enforce-nx-policy.md`
- DSL Scene-IR: `TASK-0075`, `TASK-0077`, `TASK-0079`
- Settings persistence: `TASK-0225`
- Recents persistence: `TASK-0082`
- i18n catalogs: `TASK-0240`, `TASK-0241`
