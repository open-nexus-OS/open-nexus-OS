<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Contrast And Visibility

This page groups contrast, visibility, focus indication, and related legibility expectations for all surfaces.

Primary anchors:

- `tasks/TASK-0118-ui-v20e-accessibility-settings-app-wiring-os-proofs.md`
- `tasks/TASK-0116-ui-v20c-magnifier-filters-high-contrast.md`
- `docs/dev/ui/foundations/visual/cursor-themes.md`

## High-Contrast Cursor Path

When high-contrast mode is active (for example `black` visual mode), cursor mapping
must route through the high-contrast cursor preset defined in cursor-theme policy.

Requirements:

- deterministic high-contrast cursor family/variant selection,
- HiDPI cursor size selection remains explicit and bounded,
- cursor visibility stays legible on bright, dark, and translucent surfaces.
