---
title: TASK-0046 Config & Schema v1: configd broker + JSON Schema validation + deterministic layering + 2PC reload (+ nx config)
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - DevX CLI: tasks/TASK-0045-devx-nx-cli-v1.md
  - Policy as Code (consumer): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Persistence substrate: tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Audit sink (optional): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - DSoftBus hardening (future consumer): tasks/TASK-0030-dsoftbus-discovery-authz-hardening-mdns-ttl-acl-ratelimit.md
  - Metrics gatekeeping (future consumer): tasks/TASK-0014-observability-v2-metrics-tracing.md
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
- `/state` persistence is planned (TASK-0009), so OS proofs must be gated.

This task is **host-first** and **OS-gated**.

## Goal

Deliver:

1. Versioned JSON Schemas for a small set of subsystems (dsoftbus, metrics, tracing, sandbox/security, sched).
2. A `nexus-config` library: layered loader + schema validator + canonical version hash.
3. A `configd` service: Get/Subscribe + Reload + 2PC apply orchestration with versioning and timeouts.
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
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success: reload markers only after validation + (if enabled) successful 2PC commit.

## Red flags / decision points

- **RED (OS gating)**:
  - OS persistence is required for `/state/config` overrides and durable config versions (TASK-0009).
  - Until then, OS proofs must either:
    - be RAM-only and explicitly labeled non-persistent, or
    - be host-only.
- **YELLOW (schema vs TOML sources)**:
  - If sources are TOML but schemas are JSON Schema, we must define a canonical intermediate representation
    (canonical JSON tree) and validate that.
- **YELLOW (legacy recipes mapping)**:
  - Supporting legacy `recipes/*` ingestion is useful, but must be explicitly labeled as a migration adapter
    to avoid two sources of truth.

## Contract sources (single source of truth)

- This task defines schemas under `schemas/` and config layering rules under `userspace/config/nexus-config`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

Add deterministic host tests (`tests/config_v1_host/`):

- layering precedence: defaults < /system < /state < env
- schema validation: invalid type/path → diagnostic includes JSON pointer + expected type
- 2PC orchestration: one mock consumer rejects prepare → configd aborts and effective version unchanged
- diff/effective: `nx config effective --json` output hash matches configd version.

### Proof (OS / QEMU) — gated

Once `/state` exists and at least one service participates:

- `configd: ready`
- `configd: schema loaded (N)`
- `configd: effective v<hex8>`
- `configd: reload start vX→vY`
- `configd: reload commit vY` / `configd: reload abort vY reason=<...>`
- `SELFTEST: config reload allow ok`
- `SELFTEST: config reload abort ok`

## Touched paths (allowlist)

- `schemas/` (new JSON Schemas)
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
   - Compute `ConfigVersion = sha256(canonical_json)` (truncate for logs).

3. **`configd` service**
   - API:
     - GetEffective (json + version)
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
   - `nx config effective --json`
   - `nx config diff --from ... --to ...`
   - `nx config push <file>` (writes to `/state/config` where available; host test uses temp dir)
   - `nx config reload` (calls configd)

6. **Docs**
   - `docs/config/index.md`: layering, schemas, 2PC reload, troubleshooting.

## Notes

- `configd` is the canonical place to orchestrate 2PC reload for other subsystems (e.g., Policy as Code in `TASK-0047`).
