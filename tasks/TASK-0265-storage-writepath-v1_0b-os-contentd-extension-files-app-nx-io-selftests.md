---
title: TASK-0265 Storage Write-Path v1.0b (OS/QEMU): contentd extension + Files app tweaks + `nx io` + selftests
status: Draft
owner: @platform
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Authority & naming registry: tasks/TRACK-AUTHORITY-NAMING.md
  - Storage core (host-first): tasks/TASK-0264-storage-writepath-v1_0a-host-durable-io-atomic-fsync-deterministic.md
  - Content provider foundations: tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
  - Content quotas: tasks/TASK-0232-content-v1_2a-host-content-quotas-versions-naming-nx-content.md
  - Files app: tasks/TASK-0086-ui-v12c-files-app-progress-dnd-share-openwith.md
  - Persistence: tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need OS/QEMU integration for Storage Write-Path v1.0:

- `contentd` extension (durable write APIs),
- Files app tweaks (temp+commit for Save operations),
- `nx io` CLI.

The prompt proposes these extensions. `TASK-0081` already plans `contentd` service with stream handles. `TASK-0232` already plans content quotas. This task delivers the **OS/QEMU integration** with `contentd` extension, Files app tweaks, and `nx io` CLI, complementing the host-first durable I/O work.

## Goal

On OS/QEMU:

1. **Schema hardening**:
   - add/extend `schemas/content_v1_3.schema.json`: `temp_suffix` (`.nxpart`), `atomic_replace` (true), `fsync_barriers` (true), `crash_sim` (enable, points), `quota` (per_app_bytes, per_app_files, temp_ratio_pct)
   - marker: `policy: content v1.3 writepath enforced`
2. **contentd extension** (`source/services/contentd/`):
   - extend `content.capnp` (new ops): `create2(parent, name, opts:CreateOpts) -> uri`, `write(uri, off, data) -> n`, `fsync(uri)`, `commit(tempUri, finalParent, finalName, replace) -> uri`, `unlink(uri)`, `dirsync(uri)` (using libraries from `TASK-0264`)
   - use **write-ahead temp-then-commit**: `open(O_CREAT|O_EXCL)`, write, **fsync(file)**, rename, **fsync(parent)**
   - `replace=true` path must use atomic rename over same filesystem; keep old file on crash
   - enforce **temp budget** and total **quota** at write time; deny when over budget
   - maintain **journaling stub** in `state:/content/journal/` that records `prepare/commit/done` entries for recovery (gated on `/state`)
   - on startup, scan `state:/content/journal/` and: remove orphaned `*.nxpart` older than N minutes; finalize **committed but undirsync'd** entries (rename+dirsync)
   - bounded markers: `contentd: create temp uri=…`, `contentd: fsync ok`, `contentd: commit replace ok`, `contentd: quota temp exceeded`, `contentd: recovery sweep orphans=N finalized=M`
3. **Files app & SAF tweaks**:
   - use **temp+commit** for Save operations; progress UI shows "Flushing…" between `fsync` and `dirsync`
   - on replace, show conflict dialog if target is directory or permissions mismatch
   - marker: `files: save atomic ok`
4. **CLI diagnostics** (`nx io ...` as a subcommand of the canonical `nx` tool):
   - `nx io put content://local/Documents/file.txt --from ./README.md --atomic`, `nx io tempput content://local/Documents/new.txt --from ./x --commit`, `nx io cat content://local/Documents/file.txt`, `nx io quota --app com.example.notes`, `nx io crashsim --point after_fsync --runs 3`
   - markers: `nx: io put atomic ok`, `nx: io crashsim point=after_fsync run=2 ok`
5. **OS selftests + postflight**.

## Non-Goals

- Kernel changes.
- Real hardware (QEMU/virtio-blk only).
- Full filesystem journaling (this is a stub for recovery pass only).

## Constraints / invariants (hard requirements)

- **No duplicate content authority**: `contentd` is the single authority for content operations. Do not create parallel content services.
- **No duplicate quota authority**: `contentd` enforces quotas at write time. `TASK-0232` (content quotas) should share the same quota enforcement to avoid drift.
- **Determinism**: durable I/O, atomic operations, fsync barriers, and crash-recovery must be stable given the same inputs.
- **Bounded resources**: temp budget is bounded; quota enforcement is bounded.
- **Persistence gating**: journaling requires `/state` (`TASK-0009`) or equivalent. Without `/state`, journaling must be disabled or explicit `stub/placeholder` (no "written ok" claims).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (content authority drift)**:
  - Do not create a parallel content service. `contentd` is the single authority for content operations. This task extends `contentd` with durable write semantics.
- **RED (quota authority drift)**:
  - Do not create parallel quota enforcement. `contentd` enforces quotas at write time. `TASK-0232` (content quotas) should share the same quota enforcement to avoid drift.
- **YELLOW (atomic replace determinism)**:
  - Atomic replace must use atomic rename over same filesystem; keep old file on crash. Document the filesystem requirements explicitly.

## Contract sources (single source of truth)

- QEMU marker contract: `scripts/qemu-test.sh`
- Storage core: `TASK-0264`
- Content provider foundations: `TASK-0081` (contentd service)
- Content quotas: `TASK-0232` (content quotas)
- Files app: `TASK-0086` (Files app)
- Persistence: `TASK-0009` (prerequisite for `/state`)

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — gated

UART markers:

- `policy: content v1.3 writepath enforced`
- `contentd: create temp uri=…`
- `contentd: fsync ok`
- `contentd: commit replace ok`
- `contentd: quota temp exceeded`
- `contentd: recovery sweep orphans=N finalized=M`
- `files: save atomic ok`
- `nx: io put atomic ok`
- `nx: io crashsim point=after_fsync run=2 ok`
- `SELFTEST: io atomic commit ok`
- `SELFTEST: io atomic replace ok`
- `SELFTEST: io crashsim ok`
- `SELFTEST: io quotas ok`

## Touched paths (allowlist)

- `schemas/content_v1_3.schema.json` (new)
- `source/services/contentd/` (extend: durable write APIs, crash recovery)
- `source/services/policyd/` (extend: content v1.3 writepath enforcement)
- `userspace/apps/files/` (extend: temp+commit for Save operations)
- `tools/nx/` (extend: `nx io ...` subcommands; no separate `nx-io` binary)
- `source/apps/selftest-client/` (markers)
- `docs/storage/writepath_v1_0.md` (new)
- `docs/tools/nx-io.md` (new)
- `docs/content/overview.md` (extend: new methods and semantics, recovery sweep)
- `tools/postflight-storage-writepath-v1_0.sh` (new)

## Plan (small PRs)

1. **Schema + contentd extension**
   - content_v1_3.schema.json
   - contentd durable write APIs (using libraries from `TASK-0264`)
   - crash recovery pass
   - markers

2. **Files app + CLI**
   - Files app temp+commit for Save
   - nx-io CLI
   - markers

3. **OS selftests + postflight**
   - OS selftests
   - postflight

## Acceptance criteria (behavioral)

- `contentd` durable write APIs work correctly (atomic create/replace, temp+commit, fsync barriers).
- Crash recovery pass removes orphaned temp files and finalizes committed entries correctly.
- Files app uses temp+commit for Save operations correctly.
- All four OS selftest markers are emitted.
