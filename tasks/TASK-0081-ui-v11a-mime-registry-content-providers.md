---
title: TASK-0081 UI v11a: MIME registry (mimed) + content provider API (contentd) with stream handles (no paths)
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - ADR: docs/adr/0022-modern-image-formats-avif-webp.md
  - VFS substrate: tasks/TASK-0002-userspace-vfs-proof.md
  - Persistence (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Removable storage track (USB/SD/external disks as provider): tasks/TRACK-REMOVABLE-STORAGE.md
  - Policy as Code: tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Config broker: tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - UI v6b app launch (Open With later): tasks/TASK-0065-ui-v6b-app-lifecycle-notifications-navigation.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

UI v11 introduces “Document Access”. Foundations must be **pathless** and capability-based:

- apps receive **stream handles**, not filesystem paths,
- URIs are stable identifiers (`content://...`) resolved by a broker,
- MIME type registry provides extension mapping and “Open With…” associations.

Thumbnailing, recents, and the picker UI are split into v11b/v11c.

## Goal

Deliver:

1. `mimed` service (MIME registry + associations):
   - ext → mime
     - include at least: `.png`, `.jpg/.jpeg`, `.webp`, `.avif`, `.svg`, `.txt`
   - mime → assoc (apps list + default app)
   - register app supported mimes
   - set default app per mime
   - markers:
     - `mimed: ready`
     - `mimed: default set (mime=..., app=...)`
2. `contentd` service (content provider broker):
   - `content://<provider>/<docId>` URI scheme
   - `Content.resolve/openUri/query`
   - providers v11a:
     - `state` (sandboxed per app id)
     - `pkg` (read-only)
     - `mem` (ephemeral demo)
     - `demo-cloud` (stubbed, gated by policy/config; deterministic streams)
   - `Provider.list/stat/open/create/remove/rename` returning stream handle caps (CapFd)
   - markers:
     - `contentd: ready`
     - `content: provider up (name=state|pkg|mem|demo-cloud)`
3. Host tests for MIME and content providers.

## Non-Goals

- Kernel changes.
- UI document picker and "Open With…" UI (v11c).
- Thumbnail generation (v11b).
- Full cloud integration (demo-cloud is a deterministic stub only).
- Durable write semantics (atomic create/replace, temp+commit, fsync barriers) or crash-recovery (handled by `TASK-0264`/`TASK-0265` as an extension; this task focuses on content provider API with stream handles).
- Defining removable storage as “mounted filesystem paths” for apps. Removable media must appear as a `contentd` provider with stream handles (see `tasks/TRACK-REMOVABLE-STORAGE.md`).

## Constraints / invariants (hard requirements)

- **No paths** exposed to apps for document access (only `content://` URIs and stream handles).
- Deterministic provider behavior for tests (especially demo-cloud).
- Policy guardrails:
  - sandbox `state:/` per-app subtree,
  - `pkg://` is read-only,
  - `demo-cloud://` off by default except tests.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v11a_host/`:

- MIME:
  - ext→mime query
  - mime→assoc query
  - setDefault persists (in-memory for host test) and returns deterministic results
- Content providers:
  - `state:/` create/list/open/read/write roundtrip (mocked backing store)
  - `pkg://` rejects writes
  - `mem://` is ephemeral (restart loses entries)
  - `demo-cloud://` gated and deterministic

### Proof (OS/QEMU) — gated

UART markers:

- `mimed: ready`
- `contentd: ready`
- `content: provider up (name=state)`
- `content: provider up (name=pkg)`

## Touched paths (allowlist)

- `source/services/mimed/` (new)
- `source/services/contentd/` (new)
- `schemas/` + `policies/` (provider enable flags + sandbox policy)
- `tests/ui_v11a_host/`
- `docs/platform/content.md` (new)

## Plan (small PRs)

1. mimed IDL + in-memory registry + markers + host tests
2. contentd IDL + provider implementations + policy gating + markers
3. docs + postflight wiring (v11c owns full postflight)
