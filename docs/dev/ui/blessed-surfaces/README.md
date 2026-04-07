<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Blessed Surfaces

Blessed surfaces are bounded specialized render/input cores that sit underneath a DSL-authored shell.

Use this category for:

- `NativeWidget` posture,
- embedded web surfaces,
- rich text/editor cores,
- chart/timeline/table heavy renderers,
- video/map/waveform/meters,
- and any specialized surface that should not turn the DSL into QML-like free scripting.

Current entry points:

- `docs/dev/dsl/syntax.md` (`NativeWidget` section)
- `docs/dev/ui/blessed-surfaces/webview.md`
- `docs/dev/ui/foundations/layout/layout-pipeline.md`

Related tasks/tracks:

- `tasks/TASK-0077C-dsl-v0_2c-pro-primitives-nativewidget-virtual-tables-timelines.md`
- `tasks/TRACK-ZEROCOPY-APP-PLATFORM.md`

Rule of thumb:

- if the shell should stay DSL-first but the render/input core is too heavy or too specialized for ordinary view nodes,
  it probably belongs here.
