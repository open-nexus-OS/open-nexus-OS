---
schema: 1
id: TASK-0273
title: "Repo v1: replace placeholders with canonical authorities (no parallel daemons)"
status: draft
owner: ["@runtime"]
created: 2025-12-30
subsystem: devx
links:
  - label: Vision
    target: docs/architecture/vision.md
  - label: Playbook
    target: CLAUDE.md
  - label: "Authority & naming registry"
    target: tasks/TRACK-AUTHORITY-NAMING.md
  - label: "Keystone gates"
    target: tasks/TRACK-KEYSTONE-GATES.md
  - label: "Authority decision (binding)"
    target: tasks/TASK-0266-architecture-v1-authority-naming-contract.md
---

## Context

Repo reality contains placeholder service crates with non-canonical names. Planning has now decided the canonical
authorities and naming rules. To keep the system coherent, we must ensure there are **no parallel daemons**
claiming the same responsibility.

This task is a repo-consolidation pass: replace/rename/remove placeholders so only canonical authorities remain.

## Goal

Perform a controlled consolidation so that:

- the runtime service set matches `TRACK-AUTHORITY-NAMING.md`,
- readiness markers match canonical names,
- there is exactly one authority per domain.

## Replacement matrix (binding)

- `source/services/powermgr/` → `source/services/powerd/` (canonical: `powerd`)
- `source/services/batterymgr/` → `source/services/batteryd/` (canonical: `batteryd`)
- `source/services/thermalmgr/` → `source/services/thermald/` (canonical: `thermald`)
- `source/services/ime/` → `source/services/imed/` (canonical: `imed`)
- `source/services/abilitymgr/` → `source/services/appmgrd/` (canonical: `appmgrd`)
- `source/services/compositor/` → removed; canonical compositor authority is `windowd`

## Non-Goals

- Implementing full feature behavior for each subsystem (that lives in the subsystem tasks).
- Keeping backward compatibility for placeholder names (planning/bring-up does not promise this).

## Stop conditions (Definition of Done)

- No placeholder service names appear in:
  - UART readiness markers,
  - `samgrd` registration identities,
  - task documents as canonical authorities.
- Canonical readiness markers are emitted instead:
  - `powerd: ready`, `batteryd: ready`, `thermald: ready`, `imed: ready`, `appmgrd: ready`, `windowd: ready`.

## Touched paths (allowlist)

- `source/services/**` (rename/replace/remove placeholder crates)
- `docs/**` (update any references)
- `tasks/**` (text-only corrections where needed)
