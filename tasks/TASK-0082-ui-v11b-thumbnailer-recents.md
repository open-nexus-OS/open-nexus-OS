---
title: TASK-0082 UI v11b: thumbnailer (thumbd) + recents (recentsd) + caching/budgets
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - ADR: docs/adr/0022-modern-image-formats-avif-webp.md
  - Content providers: tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
  - SVG mini pipeline baseline: tasks/TASK-0057-ui-v2b-text-shaping-svg-pipeline.md
  - Theme/text baseline (TXT thumbnail): tasks/TASK-0057-ui-v2b-text-shaping-svg-pipeline.md
  - Persistence (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Policy as Code: tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Deterministic parallelism policy (thread pools): tasks/TASK-0276-parallelism-v1-deterministic-threadpools-policy-contract.md
  - Data formats rubric (JSON vs Cap'n Proto): docs/adr/0021-structured-data-formats-json-vs-capnp.md
---

## Context

Document access needs thumbnails and “recents” to feel usable. This task provides:

- `thumbd` thumbnail generation and cache,
- `recentsd` persistence and listing.

Picker UI and Open With wiring are in v11c.

## Goal

Deliver:

1. `thumbd` service:
   - input: `(uri, mime)` via `contentd.openUri` and returned stream
   - output: thumbnail BGRA VMO (e.g., 160×160) + metadata
   - supported v1: PNG/JPEG/WebP/SVG/TXT (AVIF as a follow-up; keep decoders small and safe)
   - LRU cache with strict byte budget, keyed by `(uri, rev)`
   - markers:
     - `thumbd: ready`
     - `thumbd: gen (mime=..., px=...)`
2. `recentsd` service:
   - append `{uri,mime,appId,tsNs}`
   - list(limit)
   - persisted under `/state/recents.nxs` (Cap'n Proto snapshot) once `/state` exists (host uses temp dir)
     - optional derived/debug view: `nx recents export --json` emits deterministic `recents.json`
   - markers:
     - `recentsd: ready`
     - `recentsd: added (uri=..., app=...)`
3. Host tests for thumbnail generation and recents persistence.

## Non-Goals

- Kernel changes.
- Full media decoding suite (keep decoders small and safe).
- Picker UI and “Open With…” UX (v11c).

## Constraints / invariants (hard requirements)

- Deterministic thumbnail outputs for fixture inputs (or SSIM threshold with documented tolerance).
- Strict budgets and bounded decode:
  - cap max input bytes read from stream,
  - cap max pixels produced.
- Parallelism (optional):
  - `thumbd` may use internal worker threads for parallel decode/raster, but must follow `TASK-0276`:
    fixed worker count, bounded job queue, deterministic output equivalence `workers=1` vs `workers=N`.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v11b_host/`:

- thumbnails:
  - PNG/JPEG/WebP/SVG/TXT fixtures produce expected checksum or golden PNGs
  - second request hits cache (counters improve)
  - budget pressure triggers deterministic eviction
- recents:
  - add entries, list in order
  - persistence survives restart (host simulated)

## Touched paths (allowlist)

- `source/services/thumbd/` (new)
- `source/services/recentsd/` (new)
- `tests/ui_v11b_host/`
- `docs/platform/thumbnailer.md` + `docs/dev/ui/recents.md` (new)

## Plan (small PRs)

1. thumbd: decode+render + cache + markers + tests
2. recentsd: persist/list + markers + tests
3. docs
