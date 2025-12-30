---
title: TASK-0202 Text v2.1b (OS/QEMU): persistent font/glyph cache under /state + renderer atlas upload/damage hooks + text metrics overlay + nx-text CLI + selftests/docs
status: Draft
owner: @ui
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Text v2.1 host substrate: tasks/TASK-0201-text-v2_1a-host-hyphen-uax14-bidi-clusters-perf.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Shaping baseline (glyph cache in RAM): tasks/TASK-0057-ui-v2b-text-shaping-svg-pipeline.md
  - Compositor atlases/caches (deluxe): tasks/TASK-0060-ui-v4a-tiled-compositor-clipstack-atlases-perf.md
  - Font fallback selection (fontsel): tasks/TASK-0174-l10n-i18n-v1a-host-core-fluent-icu-fontsel-goldens.md
  - Renderer/windowd OS wiring: tasks/TASK-0170-renderer-abstraction-v1b-os-windowd-wiring-textshape-perf-markers.md
  - Policy caps baseline: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Text correctness (breaks/bidi/clusters) and deterministic host proofs live in v2.1a.
This task makes text performance real in OS/QEMU:

- persistent glyph cache (CPU raster → atlas pages) under `/state`,
- deterministic eviction and page layout,
- renderer hooks to upload atlas updates and mark damage tiles,
- and a dev metrics surface to validate behavior without fake success.

## Goal

Deliver:

1. Persistent glyph cache (“fontcache”) under `/state`:
   - fixed-endian index + deterministic pages layout
   - deterministic LRU eviction (counter-based; tie-break by key ordering)
   - cache key includes:
     - font hash, px size, glyph id, hint mode
   - explicit quotas:
     - max pages, max total bytes, max glyphs
   - markers:
     - `textcache: load pages=<n> bytes=<b>`
     - `textcache: insert glyph=<gid> px=<px>`
     - `textcache: evict n=<k>`
2. Renderer integration:
   - atlas upload path is swap-safe
   - atlas updates contribute to damage region/tile set deterministically
   - markers:
     - `renderer: atlas uploads=<n> tiles_impacted=<m>`
3. Text layout perf counters:
   - cache hit/miss counters (stable)
   - shaped glyphs per paragraph (stable)
   - time metrics are optional and must be time-injected or explicitly “debug-only”
4. Dev surfaces:
   - SystemUI overlay “Text Metrics” showing counters
   - Settings page “Fonts & Text”:
     - warm cache for locale set
     - clear cache
   - markers:
     - `ui: text metrics on`
     - `ui: fontcache clear`
5. CLI `nx-text` (host tool):
   - `stats`, `warm`, `clear-cache`, `bench --file pkg://...`
   - OS selftests must not depend on running host tools inside QEMU
6. OS selftests (bounded):
   - warm cache and shape a mixed paragraph:
     - `SELFTEST: text warm+shape ok`
   - toggle metrics overlay and observe counter change:
     - `SELFTEST: text metrics ok`
   - clear cache and re-shape; miss increases deterministically:
     - `SELFTEST: text cache clear ok`
   - hyphenation wrap proof (fixture string):
     - `SELFTEST: text hyphen ok`

## Non-Goals

- Color emoji rendering (explicitly not in v2.1).
- “Full” atlas/caches ecosystem (glyph/SVG/blur) and pacing is `TASK-0060`.

## Constraints / invariants (hard requirements)

- `/state` gating:
  - without `TASK-0009`, persistence must be `stub/placeholder` and must not claim cross-run caching.
- No fake success:
  - cache warm/clear tests must be based on counters, not log greps.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p text_v2_1_host -- --nocapture` (from v2.1a)

- **Proof (QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=185s ./scripts/qemu-test.sh`
  - Required markers:
    - `SELFTEST: text warm+shape ok`
    - `SELFTEST: text metrics ok`
    - `SELFTEST: text cache clear ok`
    - `SELFTEST: text hyphen ok`

## Touched paths (allowlist)

- `userspace/libs/textcache/` (new or extend existing)
- `userspace/libs/textlayout/` / `userspace/libs/textshape/` (integration)
- `userspace/libs/renderer/` (atlas upload + damage plumbing)
- SystemUI overlays + Settings pages
- `tools/nx-text/` (host tool)
- `source/apps/selftest-client/`
- `schemas/text.schema.json` + `pkg://fixtures/text/`
- `docs/text/` + `docs/tools/nx-text.md`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. persistent cache format + quotas + load/save + markers
2. renderer atlas upload/damage hooks
3. SystemUI overlay + Settings warm/clear
4. nx-text tool
5. OS selftests + docs + postflight wrapper (delegating)

## Acceptance criteria (behavioral)

- In QEMU, cache warm/clear and hyphen wrap behavior are proven deterministically by selftest markers and counters.

