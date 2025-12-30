---
title: TASK-0232 Files/Content v1.2a (host-first): per-app content quotas (bytes+files), deterministic naming rules, simple versions stub, and `nx content`
status: Draft
owner: @platform @devx
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Content provider foundations (contentd pathless streams): tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
  - Scoped content URI grants (grantsd): tasks/TASK-0084-ui-v12a-scoped-uri-grants.md
  - FileOps/Trash backbone: tasks/TASK-0085-ui-v12b-fileops-trash-services.md
  - State quotas substrate (bytes): tasks/TASK-0133-statefs-quotas-v1-accounting-enforcement.md
  - Storage errors contract (EDQUOTA propagation): tasks/TASK-0132-storage-errors-vfs-semantic-contract.md
  - DevX CLI base (`nx`): tasks/TASK-0045-devx-nx-cli-v1.md
---

## Context

The repo already plans the Files/Content backbone as:

- **pathless** document access (`content://` + stream handles) via `contentd` (`TASK-0081`),
- **scoped grants** via `grantsd` (persistable grants under `/state/grants.json`) (`TASK-0084`),
- **trash/restore + progress operations** via `trashd`/`fileopsd` (`TASK-0085`).

This prompt asks for additional v1.2 details that are not fully specified elsewhere:

- per-app quotas including **file count** (not just bytes),
- deterministic filename sanitization rules,
- a simple “versions” stub on overwrite (bounded),
- a CLI surface (`nx content ...`) for deterministic testing and triage.

## Goal

Host-first (deterministic tests) define and prove:

- **Quota model v1.2** for content operations:
  - bytes (reuse `TASK-0133` semantics where possible),
  - files count (new, bounded and deterministic).
- **Naming policy v1.2**:
  - sanitize rules are deterministic and documented,
  - max name length enforced deterministically.
- **Versions stub v1.2**:
  - bounded number of prior versions retained per file,
  - deterministic naming scheme and restore behavior.
- **`nx content`** subcommands (as part of the existing `nx` tool) to exercise the surface.

## Non-Goals

- Introducing a new `content_v1_2.schema.json` as a second source of truth if policy/config already exists elsewhere.
  Any new schema must be explicitly wired into the canonical config/policy system (no “random schema island”).
- Introducing a second grants database:
  - do **not** move grants into `contentd` with libSQL; `grantsd` remains the grants authority (`TASK-0084`).
- Replacing `trashd`/`fileopsd` with “trash inside contentd”. Trash semantics remain consistent with `TASK-0085`.
- Full filesystem versioning or snapshotting (this is only a stub to support a “show versions” UI affordance).

## Constraints / invariants (hard requirements)

- **Determinism**:
  - quota accounting stable (no background races),
  - versions naming stable,
  - CLI output ordering stable.
- **No fake success**:
  - quota errors only emitted when enforcement actually triggers,
  - “versions created” only when a real prior version exists.
- **No paths to apps**:
  - this work must not add any “give me a state path” APIs; keep the `content://` + stream model.

## Red flags / decision points

- **RED (quota authority)**:
  - Bytes quotas for state-backed storage are already tracked in `TASK-0133`.
    This task must either:
    - reuse the same enforcement point, or
    - explicitly document why content-layer quotas are needed in addition (avoid double-enforcement drift).
- **YELLOW (file count quotas)**:
  - Counting files is expensive without an index; v1.2 must choose a bounded, deterministic approach
    (e.g., counter maintained at create/delete boundaries only; no background crawls).
- **YELLOW (versions stub location)**:
  - Decide whether versions are implemented by:
    - `fileopsd` (as part of copy/overwrite operations), or
    - a provider-specific behavior in `contentd`’s state provider.
  - Avoid implementing version behavior in multiple layers.

## Stop conditions (Definition of Done)

### Proof (Host) — required

New deterministic tests (suggested: `tests/content_v1_2_host/`):

- quotas:
  - exceeding bytes quota denies with `EDQUOTA` deterministically (align with `TASK-0133`)
  - exceeding files quota denies with a stable error (must be documented; preferably `EDQUOTA`)
- naming:
  - sanitization transforms a set of fixture names deterministically (golden)
- versions:
  - overwrite produces `.v1/.v2/...` (or equivalent) deterministically up to max N
  - restore order stable
- CLI:
  - `nx content stat/ls/grant/usage/...` output stable against fixtures

### Proof (OS/QEMU) — gated

Only after `contentd`/`grantsd`/`fileopsd` exist on OS:

- Markers in `scripts/qemu-test.sh` expected list:
  - `SELFTEST: content quota ok`
  - `SELFTEST: content versions ok`

## `nx content` CLI scope (v1.2)

As `nx content ...` (not a standalone binary), minimum commands:

- `nx content stat <uri>`
- `nx content ls <uri>`
- `nx content create <parent_uri> <name> --mime <mime>`
- `nx content rm <uri>` (must follow trash semantics if enabled)
- `nx content mv <src> <dst_parent> --name <new>`
- `nx content grants --app <id>`
- `nx content grant --app <id> --uri <uri> --mode <r|w|rw> [--persist]`
- `nx content usage --app <id>`

## Touched paths (allowlist)

- `tools/nx/` (add `content` subcommand)
- `tests/content_v1_2_host/` (new)
- `docs/platform/content.md` and/or `docs/content/*` (document quotas/naming/versions)
- enforcement points per existing tasks (`contentd`, `fileopsd`, `trashd`, `statefsd`) only as needed

## Plan (small PRs)

1. Specify v1.2 naming + versions rules (fixtures + goldens).
2. Specify file-count quota semantics and error mapping.
3. Add host tests + `nx content` scaffolding.
4. Gate OS selftests only once real services exist; add markers then.

## Acceptance criteria (behavioral)

- Quotas, naming, and versions behavior are deterministic and bounded, and the CLI enables reproducible verification without introducing a new authority or schema drift.
