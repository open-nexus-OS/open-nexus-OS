<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Collections & Data Surfaces

This category covers query-backed, virtualized, scroll-heavy, or data-dense UI surfaces.

Use it for:

- virtual lists/grids/tables,
- lazy loading and paging,
- files/history/search-like list shells,
- collection-oriented scroll/anchor behavior,
- and chart/timeline/table posture when data scale matters.

Current entry points:

- `docs/dev/ui/collections/widgets/virtual-list.md`
- `docs/dev/ui/collections/lazy-loading.md`
- `docs/dev/ui/collections/files.md`
- `docs/dev/ui/blessed-surfaces/webview.md`
- `docs/dev/ui/navigation/recents.md`
- `docs/dev/ui/foundations/layout/scroll.md`
- `docs/dev/ui/foundations/layout/layout-pipeline.md`

Related DSL contracts:

- `docs/dev/dsl/db-queries.md`
- `docs/dev/dsl/services.md`
- `docs/dev/dsl/state.md`

Rule of thumb:

- if the UI is driven by filter/order/page state or needs virtualization to stay healthy, start here.
