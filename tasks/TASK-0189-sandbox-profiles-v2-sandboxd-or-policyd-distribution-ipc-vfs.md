---
title: TASK-0189 Sandbox profiles v2 (userspace): profile format + distribution (sandboxd or policyd) + samgr IPC allowlist + vfsd path policy + tests/markers
status: Draft
owner: @security
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Sandboxing v1 (namespaces/CapFd): tasks/TASK-0039-sandboxing-v1-vfs-namespaces-capfd-manifest.md
  - ABI filters v2 (guardrails): tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md
  - ABI filters v2 arg matching (optional): tasks/TASK-0028-abi-filters-v2-arg-match-learn-enforce.md
  - Policy authority (single source): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Policy caps/adapters: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We want per-process sandbox profiles that constrain:

- syscall surface (best-effort in v2 via `nexus-abi` filters; true enforcement via `TASK-0188`),
- IPC service connections (samgr allowlist),
- VFS path/URI prefixes (vfsd allowlist),
- and basic rate/limit hints (bounded; mostly audit in early steps).

We must avoid introducing competing authorities:

- Prefer `policyd` as the profile server (single authority) unless a dedicated `sandboxd` is clearly justified.

## Goal

Deliver:

1. Profile schema (authoring format):
   - YAML under `pkg://sandbox/profiles/*.yaml` (seeded defaults)
   - optional overrides under `state:/sandbox/profiles/*.yaml` when `/state` exists
   - profile fields:
     - sys: rules + optional rate buckets (v2 guardrail only unless kernel sysfilter exists)
     - ipc: allowlist patterns (`service` and optional scope strings)
    - vfs: read/write URI prefixes (`pkg://`, `state:/apps/<id>/...`)
     - limits: rss/fds (audit-only until enforceable)
2. Distribution service decision:
   - **Option A (preferred):** `policyd` serves sandbox profiles (avoid new service)
   - **Option B:** `sandboxd` serves profiles only if policyd coupling is undesirable; must be justified
3. samgr IPC allowlist enforcement:
   - consult active profile for caller pid/service_id
   - deny capability grants/endpoint connections not in allowlist
   - stable deny errors and audit markers:
     - `samgr: ipc deny pid=<..> svc=<..>`
4. vfsd path policy (prefix allowlists):
   - enforce read/write allowlists on URIs
   - default deny cross-app state prefixes
   - stable deny markers:
     - `vfsd: deny pid=<..> op=<read|write> uri=<...>`
5. Deterministic host tests:
   - profile parsing/matching precedence and stable errors
   - IPC allowlist deny/allow cases
   - VFS prefix deny/allow cases
6. OS/QEMU selftests:
   - deny forbidden IPC
   - deny forbidden VFS write
   - markers:
     - `SELFTEST: sandbox ipc deny ok`
     - `SELFTEST: sandbox vfs ok`

## Non-Goals

- Kernel changes in this task (kernel sysfilter is `TASK-0188`).
- Claiming hard RSS/FDS enforcement without kernel support (audit-only is OK but must be explicit).

## Constraints / invariants (hard requirements)

- Deterministic matching rules and bounded tables.
- No fake security: v2 syscall rules are guardrails only unless kernel sysfilter exists.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (authority drift)**:
  - Do not ship `sandboxd` + `policyd` as competing authorities. Pick one owner for profiles and document it.

- **YELLOW (identity binding)**:
  - Profile application must be keyed by kernel-derived identity (`sender_service_id` / `service_id`), not spoofable strings.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p sandbox_profiles_v2_host -- --nocapture`

- **Proof (QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=210s ./scripts/qemu-test.sh`
  - Required markers:
    - `SELFTEST: sandbox ipc deny ok`
    - `SELFTEST: sandbox vfs ok`

## Touched paths (allowlist)

- `source/services/policyd/` and/or `source/services/sandboxd/`
- `source/services/samgrd/` (IPC allowlist enforcement)
- `source/services/vfsd/` (path policy enforcement)
- `recipes/policy/` or `pkg://sandbox/profiles/` (profile seeds)
- `tests/`
- `source/apps/selftest-client/`
- `docs/security/sandbox-profiles.md`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. Define profile schema + seeded profiles + host tests
2. Implement samgr IPC allowlist enforcement + host tests
3. Implement vfsd path prefix enforcement + host tests
4. OS selftests + docs + marker contract update

## Acceptance criteria (behavioral)

- Host tests prove deterministic allow/deny decisions for IPC and VFS; OS selftests prove denies with stable markers.
