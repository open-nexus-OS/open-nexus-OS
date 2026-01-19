---
title: TRACK Zero-Copy App Platform (apps): RichContent + OpLog autosave + sync + connectors + UI primitives (deterministic, policy-gated)
status: Living
owner: @runtime @ui @storage
created: 2026-01-19
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Service architecture (hybrid control/data plane): docs/adr/0017-service-architecture.md
  - IPC runtime architecture: docs/adr/0003-ipc-runtime-architecture.md
  - Zero-copy VMOs plumbing: tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - DSL service stubs (svc.*): tasks/TASK-0078-dsl-v0_2b-service-stubs-cli-demo.md
  - Clipboard v3 (budgets + negotiation): tasks/TASK-0087-ui-v13a-clipboard-v3.md
  - DSL query objects (QuerySpec IR): tasks/TASK-0274-dsl-v0_2c-db-query-objects-builder-defaults-paging-deterministic.md
  - NexusNet SDK (REST/GraphQL/sync shape): tasks/TRACK-NEXUSNET-SDK.md
  - Richtext widget/app baseline: tasks/TASK-0098-ui-v15e-richtext-widget-app.md
  - NexusFrame (reference creative app consumer): tasks/TRACK-NEXUSFRAME.md
---

## Goal (track-level)

Deliver a first-party **Zero-Copy App Platform** that lets third-party developers build **beautiful, fast, safe apps**
(office, creative, BI dashboards, CAD-lite, note-taking) **without custom OS code**, while staying aligned with Open Nexus OS:

- **capability-first security** (no ambient authority; policy-gated operations),
- **hybrid control/data plane** (small typed IPC + bulk via VMO/filebuffer),
- **deterministic behavior** (host-first proofs; stable markers only after real behavior),
- **bounded resource usage** (sizes, rows, bytes, timeouts; no unbounded parsing/execution),
- **auditability** (git-like history + blame semantics; business-friendly change tracking),
- **crash safety** (autosave-by-default; recovery is deterministic).

## Scope boundaries (anti-drift)

- This is **not** “write a full general-purpose programming language” in the DSL.
- This is **not** “put databases in the kernel”.
- This is **not** “make DOCX/XLSX/ODF the source of truth”.
- We do **not** allow hidden, unbounded background work (endless retries, infinite sync loops, unbounded parsing).

## Architecture stance (single sources of truth)

### Control plane vs data plane (required)

- Control plane: small structured messages (Cap’n Proto on host/std; os-lite frames on OS bring-up).
- Data plane: bulk buffers via **VMO/filebuffer**.
- Reference: `docs/adr/0017-service-architecture.md`.

### Canonical rich content (clipboard + inter-app interchange)

We adopt a canonical **RichContent AST** (not “Markdown-only”) for:

- cross-app copy/paste (Word ↔ Sheets ↔ Slides ↔ third-party apps),
- deterministic import pipelines (HTML/RTF sanitized subset → RichContent),
- stable export surfaces (Markdown/plain/HTML/PDF via services/tooling).

Markdown remains an **interchange/export** format and a convenient human-readable surface, not the sole canonical model.

### Change tracking + autosave (core, not app-specific)

We treat “Save” as legacy UX. The platform provides:

- **append-only OpLog (text)**: small, canonical, human-readable semantic operations for audit/history/blame,
- **snapshots (Cap’n Proto)**: fast load/zero-copy friendly state snapshots,
- deterministic **replay**: snapshot + oplog replay produces the same model state,
- bounded **compaction/rotation** of logs and snapshots.

Autosave is **always-on** and tied to the OpLog commit model.

### Sync (collaboration)

Collaboration is built on **syncing operation streams** (not whole-file overwrites).

- V1 focuses on “no overwrites / no data loss” semantics via OpLog-based sync.
- Realtime co-editing can be an additive layer (OT/CRDT/ordered transactions) but must preserve determinism and boundedness.

## Shared primitives (platform building blocks)

### 1) Zero-copy buffers for bulk (VMO/filebuffer)

- Provide a stable “bulk payload handle” story for apps and services.
- Align with `TASK-0031` for VMO handle transfer and RO mapping rules.

### 2) RichContent AST v1 (canonical)

Minimal, composable node set that supports:

- blocks: paragraph/heading/list/quote/code/table (baseline), attachments/embeds via handles,
- inlines: text, emphasis/strong/code/link, mention/token spans,
- stable IDs for blame and merges.

Import/export rules:

- HTML/RTF are **import sources only**, mapped through a sanitized subset deterministically.
- Export targets include Markdown/plain/HTML; roundtrip guarantees are explicit (no silent loss).

### 3) OpLog v1 (audit + autosave foundation)

Requirements:

- canonical text representation (stable ordering, stable formatting),
- stable object IDs (never “row 12” without a stable identity),
- explicit bounds (max line length, max ops per commit frame),
- secrets never logged (handles/ids only; no credentials/tokens).

Autosave mechanics (conceptual):

- on each change, append an OpLog entry (or a small batch within a bounded debounce),
- fsync/flush policy is deterministic and bounded,
- snapshots are taken periodically / after compaction thresholds,
- crash recovery replays OpLog entries after last snapshot.

### 4) Connector/provider framework (user plugs “their DB”)

We support “user plugs in a data backend” via installable providers:

- REST/HTTP and GraphQL gateways are first-class (via NexusNet surfaces),
- direct SQL backends (Postgres/MySQL/etc.) are possible as **providers/services**, not kernel features,
- apps obtain **capability-gated handles/grants**, not raw secrets.

Queries:

- prefer structured query objects (query IR/builder),
- optional text front-end parsing is allowed only if bounded and sanitized (“no stringly SQL everywhere”).

### 5) UI primitives pack (ecosystem-level UX)

Reusable widgets that make third-party apps “feel first-party”:

- DataSource picker + consent/grants UX hooks,
- Query editor (text + builder) with diagnostics,
- Virtualized table/grid view,
- Chart widgets (animated series updates, tooltips, zoom),
- Timeline widget (tracks/keyframes/triggers) for creative apps,
- Clipboard integration (RichContent).

## Gates (RED / YELLOW / GREEN)

- **RED (blocking)**:
  - VMO/filebuffer contract (`TASK-0031`) must exist to claim “zero-copy bulk”.
  - OpLog must be crash-safe and bounded (no fake durability).
- **YELLOW (risky / drift-prone)**:
  - “Full scripting language” scope creep; must be budgeted/sandboxed or deferred.
  - Query text parsing; must stay bounded and sanitized.
- **GREEN (confirmed direction)**:
  - RichContent as canonical interchange, Markdown as export/import format.
  - OpLog-based autosave + audit + collaboration.

## Phase map

- **Phase 0 (contracts + host proofs)**
  - RichContent AST node set + import/export mappings (goldens).
  - OpLog format + deterministic replay (goldens).
  - Autosave/recovery harness (crash simulation on host).
  - Clipboard v3 integration (negotiation + budgets; canonical RichContent payload where applicable).
  - Minimal connector interface (REST/GraphQL via NexusNet shapes).

- **Phase 1 (platform UI + OS wiring where feasible)**
  - UI primitives pack v1 (picker/query/table/chart/timeline skeletons).
  - VMO/filebuffer data-plane integration where kernel ABI permits (gated by `TASK-0031`).
  - OpLog sync v1a (LAN/peer semantics via localSim/DSoftBus direction).

- **Phase 2 (ecosystem hardening)**
  - Policy/audit integration as a first-class story (capability matrix + audit events).
  - Optional cloud sync service path (still capability-gated).
  - Provider ecosystem packaging rules and hardening guides.

## Candidate subtasks (to be extracted into TASK-XXXX)

- **CAND-ZCAP-000: RichContent AST v1 + clipboard payload contract**
- **CAND-ZCAP-010: Import pipeline v1 (html/rtf → sanitized → RichContent) + goldens**
- **CAND-ZCAP-020: OpLog v1 canonical text format + deterministic replay**
- **CAND-ZCAP-030: Autosave + recovery v1 (OpLog commit rules + snapshots + compaction)**
- **CAND-ZCAP-040: OpLog sync v1a (no-overwrite collaboration; bounded merge rules)**
- **CAND-ZCAP-050: Connector/provider framework v1 (handles/grants, query IR, bounds)**
- **CAND-ZCAP-060: UI primitives pack v1 (datasource/query/table/chart/timeline)**

## Extraction rules

A candidate becomes a real `TASK-XXXX` only when it:

- states bounds and determinism constraints explicitly,
- provides host-first proof requirements (goldens, replay tests, crash/recovery tests),
- does not introduce new kernel requirements unless split out as prerequisites,
- documents security invariants (no secrets in logs, policy-gated authority, no ambient access),
- avoids “fake success” markers and proves real behavior.
