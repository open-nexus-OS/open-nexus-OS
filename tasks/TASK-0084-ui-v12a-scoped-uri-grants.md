---
title: TASK-0084 UI v12a: scoped content:// URI grants (grantsd) + contentd enforcement (time-bound, persistable)
status: Draft
owner: @security
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Content providers (contentd): tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
  - Policy as Code: tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Persistence (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Removable storage track (provider + SAF grants): tasks/TRACK-REMOVABLE-STORAGE.md
  - Testing contract: scripts/qemu-test.sh
  - Data formats rubric (JSON vs Cap'n Proto): docs/adr/0021-structured-data-formats-json-vs-capnp.md
---

## Context

UI v11 introduced `content://` URIs and stream-handle document access. UI v12 requires **scoped grants**
so apps can open URIs without being granted broad filesystem rights.

The security boundary is userspace:

- `contentd.openUri` must validate a grant when the caller is not the owner of the target subtree/provider.
- grants must be time-bound and optionally persistable (user-intent style).

This is v12a (grants + enforcement). File operations/trash and the Files app are v12b/v12c.

Terminology note:

- Some prompts refer to these flows as “SAF” (Storage Access Framework). In this repo, SAF is a **UX flow label**
  over `docpicker` + `grantsd` + `contentd` enforcement; we do not introduce a separate “saf*” daemon.

## Goal

Deliver:

1. `grantsd` service:
   - issues opaque grant tokens for `content://` URIs (read/write modes)
   - TTL for non-persist grants; persistable grants stored under `/state/grants.nxs` (Cap'n Proto snapshot; canonical)
     - optional derived/debug view: `nx grants export --json` emits deterministic JSON
   - regrant (handoff token to another subject) and revoke
   - markers:
     - `grantsd: ready`
     - `grants: grant (subject=..., uri=..., modes=...)`
     - `grants: revoke (token=...)`
2. `contentd` enforcement:
   - accept optional grant token for `openUri`
   - call `grantsd.validate(token, uri, mode)` when access crosses sandbox boundaries
   - deny-by-default if no valid grant is present for cross-subject access
3. Host tests proving correctness deterministically.

## Non-Goals

- Kernel changes.
- Files app UI (v12c).
- File operations manager and trash (v12b).
- A full cryptographic token format (opaque tokens are acceptable v1; can be upgraded later under Policy-as-Code).
- Granting ambient access to removable media (USB/SD/external disks). Removable access must be mediated via `content://` URIs + scoped grants (see `tasks/TRACK-REMOVABLE-STORAGE.md`).

## Constraints / invariants (hard requirements)

- Default deny for cross-subject access without a valid token.
- Deterministic grant validation results and stable deny reasons.
- Bounded state:
  - cap number of stored grants per subject,
  - cap token length and URI length.
- Authority clarity:
  - `grantsd` issues and persists grants; `contentd` enforces at open boundaries.
  - Do not add a second grants DB inside `contentd`.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v12a_host/`:

- grant read token allows another subject to open a URI read-only
- write without write mode is denied
- TTL expiry denies deterministically
- persisted grants survive restart (host-simulated) and can be revoked

### Proof (OS/QEMU) — gated

UART markers:

- `grantsd: ready`

## Touched paths (allowlist)

- `source/services/grantsd/` (new)
- `source/services/contentd/` (extend: enforcement)
- `tests/ui_v12a_host/`
- `docs/platform/grants.md` (new)

## Plan (small PRs)

1. grantsd IDL + in-memory core + markers
2. persistence + TTL enforcement + host tests
3. contentd openUri enforcement + docs
