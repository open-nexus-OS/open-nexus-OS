---
title: TASK-0238 Installer v1.1a (host-first): NXB format (SemVer, migrations, policy checks) + deterministic tests
status: Draft
owner: @runtime
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - NXB format baseline: tasks/TASK-0129-packages-v1a-nxb-format-signing-pkgr-tool.md
  - Packaging contract (manifest.nxb): tasks/TASK-0007-updates-packaging-v1_1-userspace-ab-skeleton.md
  - Supply-chain signing: tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md
---

## Context

We need to extend the package installation pipeline with:

- SemVer upgrade rules (enforce, allow-downgrade gating),
- schema-driven migrations (idempotent scripts, timeouts),
- policy checks (permissions/abilities/caps validation),
- atomic A/B activation per app (not system-wide).

The prompt may reference alternative bundle format names, but the canonical format is **NXB** with `manifest.nxb` (see `TASK-0129`/`TASK-0007`). This task delivers the **host-first core** (SemVer, migrations, policy checks) for NXB bundles.

## Goal

Deliver on host:

1. **NXB library** (`userspace/libs/nxb/`):
   - reads `manifest.nxb` (canonical) and provides JSON view if needed
   - verifies signature (delegates to `keystored` primitives)
   - extracts to staging (deterministic perms/mtimes)
   - markers: `nxb: verify ok app=… v=…`, `nxb: extract files=n`
2. **SemVer rules**:
   - upgrade: `new_version > current_version` (SemVer comparison)
   - downgrade: denied unless `dev_mode` + `allow_downgrade` enabled
   - deterministic version comparison
3. **Schema-driven migrations**:
   - migration scripts in `migrations/` directory (idempotent, timeout-bounded)
   - schema: `{ "from": "1.1.0", "to": "1.2.0", "script": "migrate/1_1_0__1_2_0.sh" }`
   - max steps: 64, script timeout: 4000ms
   - deterministic execution (injectable time source in tests)
4. **Policy checks** (preflight):
   - ABI/target/min_os validation
   - permissions declared (against `appmanifest` rules)
   - abilities valid (schema check)
   - quota checks (bytes/files against schema limits)
5. **Host tests** proving:
   - SemVer upgrade/downgrade rules work correctly
   - migrations run idempotently and respect timeouts
   - policy checks reject invalid manifests deterministically
   - quota checks enforce limits correctly

## Non-Goals

- New bundle format (use NXB only; no alternative names/formats).
- OS/QEMU markers (deferred to v1.1b).
- Real entitlement checks (deferred to v1.1b; host tests use stubs).

## Constraints / invariants (hard requirements)

- **No format drift**: Do not introduce `manifest.json` + `payload.tar` as a new contract. Use `manifest.nxb` + `payload.elf` (or multi-file payload if NXB contract expands).
- **Single source of truth**: `manifest.nxb` is canonical; JSON views are derived only.
- **Determinism**: SemVer comparison, migration execution, and policy checks must be stable given the same inputs.
- **Bounded resources**: migrations are timeout-bounded; quota checks enforce limits.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (format drift)**:
  - Do not introduce `manifest.json` + `payload.tar` as a new bundle format. Use NXB (`manifest.nxb` + `payload.elf`) only. If the prompt suggests a different layout or name, treat it as documentation/UI naming, not a new on-disk contract.
- **YELLOW (migration determinism)**:
  - Migration scripts must be idempotent and timeout-bounded. Tests must use injectable time sources.

## Contract sources (single source of truth)

- NXB format: `TASK-0129` (manifest.nxb canonical)
- Packaging contract: `TASK-0007` (manifest.nxb direction)
- Supply-chain signing: `TASK-0029`

## Stop conditions (Definition of Done)

### Proof (Host) — required

`cargo test -p installer_v1_1_host` green (new):

- SemVer: upgrade v1.0.0 → v1.1.0 succeeds; downgrade denied unless dev_mode+allow_downgrade
- migrations: script runs idempotently; timeout enforced; max steps respected
- policy checks: invalid ABI/target/permissions/abilities rejected with stable errors
- quota: install rejected if bytes/files exceed limits

## Touched paths (allowlist)

- `userspace/libs/nxb/` (new; NXB reader/verifier/extractor)
- `userspace/libs/semver/` (new; SemVer comparison)
- `userspace/libs/migrations/` (new; migration runner)
- `userspace/libs/pkg-policy/` (new; policy checks)
- `schemas/pkg_v1_1.schema.json` (new)
- `tests/installer_v1_1_host/` (new)
- `docs/pkg/overview.md` (new, host-first sections)

## Plan (small PRs)

1. **NXB library**
   - NXB reader/verifier/extractor
   - JSON view (derived from manifest.nxb)
   - host tests

2. **SemVer + migrations**
   - SemVer comparison rules
   - migration runner (idempotent, timeout-bounded)
   - host tests

3. **Policy checks**
   - ABI/target/min_os validation
   - permissions/abilities schema checks
   - quota enforcement
   - host tests

4. **Schema + docs**
   - `schemas/pkg_v1_1.schema.json`
   - host-first docs

## Acceptance criteria (behavioral)

- SemVer upgrade/downgrade rules work correctly.
- Migrations run idempotently and respect timeouts.
- Policy checks reject invalid manifests deterministically.
- Quota checks enforce limits correctly.
