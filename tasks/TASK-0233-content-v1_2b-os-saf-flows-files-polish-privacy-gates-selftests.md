---
title: TASK-0233 Files/Content v1.2b (OS/QEMU): SAF picker flows (open/save/folder) + persistable grants UX + Files app polish + privacy/files gates + selftests
status: Draft
owner: @ui @platform
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Content providers (contentd): tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
  - Document picker (open/save/open-with): tasks/TASK-0083-ui-v11c-document-picker-open-save-openwith.md
  - Scoped grants (persistable): tasks/TASK-0084-ui-v12a-scoped-uri-grants.md
  - FileOps/Trash services: tasks/TASK-0085-ui-v12b-fileops-trash-services.md
  - Files app baseline: tasks/TASK-0086-ui-v12c-files-app-progress-dnd-share-openwith.md
  - Recents + thumbnails: tasks/TASK-0082-ui-v11b-thumbnailer-recents.md
  - Policy capability matrix / privacy categories: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - permsd/privacyd wiring (runtime consent + indicators): tasks/TASK-0103-ui-v17a-permissions-privacyd.md
  - /state (persistence): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Removable storage track (provider + SAF folder grants + fileops): tasks/TRACK-REMOVABLE-STORAGE.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

The repo already planned the core UX building blocks:

- document picker open/save (`TASK-0083`),
- persistable scoped grants (`TASK-0084`),
- file operations + trash (`TASK-0085`),
- Files app surface (`TASK-0086`).

This prompt’s “v1.2” deltas are:

- “SAF” (Storage Access Framework) naming and **folder access** flow,
- explicit “remember access” UX for persistable grants,
- Files app polish items (Home/Recent/Downloads/Pictures/Trash; info pane),
- explicit privacy/files gating (consent + capability) for any read/write grants.

## Goal

On OS/QEMU, deliver deterministic, proven UX flows:

- **SAF picker** overlay supports:
  - Open, Save As, Select Folder
  - MIME filter + search + breadcrumb navigation
  - “Remember access” checkbox → persistable grant (through `grantsd`, not contentd)
- **Files app polish**:
  - curated roots (Home/Recent/Downloads/Pictures/App Data/Trash) as views over existing providers
  - deterministic bulk operations progress
  - info pane includes: URI, MIME, size, modified, and active grants (bounded)
- **Privacy/files gate**:
  - any cross-subject access requires:
    - policy capability (`content.*` / `files` category as defined by the policy/capability matrix),
    - and runtime consent where applicable (`permsd`), with deterministic deny reasons.

## Non-Goals

- Renaming `docpicker`/`grantsd`/`contentd` services into “SAF*” daemons. SAF is a UX/flow label only.
- Introducing a new URI scheme. We keep `content://...` as defined by `TASK-0081`.
- Implementing “cloud providers” beyond deterministic stubs.
- Full version history UI beyond listing the stub versions created by `TASK-0232` (if/when present).
- Treating removable storage as “global mounts” for apps. Removable media must be surfaced as a `contentd` provider and accessed via SAF/grants (see `tasks/TRACK-REMOVABLE-STORAGE.md`).

## Constraints / invariants (hard requirements)

- **No fake success**: UI markers only after real grants are issued and validated.
- **Deterministic picker results** for fixtures:
  - stable listing order,
  - stable generated default filenames for Save As (fixtures),
  - bounded search results.
- **Bounded grants**: cap count and size; persistable grants must be stored under `/state` via the existing plan (`TASK-0084`).
- **Authority clarity**:
  - `grantsd` is the issuer/authority for scoped grants (`TASK-0084`),
  - `contentd` enforces grants at `openUri` boundaries (`TASK-0084`),
  - policy decisions remain in `policyd` (`TASK-0136`), and runtime consent in `permsd` (`TASK-0103`).

## Red flags / decision points

- **RED (gating dependencies)**:
  - Persistable grants require `/state` (`TASK-0009`). Without it, “remember access” must be disabled and explicitly marked.
  - Privacy/files consent requires `permsd` wiring (`TASK-0103`) and policy adapters (`TASK-0136`).
- **YELLOW (folder grants semantics)**:
  - Define whether a folder grant expands to:
    - subtree allowlist (prefix semantics), or
    - enumerated docIds only.
  - Must be deterministic and enforced by `contentd` using `grantsd` validation (avoid a second rule path).

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — required (once deps exist)

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=185s ./scripts/qemu-test.sh`
- Required markers:
  - `SELFTEST: content saf grant ok`
  - `SELFTEST: content quota ok` (only if quotas are truly enforced; ties to `TASK-0232`/`TASK-0133`)
  - `SELFTEST: content trash restore ok` (ties to `TASK-0085`)

### Proof (Host) — suggested

- Extend existing UI model tests (v11c/v12c) with:
  - folder picker mode model,
  - remember-access toggle → persist grant request shape (mocked).

## Touched paths (allowlist)

- SystemUI doc picker overlay (`TASK-0083`) (extend: folder mode + remember access UX)
- `source/apps/selftest-client/` (new markers)
- `userspace/apps/files/` (polish items; still uses providers/grants/fileops)
- `docs/dev/ui/doc-picker.md` + `docs/dev/ui/files.md` (extend with SAF terminology + privacy gate explanation)
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. Extend doc picker model/UI with folder mode + remember-access.
2. Wire remember-access to persistable grants via `grantsd` (no new DB).
3. Add Files app polish views (Home/Downloads/Pictures/Trash/Info pane) as deterministic projections.
4. Add selftests + markers only after the underlying enforcement is real.

## Acceptance criteria (behavioral)

- SAF flows are deterministic and QEMU-proof, persistable grants are real (not stubs), Files app polish is consistent with the pathless content model, and privacy/files gates are enforced without introducing new authorities or URI schemes.
