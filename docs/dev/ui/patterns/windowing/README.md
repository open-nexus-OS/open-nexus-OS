<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Windowing Patterns

This subtree groups multiwindow, snap, resize, and movement patterns.

Current related docs:

- `window-intent.md` — **the window model**: app-owned Window *intent* ×
  environment-owned windowing *policy*; `chrome = intent ⟂ policy` (app declares,
  windowd composes, systemui supplies the policy). Read this first.
- `windows-as-widgets.md` — **the mechanism** (RFC-0067 P3+P4): a window is the
  `window` widget, content-sized, rendered through the retained scene graph;
  retire the hardcoded `ShellWindow` frames + the parallel hand-composite path.
- `docs/dev/ui/patterns/wm.md`
- `docs/dev/ui/patterns/wm-snap.md`
- `docs/dev/ui/patterns/wm-resize-move.md`
