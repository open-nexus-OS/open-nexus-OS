<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Lazy Loading

Lazy loading is a UI contract for large data:

- deterministic paging tokens,
- stable ordering/tie-breakers,
- clear loading/error/empty states,
- bounded memory and bounded inflight requests.

## Relationship to QuerySpec

QuerySpec and lazy loading solve different layers:

- QuerySpec defines **what to load**,
- lazy loading defines **when to load more** and how the UI consumes paged results.

Recommended staged posture:

- QuerySpec v1: query value + effect-side execution + paging-token floor,
- QuerySpec v2: stronger defaults/hardening/ergonomics,
- lazy data surfaces: viewport/provider contract over those paged queries.

That keeps the query contract reusable across picker/files/history/feed/table surfaces while keeping viewport behavior in one
place.

Integration posture:

- visible-range triggers must be index/viewport based, never timer based,
- virtualized surfaces should keep stable scroll anchors while pages arrive,
- newly loaded items should invalidate only the affected measurement/placement ranges,
- and placeholder rows must obey the same bounded height/cell rules as real content.

## Recommended flow

Use this shape for large data surfaces:

1. state stores the current QuerySpec and the most recent page token,
2. reducer updates QuerySpec or paging state purely,
3. effect executes the query when requested,
4. virtual list consumes items/placeholders from a paged provider,
5. reaching a deterministic threshold emits the next-page event,
6. new results preserve anchor-by-key and invalidate only affected rows.

Typical surfaces:

- document picker provider results,
- Files list/grid/search views,
- browser history/bookmark lists,
- feed/timeline caches,
- and office-style virtualized tables.

See also: `docs/dev/ui/foundations/layout/layout-pipeline.md`.
