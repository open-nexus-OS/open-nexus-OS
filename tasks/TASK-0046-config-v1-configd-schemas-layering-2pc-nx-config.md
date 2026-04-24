---
title: TASK-0046 Config & Schema v1: configd broker + Cap'n Proto canonical snapshot + JSON Schema validation + deterministic layering + 2PC reload (+ nx config)
status: In Progress
owner: @runtime
created: 2025-12-22
depends-on: []
follow-up-tasks:
  - TASK-0047
  - TASK-0262
  - TASK-0266
  - TASK-0268
  - TASK-0273
  - TASK-0285
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Contract seed (this task): docs/rfcs/RFC-0044-config-v1-configd-schema-layering-2pc-host-first-os-gated.md
  - DevX CLI: tasks/TASK-0045-devx-nx-cli-v1.md
  - Policy as Code (consumer): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Persistence substrate: tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Audit sink (optional): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - DSoftBus hardening (future consumer): tasks/TASK-0030-dsoftbus-discovery-authz-hardening-mdns-ttl-acl-ratelimit.md
  - Metrics gatekeeping (future consumer): tasks/TASK-0014-observability-v2-metrics-tracing.md
  - Structured format policy: docs/adr/0021-structured-data-formats-json-vs-capnp.md
  - Service control-plane data model: docs/adr/0017-service-architecture.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

The repo currently uses multiple “config surfaces”:

- `recipes/*` files for policy and subsystem defaults,
- ad-hoc env vars,
- ad-hoc per-service config parsing (where implemented).

We want a single, validated, auditable “source of truth” for runtime configuration:

- deterministic layering,
- schema validation with actionable diagnostics,
- safe reload with a two-phase prepare→commit/abort protocol,
- minimal initial adoption by a few key services.

Repo reality today:

- Many target services are still planned tasks (metricsd/traced/etc.).
- `/state` persistence baseline is already landed (`TASK-0009` is `Done`), so durable config override path is no longer a hard blocker.
- `tools/nx` baseline is already landed (`TASK-0045` is `Done`), so `nx config ...` extends an existing canonical CLI surface (no `nx-*` drift).
- Future management note:
  - `configd` is also the natural config/profile distribution consumer for later family / school / enterprise / fleet
    management work. Managed settings should arrive through the same typed, schema-validated config path rather than a
    second ad-hoc management config surface.

This task is **host-first** and **OS-gated**.

## Goal

Deliver:

1. Versioned JSON Schemas for authoring/validation for a small set of subsystems (dsoftbus, metrics, tracing, sandbox/security, sched).
2. A `nexus-config` library: layered loader + schema validator + deterministic canonicalization bridge.
3. A `configd` service: Get/Subscribe + Reload + 2PC apply orchestration with versioning/timeouts and a canonical Cap'n Proto effective snapshot contract.
4. Wire 3–4 services (as available) to accept config in a prepare/commit manner.
5. Extend `nx` with `nx config` subcommands (validate/effective/diff/push/reload/where).

## Non-Goals

- Kernel changes.
- A fully featured distributed config system.
- An unbounded “dynamic config language”. Keep schema strict and configs small.

## Constraints / invariants (hard requirements)

- Kernel untouched.
- Deterministic layering (stable precedence, stable merge strategy).
- Bounded parsing and bounded config sizes (caps on file size, object depth, list lengths).
- Canonical contract boundary follows ADR-0021:
  - Cap'n Proto for canonical persisted/runtime contract snapshots,
  - JSON for authoring/validation and derived CLI/debug views only.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success: reload markers only after validation + (if enabled) successful 2PC commit.

## Red flags / decision points

- **RESOLVED (OS gating)**:
  - `/state` persistence gate is closed (`TASK-0009` is `Done`), so `/state/config` durability is available.
  - Remaining gating for OS proof is now service adoption/marker wiring (not persistence availability).
- **RESOLVED (JSON vs canonical contract)**:
  - JSON remains authoring/validation surface (JSON Schema + deterministic canonical JSON normalization).
  - Canonical effective config contract is a Cap'n Proto snapshot for runtime/persistence boundaries (ADR-0021/0017).
- **YELLOW (legacy recipes mapping)**:
  - Supporting legacy `recipes/*` ingestion is useful, but must be explicitly labeled as a migration adapter
    to avoid two sources of truth.

## Security section

### Threat model (v1 scope)

- Untrusted config input can enter via local files (`/system/config`, `/state/config`) and environment overlays (`NEXUS_CFG_*`).
- Primary abuse risks:
  - malformed/deep/oversized config causing parser/resource stress,
  - schema bypass leading to unsafe runtime config states,
  - reload success being reported despite failed consumer prepare/commit paths.

### Security invariants

- Validation is fail-closed: invalid schema/type/path/depth/size must return non-zero and block apply.
- 2PC reload is authoritative: any prepare reject/timeout triggers abort and keeps previous effective version active.
- Config source precedence is deterministic and explicit (`defaults < /system < /state < env`).
- Canonical effective snapshot bytes are deterministic Cap'n Proto (no JSON as authoritative wire/persisted contract).
- Outputs are bounded and deterministic; no secret/token dumps in diagnostics.

### DON'T DO (task-level hard fail)

- Do not allow partial-commit semantics across consumers on reload.
- Do not treat adapter-loaded legacy recipes as an independent second authority.
- Do not claim reload success from markers/log text when 2PC result is abort/reject.
- Do not accept unbounded config trees or silently coerce invalid values.
- Do not promote JSON export/debug views to canonical runtime/persistence authority.

## Contract sources (single source of truth)

- This task defines:
  - authoring/validation schemas under `schemas/`,
  - canonical effective-config snapshot contract under Cap'n Proto schema path,
  - layering rules under `userspace/config/nexus-config`.

## Stop conditions (Definition of Done)

Gate-tier alignment note:

- Per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate J), `TASK-0046` closes against
  **production-floor** expectations (deterministic, honest, no fake success), not production-grade.

### Proof (Host) — required

Add deterministic host tests (`tests/config_v1_host/`):

- layering precedence: defaults < /system < /state < env
- schema validation: invalid type/path → diagnostic includes JSON pointer + expected type
- 2PC orchestration: one mock consumer rejects prepare → configd aborts and effective version unchanged
- diff/effective: `nx config effective --json` output hash matches configd version.
- canonical snapshot determinism: equivalent inputs produce byte-identical Cap'n Proto effective snapshot.
- boundedness rejects: oversize/depth violations fail closed with stable classification (non-zero).
- no fake success: reload reports success only after full prepare+commit path succeeds.

### Proof (OS / QEMU) — gated

Once `/state` exists and at least one service participates:

- `configd: ready`
- `configd: schema loaded (N)`
- `configd: effective v<hex8>`
- `configd: reload start vX→vY`
- `configd: reload commit vY` / `configd: reload abort vY reason=<...>`
- `SELFTEST: config reload allow ok`
- `SELFTEST: config reload abort ok`
- Marker-only evidence is insufficient for closure:
  - each success marker must be paired with deterministic state/result assertions (effective version transition, unchanged version on abort, and non-zero exit on forced failure paths),
  - marker and assertion outcomes must agree; conflicting evidence is a hard fail.

## Quality / gate expectations (Gate J)

- One authoritative CLI path: config UX must live under `nx config ...` (extend `tools/nx`, no `nx-config` logic fork).
- Schema/config surfaces must not drift per subsystem: shared validated model + canonical Cap'n Proto snapshot version hash contract.
- Canonical runtime contract must remain Cap'n Proto; JSON output stays derived (`nx config ... --json`) and non-authoritative.
- Tooling/harness semantics must match runtime proof model:
  - deterministic exit and JSON contracts,
  - reject-path tests as first-class proof,
  - no marker-only closure claims.

## Touched paths (allowlist)

- `schemas/` (new JSON Schemas)
- `tools/nexus-idl/schemas/` (new config snapshot Cap'n Proto schema)
- `userspace/config/nexus-config/` (new crate)
- `source/services/configd/` (new service)
- `tools/nx/` (extend with `nx config ...`)
- `tests/`
- `docs/config/` (new)
- `docs/devx/nx-cli.md` (extend)
- `docs/testing/index.md`
- `scripts/qemu-test.sh` (gated)

## Plan (small PRs)

1. **Schemas (JSON Schema 2020-12)**
   - `schemas/dsoftbus.schema.json`
   - `schemas/metrics.schema.json`
   - `schemas/tracing.schema.json`
   - `schemas/security.sandbox.schema.json`
   - `schemas/sched.schema.json`
   - small shared `$defs` for durations/sizes/CIDR/ports.
   - add Cap'n Proto schema for canonical effective snapshot (runtime/persistence contract).

2. **`nexus-config` library**
   - Load sources (lowest→highest):
     - builtin defaults (embedded)
     - `/system/config/*.toml` (image defaults)
     - `/state/config/*.toml` (overrides)
     - env (`NEXUS_CFG_*`) (strictly scoped)
   - Merge:
     - deep merge objects,
     - lists replace by default (documented).
   - Convert to canonical JSON and validate against schemas.
   - Materialize canonical Cap'n Proto effective snapshot from validated model.
   - Compute `ConfigVersion = sha256(capnp_effective_snapshot_bytes)` (truncate for logs).

3. **`configd` service**
   - API:
     - GetEffective (Cap'n Proto effective snapshot + version)
     - GetEffectiveJson (derived JSON debug/CLI view + same version)
     - Subscribe (push full or delta)
     - Reload (re-evaluate sources and validate)
     - 2PC apply orchestrator:
       - BeginApply(to_ver) → txn
       - Commit/Abort
   - Timeouts:
     - prepare phase bounded (e.g., 2s per service)
     - abort on first reject/timeout.
   - Markers:
     - `configd: ready`
     - `configd: effective v...`
     - `configd: reload commit/abort ...`

4. **Consumer wiring (minimal)**
   - Choose 3–4 services that exist by the time we implement:
     - `dsoftbusd` (transport knobs)
     - `vfsd` (sandbox defaults/quotas later)
     - `timed` (coalescing windows) and/or `policyd` (policy reload)
     - `metricsd/traced` once they exist.
   - Each implements:
     - PrepareConfig(ver, diff) → ok/reject
     - CommitConfig(ver) → ok
   - Must be atomic within the service (swap config pointer).

5. **`nx config`**
   - `nx config validate [PATH...]`
   - `nx config effective --json` (derived view)
   - `nx config diff --from ... --to ...`
   - `nx config push <file>` (writes to `/state/config` where available; host test uses temp dir)
   - `nx config reload` (calls configd)

6. **Docs**
   - `docs/config/index.md`: layering, schemas, 2PC reload, troubleshooting.

## Notes

- `configd` is the canonical place to orchestrate 2PC reload for other subsystems (e.g., Policy as Code in `TASK-0047`).

## Follow-up expectations matrix (contract hand-off)

- `TASK-0047` (Policy as Code v1):
  - expects canonical config distribution + 2PC reload orchestration from `configd`,
  - expects deterministic effective version/hash contract to drive safe policy cutover,
  - expects fail-closed rejects and no partial-commit semantics.
- `TASK-0262` (repo hygiene / determinism cleanup):
  - expects deterministic config proofs to be reproducible in host runs,
  - expects schema/config surfaces consolidated and non-duplicated,
  - expects lint/test gates to reject fake-success patterns.
- `TASK-0266` (authority & naming contract):
  - expects single authority model (`configd` as config authority, no parallel config daemon),
  - expects canonical naming and contract surfaces referenced as SSOT in task/docs.
- `TASK-0268` (`nx` convergence):
  - expects all config UX to remain under `nx config ...` (no `nx-config` logic fork),
  - expects deterministic CLI exit/`--json` behavior aligned with runtime semantics.
- `TASK-0273` (placeholder authority cleanup):
  - expects consumer services to integrate through the canonical `configd` authority path,
  - expects no parallel placeholder authority for config apply/reload semantics.
- `TASK-0285` (QEMU harness phase discipline):
  - expects OS-gated config proofs to be phase-safe and non-fake-success,
  - expects bounded, deterministic failure evidence for reload abort/reject paths.
