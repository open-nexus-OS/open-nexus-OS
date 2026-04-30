<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# UI Overview

This section documents the Nexus UI stack and how to build apps on it.

Principles:

- **Deterministic by default** (goldens, stable markers, stable layouts).
- **Cross-device** (phone/tablet/desktop share one design language).
- **No “menu bar laziness”**: prefer command surfaces and contextual actions over “FILE / EDIT / VIEW / HELP”.

## Information architecture

The UI docs are grouped by intent rather than by implementation detail.
The primary categories are the top-level navigation. Their second-level subtrees are the real working structure.

Use these categories as the primary entry points:

1. **Foundations**:
   - `docs/dev/ui/foundations/`
   - colors, typography, icons, motion, scaling, materials, layout/runtime/perf contracts
2. **Patterns**:
   - `docs/dev/ui/patterns/`
   - app shell, windowing, snap, resize, search-first shell posture, lifecycle/window conventions
3. **Components**:
   - `docs/dev/ui/components/`
   - standard reusable controls/primitives for apps and SystemUI
4. **Collections & data surfaces**:
   - `docs/dev/ui/collections/`
   - query-backed lists, virtualized collections, files/history-like surfaces, charts/tables/timelines
5. **Presentation**:
   - `docs/dev/ui/presentation/`
   - sheets, dialogs, popovers, overlays, toast, print/share-style presentation surfaces
6. **Navigation**:
   - `docs/dev/ui/navigation/`
   - sidebars, tab bars, breadcrumbs/path control, search/navigation surfaces
7. **Input & selection**:
   - `docs/dev/ui/input/`
   - focus, text input, gestures, IME, shortcuts, selection and editing posture
8. **Status & feedback**:
   - `docs/dev/ui/status/`
   - progress, badges, loading/empty/error, notifications, activity/rating/gauge-style indicators
9. **System experiences**:
   - `docs/dev/ui/system-experiences/`
   - document picker, share/open-with, browser shell, system delegation surfaces
10. **Blessed surfaces**:
    - `docs/dev/ui/blessed-surfaces/`
    - NativeWidget/blessed primitives for heavy specialized render/input cores

Important second-level roots:

- Foundations: `accessibility/`, `visual/`, `motion/`, `layout/`, `rendering/`, `quality/`
- Patterns: `app-structure/`, `windowing/`, `transfer-sharing/`, `identity-and-trust/`, `data-surfaces/`
- Components: `actions/`, `containers/`, `navigation/`, `input-and-selection/`, `status-and-feedback/`,
  `media-and-content/`
- System experiences: `system-delegation/`, `document-access/`, `browser/`, `capture-and-share/`

## Recommended Reading Order

1. Start with `docs/dev/ui/foundations/README.md` for system-wide rules.
2. Move to `docs/dev/ui/patterns/README.md` for app and shell structure.
3. Use `docs/dev/ui/components/README.md` for reusable controls.
4. Use `docs/dev/ui/system-experiences/README.md` when the OS should own the flow.
5. Use `docs/dev/ui/collections/README.md` for query-backed, lazy, or virtualized surfaces.

## Key Entrypoints

- Performance philosophy: `docs/dev/ui/foundations/quality/performance-philosophy.md`
- Testing & goldens: `docs/dev/ui/foundations/quality/testing.md`, `docs/dev/ui/foundations/quality/goldens.md`
- Headless `windowd` present proof: `docs/dev/ui/foundations/quality/testing.md` (`TASK-0055` section)
- Visible SystemUI first-frame proof: `docs/dev/ui/foundations/quality/testing.md` (`TASK-0055C` section)
- Accessibility semantics: `docs/accessibility/semantics.md`
- System delegation: `docs/dev/ui/system-experiences/system-delegation/overview.md`
- Document access: `docs/dev/ui/system-experiences/document-access/README.md`
- Input and editing: `docs/dev/ui/input/README.md`
- Notifications and status: `docs/dev/ui/status/README.md`

## Rule Of Thumb

- If the question is “which global rule applies everywhere?”, start in `Foundations`.
- If the question is “how should this whole flow or screen be structured?”, start in `Patterns` or `System Experiences`.
- If the question is “which reusable control or primitive should I use?”, start in `Components`.
- If the question is “how does a large data-heavy surface stay deterministic?”, start in `Collections`.
