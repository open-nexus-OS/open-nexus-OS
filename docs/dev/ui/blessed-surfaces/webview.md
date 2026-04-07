<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# WebView (UI)

WebView UI covers:

- embedding/offscreen rendering model,
- navigation and policy surfaces,
- downloads/history UX (where enabled),
- deterministic fixtures and selftests.

## Browser data posture

Browser-facing data surfaces such as:

- history,
- recent searches,
- later bookmarks/favorites,
- and export/search/filter views

should be treated as **queryable UI data**, not as ad-hoc per-screen arrays.

Recommended posture:

- visible browser shell/chrome remains DSL-authored,
- history/bookmark lists use QuerySpec-style filtering/ordering/paging contracts,
- execution stays service-gated or library-gated behind a browser storage abstraction,
- and the storage backend remains replaceable.

This means:

- a deterministic snapshot/log backend is a valid default,
- an optional SQL/libSQL backend may exist later,
- but the UI should depend on the query contract, not on a specific engine.

## Lazy execution posture

Browser data should follow the same pure/effect split as other data-heavy surfaces:

- build/update query state purely,
- execute recent/search/history queries from effects or shell adapters,
- page lazily when the visible range requires it,
- and keep command flows such as open/reload/download/clear-history as explicit domain actions.
