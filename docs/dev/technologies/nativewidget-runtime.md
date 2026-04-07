<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# NativeWidget Runtime

NativeWidget Runtime is the platform offering for app surfaces that outgrow ordinary DSL widgets but should still stay
inside the platform’s deterministic shell, sizing, and policy model.

## Primary task anchors

- `tasks/TASK-0077C-dsl-v0_2c-pro-primitives-nativewidget-virtual-tables-timelines.md`
- `tasks/TRACK-ZEROCOPY-APP-PLATFORM.md`

## Good fit

Use NativeWidget Runtime when your app has:

- charts, timelines, or complex visualizations,
- advanced editors,
- map-like or canvas-heavy surfaces,
- heavy web/text/media embeddings,
- or other cases where the surrounding shell should remain canonical but the inner surface needs a specialized engine.

Typical consumers:

- Maps and timeline-style apps,
- pro/editor-style tools,
- heavy table or data-exploration surfaces,
- advanced media or document viewers,
- and selected system-owned heavy surfaces such as WebView-like or specialized content panes.

## What users experience

When used well, users should experience:

- richer and more capable heavy surfaces,
- consistent surrounding app chrome and interaction behavior,
- fewer “special case” apps that feel off-platform,
- and deterministic shells even when the inner renderer is specialized.

## What it gives app developers

- a blessed path for heavy surfaces without abandoning the DSL shell,
- explicit boundaries for sizing, focus, invalidation, and data flow,
- a stable way to integrate specialized render/input engines,
- and a path to pair high-performance rendering with platform goldens and host-first testing.

## Best practice

- keep the surrounding shell DSL-authored,
- keep the specialized core bounded and explicit,
- use NativeWidget for the heavy center, not for ordinary rows/cards/forms that should stay normal UI,
- and preserve deterministic invalidation, sizing, and data-flow contracts at the boundary.

## Avoid

- using NativeWidget as a shortcut around ordinary UI architecture,
- letting the inner engine take ownership of unrelated shell state,
- or introducing unbounded custom runtimes with vague sizing/focus contracts.

## Related UI docs

- `docs/dev/ui/blessed-surfaces/README.md`
- `docs/dev/ui/blessed-surfaces/webview.md`

## Related docs

- `docs/dev/technologies/zero-copy-data-plane.md`
- `docs/dev/ui/foundations/layout/layout-pipeline.md`
