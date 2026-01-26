<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Files

Files UI is built on:

- `content://` URIs (stream handles),
- scoped grants (cross-subject access),
- background file operations with progress,
- trash/restore semantics.

This document also defines the “Finder-like” **display + preferences contract** for the Files app.

See also:

- Folder icon theming via template + emblem overlay: `docs/dev/ui/icon-guidelines.md`
- Icon sizes/strokes: `docs/dev/ui/icons.md`

## View modes (Finder-like)

The Files app should support a small set of canonical view modes:

- **List view**: dense rows, optional thumbnail column, best for sorting/filtering.
- **Grid view**: larger thumbnails/icons, best for media and touch.
- **Columns view** (optional, later): hierarchical browsing with preview pane.

Constraints:

- Views must be **deterministic** for goldens/tests: stable ordering, stable column defaults, stable truncation rules.
- Touch targets must remain reasonable across phone/tablet/desktop (see `docs/dev/ui/profiles.md`).

## Sorting, grouping, and display toggles

Expose Finder-like controls, but keep the underlying model deterministic:

- **Sort by**: name, kind (MIME), size, modified
- **Order**: ascending/descending
- **Grouping** (optional): kind/date
- **Show** toggles:
  - show hidden files
  - show file extensions
  - show preview pane / info pane
  - show path / breadcrumbs

Rules:

- Sorting must be stable (tie-break by deterministic key, e.g. URI/docId).
- “Kind” is derived from MIME/type resolution (no path-based heuristics).

## Tags / labels (Finder-style)

We want a Finder-like tagging system:

- **Color tags** (chips/badges) that are:
  - visible in list/grid,
  - filterable in the UI (facets),
  - and searchable (e.g. `tag:work`).

Tag colors must be chosen from the curated palette (no arbitrary RGB by default): `docs/dev/ui/colors.md`.

Design constraints:

- Treat tags as **user metadata** (can be sensitive). Do not expose tags to arbitrary apps by default.
- Prefer a **metadata index** model that works across providers (not all providers support xattrs).

Recommended model (v1):

- Store tags in a per-user metadata store keyed by stable doc identity (URI/docId), with deterministic ordering.
- Optionally mirror to provider xattrs only when supported and policy allows (do not require xattrs).

## Preferences and per-folder settings

We should integrate “Finder settings” into Files:

- **Global defaults** (apply to new folders/views):
  - default view mode (list/grid)
  - default sort key/order
  - default visible columns (list view)
  - show extensions / show hidden
- **Per-folder overrides** (optional, later):
  - last used view mode for this folder
  - last used sort/group
  - per-folder column configuration

Persistence guidance:

- Treat view preferences as **small durable UI state**:
  - persist via `settingsd` and/or a small `.nxs` snapshot (see the project’s data-format rubric).
- Keep it bounded:
  - cap the number of per-folder overrides retained (LRU eviction).

## Icon customization for folders

We support:

- folder color theming (user-chosen),
- and a per-folder emblem overlay (symbolic icon), composed at render-time.

See the contract: `docs/dev/ui/icon-guidelines.md` (“Folder icons” section).
