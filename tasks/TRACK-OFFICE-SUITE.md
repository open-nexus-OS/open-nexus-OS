---
schema: 1
id: TRACK-OFFICE-SUITE
title: "Office Suite (Word + Sheets/BI + Slides): RichContent-first, zero-copy models, autosave-by-default, op-log sync (first-party reference apps)"
status: living
owner: ["@ui"]
created: 2026-01-19
links:
  - label: Vision
    target: docs/architecture/vision.md
  - label: Playbook
    target: CLAUDE.md
  - label: Link
    target: "System Delegation / System Surfaces (Open With, share/export, collaboration entry points): tasks/TRACK-SYSTEM-DELEGATION.md"
  - label: "Zero-Copy App Platform (foundation)"
    target: tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
  - label: "Service architecture (hybrid control/data plane)"
    target: docs/adr/0017-service-architecture.md
  - label: "Richtext widget/app baseline"
    target: tasks/TASK-0098-ui-v15e-richtext-widget-app.md
  - label: "Clipboard v3 (paste inputs + budgets)"
    target: tasks/TASK-0087-ui-v13a-clipboard-v3.md
  - label: "DSL service stubs (svc.*)"
    target: tasks/TASK-0078-dsl-v0_2b-service-stubs-cli-demo.md
  - label: "DSL query objects (Sheets/BI ergonomics)"
    target: tasks/TASK-0274-dsl-v0_2c-db-query-objects-builder-defaults-paging-deterministic.md
  - label: "NexusNet SDK (REST/GraphQL/sync shape)"
    target: tasks/TRACK-NEXUSNET-SDK.md
  - label: "Zero-copy VMOs plumbing"
    target: tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
---

## Goal (track-level)

Deliver a first-party Office suite that proves the ecosystem story:

- **Word**-class rich text (stable paste/import; never corrupt documents),
- **Sheets** that can act as a lightweight **BI tool** (dashboards + animated charts + “pull on open”),
- **Slides** with Canva-like motion (timeline tracks/keyframes/triggers),
- **always-on autosave** (no “did you save?” data loss),
- **collaboration without file overwrites** (op-log sync; git-like audit/blame),
- and a shared core so that “it feels like someone else built it” (consistent UX + primitives).

The suite is a reference implementation of `tasks/TRACK-ZEROCOPY-APP-PLATFORM.md`.

## Product stance (what makes this “better than Excel/Word”)

- **Canonical RichContent** for text/paste/interchange (not fragile DOCX-style formatting blobs).
- **Audit-first** by design (human-readable OpLog, blame-ready stable IDs).
- **Sync-first** by design (operations, not whole-file overwrites).
- **Zero-copy data plane** for bulk content (fast load, large datasets, large decks).
- **Separation of concerns**:
  - data/compute models stay stable and testable,
  - presentation (dashboard/layout/motion) is powerful but bounded and deterministic.

## Shared core (required)

The suite shares a common foundation (library and/or service boundaries):

- **Office core model**: shared Cap’n Proto schemas for:
  - RichContent fragments,
  - workbook (sheets/tables/charts),
  - deck (slides/nodes/timeline),
  - common styling tokens and object IDs.
- **OpLog operations**: shared operation vocabulary across apps:
  - text edits, table ops, chart/layout ops, timeline ops, connector/query ops.
- **Autosave + recovery**: always-on, identical semantics across apps.
- **Clipboard**: RichContent canonical payload with plain/markdown fallbacks.

Note: This track does not mandate a single “monolithic app”; it mandates shared contracts and primitives.

## System Delegation integration (avoid duplicated document UX)

Office apps should lean on system delegation instead of re-implementing platform flows:
- “Open With…” and document selection should use the system picker/associations direction (`TASK-0083`, `TASK-0081`).
- Share/export (PDF/HTML/links) should be expressed as intents (Share v2) so targets (Files/Notes/Chat) can evolve independently.

## App 1: Word (RichText)

### Scope

- Rich text editor (runs, paragraph styles, lists, links, code).
- Paste/import that **never corrupts** the document:
  - HTML/RTF → sanitized subset → RichContent.
- Export:
  - HTML (sanitized),
  - PDF via print pipeline.

### Collaboration + audit

- OpLog captures semantic edits (insert/delete/format) with stable node IDs.
- Blame can answer: “who changed this paragraph/link?”

## App 2: Sheets / BI (script-left, sheet+dashboard-right)

### UX stance

The user edits computation as a **line-based script** (left pane):

- “next line = next instruction”
- deterministic diagnostics with source spans

The grid (right pane) is a view + interactive surface, but not the only place logic lives.

### Data sources / connectors (user plugs “their DB”)

Sheets must support user-provided data backends via the platform connector model:

- REST/HTTP and GraphQL are first-class,
- other “enterprise DBs” are supported via installable providers/services (no kernel work).

Required behavior:

- “pull on open” with explicit refresh policy (bounded time/bytes/rows),
- cache snapshots for offline use,
- audit logs for data refresh events (without logging secrets).

### Query model

- Prefer structured query objects (builder/IR) for safety and policy enforcement.
- A text editor may exist, but must compile to a bounded query IR (no raw unbounded SQL everywhere).
- QuerySpec should be the default contract for connector-backed tables, cached result sets, dashboard panels, and refresh
  state; formulas, document edits, and command actions remain app/domain operations rather than generic queries.

### Formulas and “simple scripting”

Baseline:

- a safe **expression language** for cell formulas (e.g., `B12 = SUM(A17:A22)`),
- deterministic recalculation (dependency graph), bounded compute per tick.

“Scripting” beyond expressions is optional and must be budgeted/sandboxed; IO only via `svc.*`/connector handles.

### Dashboards + animated charts

Sheets includes a dashboard layer:

- panels/widgets bound to tables/series,
- charts animate deterministically on series updates (defined transitions; bounded keyframes),
- layout uses constraints/anchors to avoid “pixel chaos”.

## App 3: Slides (Canva-like motion, PowerPoint-like familiarity)

### Model

- slide scene graph (nodes: text/image/shape/table/chart/group),
- RichContent for text nodes,
- layout (constraints/anchors) + style tokens,
- timeline:
  - tracks per node/property,
  - keyframes with limited easing set,
  - triggers (onEnter, onClick, withPrevious, afterPrevious).

### Determinism + export

- timeline playback is deterministic (same timeline → same frames),
- PDF export via print pipeline; video export is deferred unless bounded and testable.

## Autosave (non-negotiable)

All three apps inherit platform autosave semantics:

- every change produces OpLog entries (or a bounded micro-batch),
- snapshots are taken to accelerate load/recovery,
- crash recovery is deterministic and bounded,
- “Save” is not a required user action; “Export” remains for external formats.

## Collaboration (non-overwrite guarantee)

We do not rely on file locks or “take turns”. Collaboration uses:

- op-log sync and merge rules,
- stable IDs for blame/conflict explanations,
- explicit conflict UX (no silent data loss),
- offline queue + later sync.

Realtime co-edit is an additive layer but must preserve boundedness and determinism.

## Phase map

- **Phase 0 (prove the core primitives)**
  - RichContent clipboard + paste mapping proves “documents don’t break”.
  - OpLog autosave + deterministic recovery proofs.
  - Sheets: script-left editor with diagnostics + expression-only formulas.

- **Phase 1 (dashboards + charts + connectors)**
  - connector picker + query builder + bounded refresh policies.
  - connector-backed table/list/chart views use QuerySpec-backed cache snapshots with explicit ordering/paging contracts.
  - animated charts + dashboard layout constraints.

- **Phase 2 (slides motion + collaboration hardening)**
  - timeline tracks/keyframes/triggers.
  - collaboration UX refinements + audit/blame tooling.

## Candidate subtasks (to be extracted into TASK-XXXX)

- **CAND-OFF-000: Office core schemas (RichContent + workbook + deck) + stable IDs**
- **CAND-OFF-010: Sheets script editor v1 (line-based) + diagnostics**
- **CAND-OFF-020: Formula engine v1 (expression-only) + dependency graph (bounded, deterministic)**
- **CAND-OFF-030: Query bindings v1 (pull-on-open, cache, bounds, audit hooks)**
- **CAND-OFF-040: Dashboard layout v1 (constraints/anchors) + virtualized table**
- **CAND-OFF-050: Charts v1 (animated series updates, deterministic transitions)**
- **CAND-OFF-060: Slides scene graph v1 (nodes/layout/style)**
- **CAND-OFF-070: Slides timeline v1 (tracks/keyframes/triggers; deterministic playback)**
- **CAND-OFF-080: Office collaboration v1 (op-log sync + conflict UX + blame tooling)**

## Extraction rules

A candidate becomes a real `TASK-XXXX` only when it:

- declares bounds (rows/bytes/time, undo depth, timeline keyframe caps),
- defines deterministic ordering and stable IDs,
- includes host-first proof requirements (goldens, replay, recovery, deterministic chart/timeline outputs),
- and documents security invariants (no secrets in logs, capability-gated data access).
